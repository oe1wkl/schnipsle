// SPDX-License-Identifier: Apache-2.0
//! Conflict versions kept by a sync service (ADR-0006).
//!
//! When two devices change a note before they sync, iCloud Drive keeps one text
//! as the file and the other as a hidden "conflict version". Both saves were
//! correct where they happened, so the hash check of [`Vault::write_note`] never
//! sees this; only the sync service knows. A [`SyncConflicts`] implementation
//! (`schnipsle-apple` for iCloud) lists those versions, and
//! [`Vault::resolve_conflict`] settles them: every text the user does not keep
//! goes into the note's version history first (ADR-0011), and only versions
//! that are safely stored are marked resolved.

use std::collections::BTreeSet;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::front_matter;
use crate::stamp::{is_ulid, new_ulid, now_ms, rfc3339_utc};
use crate::vault::{
    Access, NoteContent, Result, Vault, VaultError, created_ms, modified_ms_of, sha256_hex,
};

/// A conflict version as the sync service stores it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawVersion {
    /// The complete file content of that version.
    pub bytes: Vec<u8>,
    /// When it was saved, milliseconds since the Unix epoch.
    pub modified_ms: u64,
    /// Name of the device that saved it, if known.
    pub device: Option<String>,
}

/// Lists and resolves conflict versions of a file.
pub trait SyncConflicts: Send + Sync + fmt::Debug {
    /// The unresolved conflict versions of `file`; empty if there are none.
    ///
    /// # Errors
    /// If the sync service cannot be asked or a version cannot be read.
    fn versions(&self, file: &Path) -> io::Result<Vec<RawVersion>>;

    /// Marks each unresolved version of `file` for which `resolve` returns `true`
    /// as resolved, so the sync service may delete it; returns how many were.
    /// Called inside a coordinated write of `file`.
    ///
    /// # Errors
    /// If the sync service refuses.
    fn mark_resolved(
        &self,
        file: &Path,
        resolve: &mut dyn FnMut(&[u8]) -> bool,
    ) -> io::Result<usize>;
}

/// No sync service conflicts (Linux, Windows, tests).
#[derive(Debug, Default, Clone, Copy)]
pub struct NoSyncConflicts;

impl SyncConflicts for NoSyncConflicts {
    fn versions(&self, _file: &Path) -> io::Result<Vec<RawVersion>> {
        Ok(Vec::new())
    }

    fn mark_resolved(
        &self,
        _file: &Path,
        _resolve: &mut dyn FnMut(&[u8]) -> bool,
    ) -> io::Result<usize> {
        Ok(0)
    }
}

/// A conflict version as the UI sees it. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictVersion {
    /// Its text.
    pub text: String,
    /// SHA-256 of its bytes.
    pub hash: String,
    /// When it was saved, milliseconds since the Unix epoch.
    pub modified_ms: u64,
    /// Name of the device that saved it, if known.
    pub device: Option<String>,
    /// For another file with the same `id` (format.md §4), e.g. a copy iCloud
    /// Drive made as `Name 2.md`: its vault-relative path. `None` for versions
    /// kept by the sync service.
    pub copy_path: Option<String>,
}

impl From<&RawVersion> for ConflictVersion {
    fn from(raw: &RawVersion) -> Self {
        Self {
            text: String::from_utf8_lossy(&raw.bytes).into_owned(),
            hash: sha256_hex(&raw.bytes),
            modified_ms: raw.modified_ms,
            device: raw.device.clone(),
            copy_path: None,
        }
    }
}

/// Another note file with the same `id`, as read.
struct Copy {
    path: String,
    bytes: Vec<u8>,
    modified_ms: u64,
}

impl From<&Copy> for ConflictVersion {
    fn from(copy: &Copy) -> Self {
        Self {
            text: String::from_utf8_lossy(&copy.bytes).into_owned(),
            hash: sha256_hex(&copy.bytes),
            modified_ms: copy.modified_ms,
            device: None,
            copy_path: Some(copy.path.clone()),
        }
    }
}

impl Vault {
    /// The unresolved conflicts of a note: versions kept by the sync service and
    /// other files in the same folder with the same `id`.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`], or if the sync service cannot be asked.
    pub fn conflicts(&self, path: &str) -> Result<Vec<ConflictVersion>> {
        let full = self.resolve(path)?;
        let text = fs::read(&full).map(|b| String::from_utf8_lossy(&b).into_owned())?;
        self.conflicts_at(path, &full, &text)
    }

    pub(crate) fn conflicts_at(
        &self,
        path: &str,
        full: &Path,
        text: &str,
    ) -> Result<Vec<ConflictVersion>> {
        let versions = self.sync().versions(full)?;
        let copies = self.copies_of(path, text)?;
        Ok(versions
            .iter()
            .map(ConflictVersion::from)
            .chain(copies.iter().map(ConflictVersion::from))
            .collect())
    }

    /// Other notes in the same folder whose front matter has the same `id` as
    /// `text` (format.md §4). Only the folder is searched: that is where iCloud
    /// Drive puts its copies; a vault-wide check comes with the index (M2).
    fn copies_of(&self, path: &str, text: &str) -> Result<Vec<Copy>> {
        let Some(id) = front_matter::get(text, "id") else {
            return Ok(Vec::new());
        };
        let folder = path.rsplit_once('/').map_or("", |(dir, _)| dir);
        let prefix = if folder.is_empty() {
            String::new()
        } else {
            format!("{folder}/")
        };
        let dir = if folder.is_empty() {
            self.root().to_owned()
        } else {
            self.root().join(folder)
        };
        let names = self.coordinated(Access::Read, &dir, |dir| {
            let mut names = Vec::new();
            for entry in fs::read_dir(dir)?.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let is_note = entry.file_type().is_ok_and(|t| t.is_file())
                    && !name.starts_with('.')
                    && name.to_ascii_lowercase().ends_with(".md");
                if is_note && format!("{prefix}{name}") != path {
                    names.push(name);
                }
            }
            Ok(names)
        })?;
        let mut copies = Vec::new();
        for name in names {
            let copy_path = format!("{prefix}{name}");
            let copy_full = self.resolve(&copy_path)?;
            let read = self.coordinated(Access::Read, &copy_full, |file| {
                Ok(fs::read(file)
                    .ok()
                    .map(|bytes| (bytes, modified_ms_of(file))))
            })?;
            if let Some((bytes, modified_ms)) = read
                && front_matter::get(&String::from_utf8_lossy(&bytes), "id").as_deref()
                    == Some(id.as_str())
            {
                copies.push(Copy {
                    path: copy_path,
                    bytes,
                    modified_ms,
                });
            }
        }
        copies.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(copies)
    }

    /// Keeps a note and its copy as two notes: the one created later gets a new
    /// `id` (format.md §4). With equal `created` values the file created later on
    /// this file system counts as later; that is the copy iCloud made.
    ///
    /// # Errors
    /// [`VaultError::Conflict`] if either file changed since it was read, or I/O errors.
    pub fn keep_both(&self, path: &str, copy_path: &str) -> Result<NoteContent> {
        let note = self.read_note(path)?;
        let copy = self.read_note(copy_path)?;
        let id = front_matter::get(&note.text, "id");
        if id.is_none() || id != front_matter::get(&copy.text, "id") {
            return Ok(note); // no longer the same note
        }
        let later_key = |n: &NoteContent| {
            (
                front_matter::get(&n.text, "created"),
                self.resolve(&n.path).map_or(0, |f| created_ms(&f)),
            )
        };
        let later = if later_key(&copy) >= later_key(&note) {
            &copy
        } else {
            &note
        };
        let renamed = front_matter::set(&later.text, "id", &new_ulid()?);
        self.write_note(&later.path, &renamed, &later.hash)?;
        self.read_note(path)
    }

    /// Settles a sync conflict: `text` becomes the note, every other version (the
    /// file as it is now, each conflict version and each copy with the same
    /// `id`) is stored as a snapshot in the note's history, then the stored
    /// versions are marked resolved and the copies removed.
    ///
    /// `expected_hash` is the hash of the note as last read, as for
    /// [`Vault::write_note`]. The note gets `id`, `created` and `modified` in its
    /// front matter (format.md §4), since the history is filed under its id.
    /// A conflict version that arrives meanwhile is not snapshotted and stays
    /// unresolved, so it is reported again.
    ///
    /// Returns the note as now on disk, with any versions still unresolved.
    ///
    /// # Errors
    /// [`VaultError::Conflict`] if the note changed since it was read,
    /// [`VaultError::InvalidId`] if its front matter `id` is not a ULID, or I/O
    /// errors. Nothing is marked resolved unless the note and all snapshots
    /// were written.
    pub fn resolve_conflict(
        &self,
        path: &str,
        text: &str,
        expected_hash: &str,
    ) -> Result<NoteContent> {
        let full = self.resolve(path)?;
        let current = self.coordinated(Access::Read, &full, |file| Ok(fs::read(file)?))?;
        let current_hash = sha256_hex(&current);
        if current_hash != expected_hash {
            return Err(VaultError::Conflict {
                path: path.to_owned(),
                current_hash: Some(current_hash),
            });
        }
        let versions = self.sync().versions(&full)?;
        let current_text = String::from_utf8_lossy(&current);
        let copies = self.copies_of(path, &current_text)?;

        let id = front_matter::get(text, "id")
            .or_else(|| front_matter::get(&current_text, "id"))
            .map_or_else(new_ulid, Ok)?;
        if !is_ulid(&id) {
            return Err(VaultError::InvalidId(id));
        }

        // Everything not kept goes to history before anything is resolved.
        let chosen_hash = sha256_hex(text.as_bytes());
        let mut stored = BTreeSet::from([chosen_hash.clone()]);
        let losing = std::iter::once(current.as_slice())
            .chain(versions.iter().map(|v| v.bytes.as_slice()))
            .chain(copies.iter().map(|c| c.bytes.as_slice()));
        for bytes in losing {
            if stored.insert(sha256_hex(bytes)) {
                self.snapshot(&id, bytes)?;
            }
        }

        let created = front_matter::get(&current_text, "created")
            .unwrap_or_else(|| rfc3339_utc(created_ms(&full)));
        let stamped = front_matter::stamp(text, &id, &created, &rfc3339_utc(now_ms()));
        self.write_note(path, &stamped, expected_hash)?;

        self.coordinated(Access::Write, &full, |file| {
            Ok(self
                .sync()
                .mark_resolved(file, &mut |bytes| stored.contains(&sha256_hex(bytes)))?)
        })?;
        // Copies are in the history now (or their text is the note); remove
        // each one unless it changed since it was read, then it is reported again.
        for copy in &copies {
            let copy_full = self.resolve(&copy.path)?;
            self.coordinated(Access::Delete, &copy_full, |file| {
                if fs::read(file).is_ok_and(|now| now == copy.bytes) {
                    fs::remove_file(file)?;
                }
                Ok(())
            })?;
        }
        self.read_note(path)
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::{Arc, Mutex};

    use super::*;

    /// A sync service with conflict versions held in memory.
    #[derive(Debug, Default)]
    struct FakeICloud {
        unresolved: Mutex<Vec<RawVersion>>,
        /// Arrives between listing and resolving, like a third device.
        late: Mutex<Option<RawVersion>>,
        resolved_in: Mutex<Vec<PathBuf>>,
    }

    impl FakeICloud {
        fn with(texts: &[&str]) -> Self {
            Self {
                unresolved: Mutex::new(texts.iter().map(|t| version(t)).collect()),
                ..Self::default()
            }
        }
    }

    fn version(text: &str) -> RawVersion {
        RawVersion {
            bytes: text.as_bytes().to_vec(),
            modified_ms: 1_792_099_714_000,
            device: Some("wklPhone".into()),
        }
    }

    impl SyncConflicts for FakeICloud {
        fn versions(&self, _file: &Path) -> io::Result<Vec<RawVersion>> {
            Ok(self.unresolved.lock().unwrap().clone())
        }

        fn mark_resolved(
            &self,
            file: &Path,
            resolve: &mut dyn FnMut(&[u8]) -> bool,
        ) -> io::Result<usize> {
            self.resolved_in.lock().unwrap().push(file.to_owned());
            let mut unresolved = self.unresolved.lock().unwrap();
            unresolved.extend(self.late.lock().unwrap().take());
            let before = unresolved.len();
            unresolved.retain(|v| !resolve(&v.bytes));
            Ok(before - unresolved.len())
        }
    }

    fn vault(sync: FakeICloud) -> (tempfile::TempDir, Vault, Arc<FakeICloud>) {
        let dir = tempfile::tempdir().unwrap();
        let sync = Arc::new(sync);
        let vault = Vault::open(dir.path())
            .unwrap()
            .with_sync_conflicts(sync.clone());
        (dir, vault, sync)
    }

    fn history_texts(vault: &Vault, id: &str) -> Vec<String> {
        let mut texts: Vec<_> = vault
            .snapshots(id)
            .unwrap()
            .iter()
            .map(|n| vault.read_snapshot(id, n).unwrap())
            .collect();
        texts.sort();
        texts
    }

    #[test]
    fn read_note_reports_conflict_versions() {
        let (dir, vault, _) = vault(FakeICloud::with(&["from the phone"]));
        fs::write(dir.path().join("n.md"), "from the mac").unwrap();
        let note = vault.read_note("n.md").unwrap();
        assert_eq!(note.conflicts.len(), 1);
        assert_eq!(note.conflicts[0].text, "from the phone");
        assert_eq!(note.conflicts[0].device.as_deref(), Some("wklPhone"));
        assert_eq!(note.conflicts[0].hash, sha256_hex(b"from the phone"));
    }

    #[test]
    fn keeping_mine_stores_theirs_in_history() {
        let (dir, vault, sync) = vault(FakeICloud::with(&["from the phone"]));
        fs::write(dir.path().join("n.md"), "# Note\nfrom the mac\n").unwrap();
        let note = vault.read_note("n.md").unwrap();

        let after = vault
            .resolve_conflict("n.md", &note.text, &note.hash)
            .unwrap();
        assert!(after.conflicts.is_empty());
        assert!(
            after.text.ends_with("---\n# Note\nfrom the mac\n"),
            "{}",
            after.text
        );
        let id = front_matter::get(&after.text, "id").unwrap();
        assert!(is_ulid(&id));
        assert!(front_matter::get(&after.text, "created").is_some());
        assert!(front_matter::get(&after.text, "modified").is_some());
        assert_eq!(history_texts(&vault, &id), ["from the phone"]);
        assert_eq!(*sync.resolved_in.lock().unwrap(), [dir.path().join("n.md")]);
    }

    #[test]
    fn using_theirs_stores_mine_in_history_and_keeps_the_id() {
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let mine = format!("---\nid: {id}\ncreated: 2026-01-01T00:00:00Z\n---\nfrom the mac\n");
        let (dir, vault, _) = vault(FakeICloud::with(&["from the phone\n"]));
        fs::write(dir.path().join("n.md"), &mine).unwrap();
        let note = vault.read_note("n.md").unwrap();

        let after = vault
            .resolve_conflict("n.md", &note.conflicts[0].text, &note.hash)
            .unwrap();
        assert_eq!(front_matter::get(&after.text, "id").as_deref(), Some(id));
        assert_eq!(
            front_matter::get(&after.text, "created").as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert!(after.text.ends_with("---\nfrom the phone\n"));
        assert_eq!(history_texts(&vault, id), [mine]);
        assert!(after.conflicts.is_empty());
    }

    #[test]
    fn a_merge_keeps_both_originals() {
        let (dir, vault, _) = vault(FakeICloud::with(&["phone"]));
        fs::write(dir.path().join("n.md"), "mac").unwrap();
        let note = vault.read_note("n.md").unwrap();
        let after = vault
            .resolve_conflict("n.md", "mac\nphone\n", &note.hash)
            .unwrap();
        let id = front_matter::get(&after.text, "id").unwrap();
        assert_eq!(history_texts(&vault, &id), ["mac", "phone"]);
    }

    #[test]
    fn a_version_arriving_meanwhile_stays_unresolved() {
        let sync = FakeICloud::with(&["phone"]);
        *sync.late.lock().unwrap() = Some(version("ipad"));
        let (dir, vault, _) = vault(sync);
        fs::write(dir.path().join("n.md"), "mac").unwrap();
        let note = vault.read_note("n.md").unwrap();
        let after = vault.resolve_conflict("n.md", "mac", &note.hash).unwrap();
        assert_eq!(after.conflicts.len(), 1);
        assert_eq!(after.conflicts[0].text, "ipad");
    }

    #[test]
    fn nothing_is_resolved_when_the_note_changed() {
        let (dir, vault, sync) = vault(FakeICloud::with(&["phone"]));
        fs::write(dir.path().join("n.md"), "mac").unwrap();
        let note = vault.read_note("n.md").unwrap();
        fs::write(dir.path().join("n.md"), "mac, edited elsewhere").unwrap();
        assert!(matches!(
            vault.resolve_conflict("n.md", "mac", &note.hash),
            Err(VaultError::Conflict { .. })
        ));
        assert_eq!(sync.unresolved.lock().unwrap().len(), 1);
        assert!(!dir.path().join(crate::history::HISTORY_DIR).exists());
    }

    #[test]
    fn an_invalid_id_stops_before_anything_is_written() {
        let (dir, vault, sync) = vault(FakeICloud::with(&["phone"]));
        let mine = "---\nid: ../../etc\n---\nmac";
        fs::write(dir.path().join("n.md"), mine).unwrap();
        let note = vault.read_note("n.md").unwrap();
        assert!(matches!(
            vault.resolve_conflict("n.md", mine, &note.hash),
            Err(VaultError::InvalidId(_))
        ));
        assert_eq!(fs::read_to_string(dir.path().join("n.md")).unwrap(), mine);
        assert_eq!(sync.unresolved.lock().unwrap().len(), 1);
    }

    /// A sync service that cannot be asked.
    #[derive(Debug)]
    struct Broken;

    impl SyncConflicts for Broken {
        fn versions(&self, _file: &Path) -> io::Result<Vec<RawVersion>> {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "no access to versions",
            ))
        }
        fn mark_resolved(&self, _: &Path, _: &mut dyn FnMut(&[u8]) -> bool) -> io::Result<usize> {
            Ok(0)
        }
    }

    #[test]
    fn a_failing_sync_service_is_reported_not_hidden() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path())
            .unwrap()
            .with_sync_conflicts(Arc::new(Broken));
        fs::write(dir.path().join("n.md"), "text").unwrap();
        let note = vault.read_note("n.md").unwrap();
        assert_eq!(note.text, "text");
        assert!(note.conflicts.is_empty());
        assert_eq!(
            note.conflicts_error.as_deref(),
            Some("no access to versions")
        );
        let json = serde_json::to_string(&note).unwrap();
        assert!(
            json.contains("\"conflictsError\":\"no access to versions\""),
            "{json}"
        );
    }

    const SAME: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn note_with(id: &str, body: &str) -> String {
        format!("---\nid: {id}\ncreated: 2026-10-16T07:00:00Z\n---\n{body}")
    }

    /// A vault with `Einkaufsliste.md` and its iCloud copy `Einkaufsliste 2.md`.
    fn vault_with_copy() -> (tempfile::TempDir, Vault, String, String) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let mine = note_with(SAME, "mac\n");
        let copy = note_with(SAME, "phone\n");
        fs::write(dir.path().join("Einkaufsliste.md"), &mine).unwrap();
        fs::write(dir.path().join("Einkaufsliste 2.md"), &copy).unwrap();
        fs::write(
            dir.path().join("Other.md"),
            note_with("01BX5ZZKBKACTAV9WEVGEMMVRZ", "x"),
        )
        .unwrap();
        (dir, vault, mine, copy)
    }

    #[test]
    fn read_note_reports_a_copy_with_the_same_id() {
        let (_dir, vault, _, copy) = vault_with_copy();
        let note = vault.read_note("Einkaufsliste.md").unwrap();
        assert_eq!(note.conflicts.len(), 1);
        assert_eq!(
            note.conflicts[0].copy_path.as_deref(),
            Some("Einkaufsliste 2.md")
        );
        assert_eq!(note.conflicts[0].text, copy);
        // And the other way round.
        let other = vault.read_note("Einkaufsliste 2.md").unwrap();
        assert_eq!(
            other.conflicts[0].copy_path.as_deref(),
            Some("Einkaufsliste.md")
        );
        assert!(vault.read_note("Other.md").unwrap().conflicts.is_empty());
    }

    #[test]
    fn copies_are_found_in_subfolders_and_notes_without_id_have_none() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/a.md"), note_with(SAME, "a")).unwrap();
        fs::write(dir.path().join("sub/a 2.md"), note_with(SAME, "b")).unwrap();
        fs::write(dir.path().join("plain.md"), "no id").unwrap();
        fs::write(dir.path().join("plain 2.md"), "no id").unwrap();
        let note = vault.read_note("sub/a.md").unwrap();
        assert_eq!(note.conflicts[0].copy_path.as_deref(), Some("sub/a 2.md"));
        assert!(vault.read_note("plain.md").unwrap().conflicts.is_empty());
    }

    #[test]
    fn keeping_mine_stores_the_copy_and_removes_it() {
        let (dir, vault, _, copy) = vault_with_copy();
        let note = vault.read_note("Einkaufsliste.md").unwrap();
        let after = vault
            .resolve_conflict("Einkaufsliste.md", &note.text, &note.hash)
            .unwrap();
        assert!(after.conflicts.is_empty());
        assert_eq!(front_matter::body(&after.text), "mac\n");
        assert!(!dir.path().join("Einkaufsliste 2.md").exists());
        assert_eq!(history_texts(&vault, SAME), [copy]);
    }

    #[test]
    fn using_the_copy_stores_mine_and_removes_the_copy() {
        let (dir, vault, mine, copy) = vault_with_copy();
        let note = vault.read_note("Einkaufsliste.md").unwrap();
        let after = vault
            .resolve_conflict("Einkaufsliste.md", &copy, &note.hash)
            .unwrap();
        assert_eq!(front_matter::body(&after.text), "phone\n");
        assert_eq!(front_matter::get(&after.text, "id").as_deref(), Some(SAME));
        assert!(!dir.path().join("Einkaufsliste 2.md").exists());
        assert_eq!(history_texts(&vault, SAME), [mine]);
    }

    /// Deletes like `Uncoordinated`, but another device saves the file first.
    #[derive(Debug)]
    struct EditsBeforeDelete;

    impl crate::Coordinator for EditsBeforeDelete {
        fn read(&self, p: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
            f(p)
        }
        fn write(&self, p: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
            f(p)
        }
        fn delete(&self, p: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
            fs::write(p, note_with(SAME, "edited on the iPad\n"))?;
            f(p)
        }
    }

    #[test]
    fn a_copy_changed_meanwhile_is_kept_and_reported_again() {
        let (dir, _, _, _) = vault_with_copy();
        let vault = Vault::open_with(dir.path(), Arc::new(EditsBeforeDelete)).unwrap();
        let note = vault.read_note("Einkaufsliste.md").unwrap();
        let after = vault
            .resolve_conflict("Einkaufsliste.md", &note.text, &note.hash)
            .unwrap();
        assert!(dir.path().join("Einkaufsliste 2.md").exists());
        assert_eq!(after.conflicts.len(), 1);
        assert!(after.conflicts[0].text.contains("edited on the iPad"));
    }

    #[test]
    fn keeping_both_gives_the_copy_a_new_id() {
        let (dir, vault, _, _) = vault_with_copy();
        let after = vault
            .keep_both("Einkaufsliste.md", "Einkaufsliste 2.md")
            .unwrap();
        assert!(after.conflicts.is_empty());
        assert_eq!(front_matter::get(&after.text, "id").as_deref(), Some(SAME));
        let copy = fs::read_to_string(dir.path().join("Einkaufsliste 2.md")).unwrap();
        let new_id = front_matter::get(&copy, "id").unwrap();
        assert_ne!(new_id, SAME);
        assert!(is_ulid(&new_id));
        assert_eq!(front_matter::body(&copy), "phone\n");
        assert!(
            vault
                .read_note("Einkaufsliste 2.md")
                .unwrap()
                .conflicts
                .is_empty()
        );
    }

    #[test]
    fn keeping_both_renames_the_later_created_note() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        let older = format!("---\nid: {SAME}\ncreated: 2026-01-01T00:00:00Z\n---\nolder");
        let newer = format!("---\nid: {SAME}\ncreated: 2026-05-01T00:00:00Z\n---\nnewer");
        fs::write(dir.path().join("b.md"), &older).unwrap();
        fs::write(dir.path().join("a.md"), &newer).unwrap();
        // Asked from the older note's side: still the newer one gets the new id.
        vault.keep_both("b.md", "a.md").unwrap();
        let a = fs::read_to_string(dir.path().join("a.md")).unwrap();
        let b = fs::read_to_string(dir.path().join("b.md")).unwrap();
        assert_eq!(front_matter::get(&b, "id").as_deref(), Some(SAME));
        assert_ne!(front_matter::get(&a, "id").as_deref(), Some(SAME));
        // From the newer note's side as well.
        fs::write(dir.path().join("a.md"), &newer).unwrap();
        vault.keep_both("a.md", "b.md").unwrap();
        let a = fs::read_to_string(dir.path().join("a.md")).unwrap();
        assert_ne!(front_matter::get(&a, "id").as_deref(), Some(SAME));
        assert_eq!(
            front_matter::get(&fs::read_to_string(dir.path().join("b.md")).unwrap(), "id")
                .as_deref(),
            Some(SAME)
        );
    }

    #[test]
    fn conflict_version_serializes_camel_case() {
        let json = serde_json::to_string(&ConflictVersion::from(&version("x"))).unwrap();
        assert!(json.contains("\"modifiedMs\":1792099714000"), "{json}");
        assert!(json.contains("\"device\":\"wklPhone\""), "{json}");
    }
}
