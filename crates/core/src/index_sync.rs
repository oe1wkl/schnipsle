// SPDX-License-Identifier: Apache-2.0
//! Keeps the search index (ADR-0009) in step with the vault.
//!
//! [`Vault::sync_index`] compares the index with the notes in `notes/` and
//! the trash: a file whose modification time and size match its stamp is
//! skipped, one whose content hash still matches only gets a new stamp, the
//! rest is read and analyzed again; entries without a file are removed. It
//! runs when a vault opens and after events that may have been missed.
//! [`Vault::index_file`] updates one path, after a change event or the app's
//! own write. Notes that are only iCloud placeholders are not indexed until
//! they are downloaded.
//!
//! Both give a note back its `id` and `created` when a change made
//! elsewhere removed them (ADR-0044, format.md §4): the index still has them
//! from the version before.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;

use crate::front_matter;
use crate::index::{FileStamp, Index, IndexError, NoteState, analyze};
use crate::layout::NOTES_DIR;
use crate::trash::TRASH_DIR;
use crate::vault::{Access, Result, Vault, VaultError, sha256_hex};

impl From<IndexError> for VaultError {
    fn from(e: IndexError) -> Self {
        Self::Io(io::Error::other(e.to_string()))
    }
}

/// What [`Vault::sync_index`] did. Serialized with camelCase names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct IndexSync {
    /// Notes read and (re)indexed.
    pub indexed: usize,
    /// Notes whose files were touched but unchanged.
    pub touched: usize,
    /// Index entries removed because their file is gone.
    pub removed: usize,
    /// Notes left as they were in the index because they could not be read.
    pub failed: usize,
    /// Notes whose `id` and `created` were written back (ADR-0044).
    pub restored: usize,
}

/// A note whose file lost its `id` through a change made elsewhere, with the
/// `id` and `created` its version before had (ADR-0044).
struct Lost {
    key: String,
    id: String,
    created: Option<String>,
}

/// A note file found on disk: its key (path below the vault root), state
/// and the parts of its stamp that need no reading.
struct Found {
    key: String,
    state: NoteState,
    mtime_ms: u64,
    size: u64,
}

impl Vault {
    /// Brings `index` in step with the notes and the trash on disk.
    ///
    /// # Errors
    /// I/O errors listing `notes/`, or index errors; a single note that
    /// cannot be read only counts as `failed`.
    pub fn sync_index(&self, index: &mut Index) -> Result<IndexSync> {
        let mut found = Vec::new();
        walk(self.notes_dir(), NOTES_DIR, NoteState::Active, &mut found)?;
        let trash = self.root().join(TRASH_DIR);
        if trash.is_dir() {
            walk(&trash, TRASH_DIR, NoteState::Trash, &mut found)?;
        }

        let stamps = index.stamps()?;
        let mut report = IndexSync::default();
        let mut seen = HashSet::new();
        let mut lost = Vec::new();
        for file in found {
            seen.insert(file.key.clone());
            if let Some(old) = stamps.get(&file.key)
                && old.mtime_ms == file.mtime_ms
                && old.size == file.size
            {
                continue;
            }
            let bytes = match self.read_for_index(&file.key) {
                Ok(bytes) => bytes,
                Err(e) => {
                    eprintln!("not indexed: {}: {e}", file.key);
                    report.failed += 1;
                    continue;
                }
            };
            let stamp = FileStamp {
                mtime_ms: file.mtime_ms,
                size: file.size,
                hash: sha256_hex(&bytes),
            };
            if stamps
                .get(&file.key)
                .is_some_and(|old| old.hash == stamp.hash)
            {
                index.set_stamp(&file.key, &stamp)?;
                report.touched += 1;
            } else {
                lost.extend(lost_identity(index, &file.key, file.state, &bytes)?);
                let facts = analyze(&file.key, &String::from_utf8_lossy(&bytes));
                index.put(&file.key, file.state, &stamp, &facts)?;
                report.indexed += 1;
            }
        }
        for key in stamps.keys().filter(|key| !seen.contains(*key)) {
            index.remove(key)?;
            report.removed += 1;
        }
        // Last, so that a note moved or copied elsewhere is in the index by now.
        for lost in lost {
            if self.restore_identity(index, &lost)? {
                report.restored += 1;
            }
        }
        Ok(report)
    }

    /// Updates the index for one path below the vault root: a note in
    /// `notes/…` or a trashed note in `.notesapp/trash/…`. A path without a
    /// file is removed from the index. A note that lost its `id` gets it
    /// back (ADR-0044).
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] for a path that is neither, I/O errors
    /// reading it, or index errors.
    pub fn index_file(&self, index: &mut Index, key: &str) -> Result<()> {
        let (file, state) = self.index_target(key)?;
        let meta = match fs::metadata(&file) {
            Ok(meta) if meta.is_file() => meta,
            Ok(_) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => {
                index.remove(key)?;
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };
        let bytes = self.coordinated(Access::Read, &file, |file| Ok(fs::read(file)?))?;
        let stamp = FileStamp {
            mtime_ms: mtime_ms(&meta),
            size: meta.len(),
            hash: sha256_hex(&bytes),
        };
        let lost = lost_identity(index, key, state, &bytes)?;
        let facts = analyze(key, &String::from_utf8_lossy(&bytes));
        index.put(key, state, &stamp, &facts)?;
        if let Some(lost) = lost {
            self.restore_identity(index, &lost)?;
        }
        Ok(())
    }

    /// Writes the `id` and `created` of `lost` back into the front matter of
    /// its note and indexes the result, unless another note (in the trash
    /// too) has that `id` now, the vault is read-only, the note is not UTF-8
    /// or changed again meanwhile, or a write fails; then the note is left as
    /// it is and gets a new `id` at its next save. Returns whether it wrote.
    fn restore_identity(&self, index: &mut Index, lost: &Lost) -> Result<bool> {
        if self.check_writable().is_err() || index.id_used_elsewhere(&lost.id, &lost.key)? {
            return Ok(false);
        }
        let Some(path) = lost
            .key
            .strip_prefix(NOTES_DIR)
            .and_then(|k| k.strip_prefix('/'))
        else {
            return Ok(false);
        };
        let Ok(full) = self.resolve(path) else {
            return Ok(false);
        };
        let Ok(bytes) = self.read_for_index(&lost.key) else {
            return Ok(false);
        };
        let hash = sha256_hex(&bytes);
        let Ok(text) = String::from_utf8(bytes) else {
            return Ok(false);
        };
        if front_matter::get(&text, "id").is_some() {
            return Ok(false); // changed again, and has an id now
        }
        let mut stamped = text;
        if let Some(created) = lost
            .created
            .as_deref()
            .filter(|_| front_matter::get(&stamped, "created").is_none())
        {
            stamped = front_matter::set(&stamped, "created", created);
        }
        stamped = front_matter::set(&stamped, "id", &lost.id);
        match self.replace_expected(path, &full, &hash, stamped) {
            Ok(_) => {}
            Err(VaultError::Conflict { .. }) => return Ok(false),
            Err(e) => {
                eprintln!("id not written back: {}: {e}", lost.key);
                return Ok(false);
            }
        }
        self.index_file(index, &lost.key)?;
        Ok(true)
    }

    fn read_for_index(&self, key: &str) -> Result<Vec<u8>> {
        let (file, _) = self.index_target(key)?;
        self.coordinated(Access::Read, &file, |file| Ok(fs::read(file)?))
    }

    /// The file and state for an index key, checked like any note path.
    fn index_target(&self, key: &str) -> Result<(std::path::PathBuf, NoteState)> {
        let invalid = || VaultError::InvalidPath(key.to_owned());
        if let Some(path) = key
            .strip_prefix(NOTES_DIR)
            .and_then(|k| k.strip_prefix('/'))
        {
            return Ok((self.resolve(path)?, NoteState::Active));
        }
        let name = key
            .strip_prefix(TRASH_DIR)
            .and_then(|k| k.strip_prefix('/'))
            .ok_or_else(invalid)?;
        let valid = name.to_ascii_lowercase().ends_with(".md")
            && !name.starts_with('.')
            && !name.contains(['/', '\\']);
        if !valid {
            return Err(invalid());
        }
        Ok((self.root().join(TRASH_DIR).join(name), NoteState::Trash))
    }
}

/// The `id` and `created` to give back to the note at `key` if its new
/// content `bytes` has no `id` but its version in the index had one; a note
/// in the trash keeps what it has.
fn lost_identity(index: &Index, key: &str, state: NoteState, bytes: &[u8]) -> Result<Option<Lost>> {
    if state != NoteState::Active
        || front_matter::get(&String::from_utf8_lossy(bytes), "id").is_some()
    {
        return Ok(None);
    }
    Ok(match index.identity(key)? {
        Some((Some(id), created)) => Some(Lost {
            key: key.to_owned(),
            id,
            created,
        }),
        _ => None,
    })
}

/// Collects the `.md` files below `dir` (hidden files and folders skipped,
/// so iCloud placeholders too), keyed by `prefix/…`.
fn walk(dir: &Path, prefix: &str, state: NoteState, out: &mut Vec<Found>) -> io::Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(e) if out.is_empty() && prefix == NOTES_DIR => return Err(e),
        Err(_) => return Ok(()), // an unreadable subfolder must not stop the rest
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let key = format!("{prefix}/{name}");
        if meta.is_dir() {
            if state == NoteState::Active {
                walk(&entry.path(), &key, state, out)?;
            }
        } else if meta.is_file() && name.len() > 3 && name.to_ascii_lowercase().ends_with(".md") {
            out.push(Found {
                key,
                state,
                mtime_ms: mtime_ms(&meta),
                size: meta.len(),
            });
        }
    }
    Ok(())
}

fn mtime_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn setup() -> (tempfile::TempDir, Vault, Index) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault, Index::in_memory().unwrap())
    }

    fn paths(index: &Index) -> Vec<String> {
        let mut paths: Vec<_> = index.stamps().unwrap().into_keys().collect();
        paths.sort();
        paths
    }

    #[test]
    fn sync_indexes_notes_and_trash_and_only_what_changed() {
        let (dir, vault, mut index) = setup();
        let root = dir.path();
        vault.create_note("a.md", "# A #eins\n").unwrap();
        vault.create_note("Projekte/b.md", "# B\n").unwrap();
        vault.create_note("weg.md", "# Weg\n").unwrap();
        vault.trash_note("weg.md").unwrap();
        fs::write(root.join("notes/.c.md.icloud"), "placeholder").unwrap();
        fs::write(root.join("notes/bild.png"), "not a note").unwrap();

        let first = vault.sync_index(&mut index).unwrap();
        assert_eq!(first.indexed, 3);
        let all = paths(&index);
        assert_eq!(all.len(), 3);
        assert!(all.contains(&"notes/a.md".to_owned()));
        assert!(all.contains(&"notes/Projekte/b.md".to_owned()));
        assert!(
            all.iter()
                .any(|p| p.starts_with(".notesapp/trash/")
                    && p.to_ascii_lowercase().ends_with(".md"))
        );

        // Nothing changed: nothing is read again.
        assert_eq!(vault.sync_index(&mut index).unwrap(), IndexSync::default());

        // Edited, touched, deleted.
        let a = vault.read_note("a.md").unwrap();
        vault.write_note("a.md", "# A neu\n", &a.hash).unwrap();
        let b = root.join("notes/Projekte/b.md");
        let content = fs::read(&b).unwrap();
        fs::write(&b, b"x").unwrap();
        fs::write(&b, &content).unwrap(); // same content, new size history and mtime
        fs::remove_file(root.join("notes/a.md")).unwrap();
        vault.create_note("neu.md", "# Neu\n").unwrap();
        let report = vault.sync_index(&mut index).unwrap();
        assert_eq!(report.removed, 1);
        assert_eq!(report.indexed, 1);
        assert!(report.touched <= 1, "{report:?}"); // depends on the file system's mtime resolution
        assert!(paths(&index).contains(&"notes/neu.md".to_owned()));
        assert!(!paths(&index).contains(&"notes/a.md".to_owned()));
    }

    #[test]
    fn single_files_follow_writes_moves_and_the_trash() {
        let (_dir, vault, mut index) = setup();
        vault.create_note("a.md", "# A\n").unwrap();
        vault.index_file(&mut index, "notes/a.md").unwrap();
        assert_eq!(paths(&index), ["notes/a.md"]);

        let trashed = vault.trash_note("a.md").unwrap();
        let trash_key = format!("{TRASH_DIR}/{}.md", trashed.key);
        vault.index_file(&mut index, "notes/a.md").unwrap();
        vault.index_file(&mut index, &trash_key).unwrap();
        assert_eq!(paths(&index), std::slice::from_ref(&trash_key));

        let restored = vault.restore_note(&trashed.key).unwrap();
        vault.index_file(&mut index, &trash_key).unwrap();
        vault
            .index_file(&mut index, &format!("{NOTES_DIR}/{restored}"))
            .unwrap();
        assert_eq!(paths(&index), ["notes/a.md"]);

        for bad in [
            "a.md",
            "notes/../x.md",
            ".notesapp/vault.yaml",
            ".notesapp/trash/../x.md",
        ] {
            assert!(
                matches!(
                    vault.index_file(&mut index, bad),
                    Err(VaultError::InvalidPath(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn the_index_knows_the_trash_state() {
        let (_dir, vault, mut index) = setup();
        vault
            .create_note("a.md", "---\nid: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T\n---\n# A\n")
            .unwrap();
        vault.sync_index(&mut index).unwrap();
        assert_eq!(
            index.paths_with_id("01JA7Q5Y0Z8H3K2M6N4P9R1S2T").unwrap(),
            ["notes/a.md"]
        );
        vault.trash_note("a.md").unwrap();
        vault.sync_index(&mut index).unwrap();
        assert!(
            index
                .paths_with_id("01JA7Q5Y0Z8H3K2M6N4P9R1S2T")
                .unwrap()
                .is_empty()
        );
        assert_eq!(index.len().unwrap(), 1);
    }

    const ID: &str = "01JA7Q5Y0Z8H3K2M6N4P9R1S2T";

    /// format.md §4, ADR-0044: the golden files in `tests/fixtures/format/4-keep-id/`.
    fn keep_id_fixture(name: &str) -> String {
        let dir = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/4-keep-id");
        fs::read_to_string(dir.join(name)).unwrap()
    }

    #[test]
    fn a_note_whose_id_was_removed_elsewhere_gets_it_back() {
        let (dir, vault, mut index) = setup();
        let file = dir.path().join("notes/tuner.md");
        fs::write(&file, keep_id_fixture("before.md")).unwrap();
        vault.sync_index(&mut index).unwrap();
        // A version in the history, filed under the id (format.md §7).
        vault.snapshot_note("tuner.md", None).unwrap();

        fs::write(&file, keep_id_fixture("changed.md")).unwrap();
        let report = vault.sync_index(&mut index).unwrap();
        assert_eq!(report.restored, 1, "{report:?}");
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            keep_id_fixture("after.md")
        );
        assert_eq!(index.paths_with_id(ID).unwrap(), ["notes/tuner.md"]);
        assert_eq!(vault.versions("tuner.md").unwrap().len(), 1);
        // Nothing more to do.
        assert_eq!(vault.sync_index(&mut index).unwrap(), IndexSync::default());

        // The same after a single change event.
        fs::write(&file, keep_id_fixture("changed.md")).unwrap();
        vault.index_file(&mut index, "notes/tuner.md").unwrap();
        assert_eq!(
            fs::read_to_string(&file).unwrap(),
            keep_id_fixture("after.md")
        );
        assert_eq!(index.paths_with_id(ID).unwrap(), ["notes/tuner.md"]);
    }

    #[test]
    fn an_id_carried_elsewhere_is_not_given_back() {
        let (dir, vault, mut index) = setup();
        let notes = dir.path().join("notes");
        fs::write(notes.join("tuner.md"), keep_id_fixture("before.md")).unwrap();
        vault.sync_index(&mut index).unwrap();

        // Moved elsewhere, and a copy without front matter left behind.
        fs::write(notes.join("moved.md"), keep_id_fixture("before.md")).unwrap();
        fs::write(notes.join("tuner.md"), keep_id_fixture("changed.md")).unwrap();
        let report = vault.sync_index(&mut index).unwrap();
        assert_eq!(report.restored, 0);
        assert_eq!(
            fs::read_to_string(notes.join("tuner.md")).unwrap(),
            keep_id_fixture("changed.md")
        );

        // In the trash, the id is taken as well.
        fs::remove_file(notes.join("tuner.md")).unwrap();
        vault.sync_index(&mut index).unwrap();
        fs::write(notes.join("tuner.md"), keep_id_fixture("before.md")).unwrap();
        fs::remove_file(notes.join("moved.md")).unwrap();
        vault.sync_index(&mut index).unwrap();
        fs::write(notes.join("copy.md"), keep_id_fixture("before.md")).unwrap();
        vault.sync_index(&mut index).unwrap();
        vault.trash_note("copy.md").unwrap();
        vault.sync_index(&mut index).unwrap();
        fs::write(notes.join("tuner.md"), keep_id_fixture("changed.md")).unwrap();
        vault.index_file(&mut index, "notes/tuner.md").unwrap();
        assert_eq!(
            fs::read_to_string(notes.join("tuner.md")).unwrap(),
            keep_id_fixture("changed.md")
        );
    }

    #[test]
    fn other_changes_keep_what_they_have() {
        let (dir, vault, mut index) = setup();
        let notes = dir.path().join("notes");
        fs::write(notes.join("tuner.md"), keep_id_fixture("before.md")).unwrap();
        fs::write(notes.join("plain.md"), "# Plain\n").unwrap();
        vault.sync_index(&mut index).unwrap();

        // Another id, even one that is not valid, is the other program's choice.
        let other = "---\nid: not-a-ulid\n---\n# Antennentuner\n";
        fs::write(notes.join("tuner.md"), other).unwrap();
        // A note that never had an id gets one only at its next save.
        fs::write(notes.join("plain.md"), "# Plain\n\nmore\n").unwrap();
        let report = vault.sync_index(&mut index).unwrap();
        assert_eq!(report.restored, 0);
        assert_eq!(fs::read_to_string(notes.join("tuner.md")).unwrap(), other);
        assert_eq!(
            fs::read_to_string(notes.join("plain.md")).unwrap(),
            "# Plain\n\nmore\n"
        );
    }
}
