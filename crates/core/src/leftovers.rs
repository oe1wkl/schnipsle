// SPDX-License-Identifier: Apache-2.0
//! Temporary files left behind by a write that was cut off (ADR-0042 rule 4).
//!
//! Every write goes through a temporary file next to its target,
//! `.<name>.tmp-<process>-<n>` (format.md §10, [`crate::vault::TempFile`]),
//! which is removed or renamed into place when the write ends. Only a crash
//! or a kill leaves one behind. It may hold the complete new text, flushed
//! but not yet placed, or only the start of it.
//!
//! [`Vault::clear_leftovers`] removes such files once they are an hour old;
//! a younger one may belong to a write in progress on another device. Before
//! a note's temporary file is removed, its text is stored in the note's
//! history if it has a valid `id` and differs from the note, so the last
//! text typed before a crash can be found there.

use std::fs;
use std::io;
use std::path::Path;
use std::time::UNIX_EPOCH;

use crate::front_matter;
use crate::layout::{ATTACHMENTS_DIR, NOTES_DIR};
use crate::stamp::is_ulid;
use crate::vault::{Access, Result, Vault};

/// How old a temporary file must be before it counts as left behind.
pub const LEFTOVER_AGE_MS: u64 = 60 * 60 * 1000;

/// What [`Vault::clear_leftovers`] did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Leftovers {
    /// Notes (relative to `notes/`) whose leftover text went into their history.
    pub stored: Vec<String>,
    /// How many temporary files were removed.
    pub removed: usize,
    /// Temporary files kept because their text could not be stored, or they
    /// could not be removed, with why; relative to the vault root.
    pub kept: Vec<(String, String)>,
}

/// `true` for a temporary file name as [`crate::vault::TempFile`] makes them.
fn is_temp_name(name: &str) -> bool {
    let Some((target, tail)) = name.strip_prefix('.').and_then(|n| n.rsplit_once(".tmp-")) else {
        return false;
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    !target.is_empty()
        && tail
            .split_once('-')
            .is_some_and(|(p, n)| digits(p) && digits(n))
}

impl Vault {
    /// Removes temporary files older than [`LEFTOVER_AGE_MS`] at `now_ms` in
    /// `notes/`, `attachments/` and `.notesapp/`; a note's leftover text goes
    /// into its history first (see the module documentation). A file whose
    /// text cannot be stored is kept, to be tried again next time.
    ///
    /// # Errors
    /// [`crate::VaultError::ReadOnly`]; I/O errors reading the folders.
    /// Problems with single files are reported in [`Leftovers::kept`].
    pub fn clear_leftovers(&self, now_ms: u64) -> Result<Leftovers> {
        self.check_writable()?;
        let mut found = Leftovers::default();
        for dir in [NOTES_DIR, ATTACHMENTS_DIR, ".notesapp"] {
            self.clear_in(&self.root().join(dir), dir, now_ms, &mut found)?;
        }
        Ok(found)
    }

    fn clear_in(&self, dir: &Path, rel: &str, now_ms: u64, found: &mut Leftovers) -> Result<()> {
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(()),
            Err(e) => return Err(e.into()),
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let path = entry.path();
            let rel = format!("{rel}/{name}");
            let Ok(meta) = entry.metadata() else { continue };
            if meta.is_dir() {
                self.clear_in(&path, &rel, now_ms, found)?;
                continue;
            }
            if !is_temp_name(&name) {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(u64::MAX, |d| {
                    u64::try_from(d.as_millis()).unwrap_or(u64::MAX)
                });
            if now_ms.saturating_sub(modified) < LEFTOVER_AGE_MS {
                continue;
            }
            match self.clear_one(&path, &rel) {
                Ok(Some(note)) => {
                    found.stored.push(note);
                    found.removed += 1;
                }
                Ok(None) => found.removed += 1,
                Err(e) => found.kept.push((rel, e.to_string())),
            }
        }
        Ok(())
    }

    /// Stores the text of a note's temporary file at `path` in the note's
    /// history if it is new there, then removes the file. Returns the note
    /// if its text was stored.
    fn clear_one(&self, path: &Path, rel: &str) -> Result<Option<String>> {
        let note = rel
            .strip_prefix(NOTES_DIR)
            .and_then(|r| r.strip_prefix('/'))
            .and_then(|r| {
                let (folder, name) = r.rsplit_once('/').map_or(("", r), |(f, n)| (f, n));
                let target = name.strip_prefix('.')?.rsplit_once(".tmp-")?.0;
                let note = if folder.is_empty() {
                    target.to_owned()
                } else {
                    format!("{folder}/{target}")
                };
                self.resolve(&note).ok().map(|full| (note, full))
            });
        let mut stored = None;
        if let Some((note, full)) = note {
            let bytes = self.coordinated(Access::Read, path, |file| Ok(fs::read(file)?))?;
            let id =
                front_matter::get(&String::from_utf8_lossy(&bytes), "id").filter(|id| is_ulid(id));
            let current = self.coordinated(Access::Read, &full, |file| Ok(fs::read(file).ok()))?;
            if let Some(id) = id
                && current.as_deref() != Some(bytes.as_slice())
                && self.snapshot_if_new(&id, &bytes)?.is_some()
            {
                stored = Some(note);
            }
        }
        self.coordinated(Access::Delete, path, |file| match fs::remove_file(file) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e.into()),
            _ => Ok(()),
        })?;
        Ok(stored)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, SystemTime};

    const ID: &str = "01HF7YAT04EXCA2ARJ0J9F8K0P";

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    fn now() -> u64 {
        crate::stamp::now_ms()
    }

    #[test]
    fn temp_names() {
        assert!(is_temp_name(".Note.md.tmp-123-0"));
        assert!(is_temp_name(".20261119T101010Z.md.zst.tmp-9-42"));
        assert!(!is_temp_name("Note.md.tmp-123-0"));
        assert!(!is_temp_name(".Note.md.tmp-123"));
        assert!(!is_temp_name(".Note.md.tmp-a-1"));
        assert!(!is_temp_name(".DS_Store"));
        assert!(!is_temp_name(".tmp-1-2"));
    }

    #[test]
    fn a_leftover_note_text_goes_into_the_history() {
        let (dir, vault) = vault();
        let note = format!("---\nid: {ID}\n---\n# N\n\nsaved\n");
        let lost = format!("---\nid: {ID}\n---\n# N\n\nsaved, and typed before the crash\n");
        let notes = dir.path().join("notes/sub");
        fs::create_dir_all(&notes).unwrap();
        fs::write(notes.join("N.md"), &note).unwrap();
        fs::write(notes.join(".N.md.tmp-77-0"), &lost).unwrap();
        fs::write(notes.join(".N.md.tmp-77-1"), &note).unwrap(); // the same as the note

        // Too young: left alone.
        assert_eq!(vault.clear_leftovers(now()).unwrap(), Leftovers::default());
        assert!(notes.join(".N.md.tmp-77-0").exists());

        let later = now() + LEFTOVER_AGE_MS + 1000;
        let found = vault.clear_leftovers(later).unwrap();
        assert_eq!(found.stored, ["sub/N.md"]);
        assert_eq!(found.removed, 2);
        assert!(found.kept.is_empty());
        let names = vault.snapshots(ID).unwrap();
        assert_eq!(names.len(), 1);
        assert_eq!(vault.read_snapshot(ID, &names[0]).unwrap(), lost);
        assert_eq!(fs::read_dir(&notes).unwrap().count(), 1);
    }

    #[test]
    fn other_leftovers_are_removed() {
        let (dir, vault) = vault();
        let trash = dir.path().join(".notesapp/trash");
        fs::create_dir_all(&trash).unwrap();
        fs::write(trash.join(format!(".{ID}.yaml.tmp-5-3")), "deleted: x\n").unwrap();
        // Cut short in the front matter: no id, nothing to store.
        fs::write(dir.path().join("notes/.cut.md.tmp-5-4"), "---\nid: 01HF").unwrap();
        fs::write(dir.path().join("notes/.hidden"), "not ours").unwrap();
        let found = vault
            .clear_leftovers(now() + LEFTOVER_AGE_MS + 1000)
            .unwrap();
        assert_eq!(found.removed, 2);
        assert!(found.stored.is_empty());
        assert!(dir.path().join("notes/.hidden").exists());
        assert_eq!(fs::read_dir(&trash).unwrap().count(), 0);
    }

    #[test]
    fn a_young_file_is_kept_by_its_age() {
        let (dir, vault) = vault();
        let file = dir.path().join("notes/.N.md.tmp-1-1");
        fs::write(&file, "x").unwrap();
        let f = fs::File::options().write(true).open(&file).unwrap();
        f.set_modified(SystemTime::now() - Duration::from_secs(30 * 60))
            .unwrap();
        assert_eq!(vault.clear_leftovers(now()).unwrap().removed, 0);
        f.set_modified(SystemTime::now() - Duration::from_secs(2 * 60 * 60))
            .unwrap();
        assert_eq!(vault.clear_leftovers(now()).unwrap().removed, 1);
    }
}
