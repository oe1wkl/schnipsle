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

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use serde::Serialize;

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
                let facts = analyze(&file.key, &String::from_utf8_lossy(&bytes));
                index.put(&file.key, file.state, &stamp, &facts)?;
                report.indexed += 1;
            }
        }
        for key in stamps.keys().filter(|key| !seen.contains(*key)) {
            index.remove(key)?;
            report.removed += 1;
        }
        Ok(report)
    }

    /// Updates the index for one path below the vault root: a note in
    /// `notes/…` or a trashed note in `.notesapp/trash/…`. A path without a
    /// file is removed from the index.
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
        let facts = analyze(key, &String::from_utf8_lossy(&bytes));
        index.put(key, state, &stamp, &facts)?;
        Ok(())
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
}
