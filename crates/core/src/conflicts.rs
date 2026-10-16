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
use crate::vault::{Access, NoteContent, Result, Vault, VaultError, sha256_hex};

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
}

impl From<&RawVersion> for ConflictVersion {
    fn from(raw: &RawVersion) -> Self {
        Self {
            text: String::from_utf8_lossy(&raw.bytes).into_owned(),
            hash: sha256_hex(&raw.bytes),
            modified_ms: raw.modified_ms,
            device: raw.device.clone(),
        }
    }
}

impl Vault {
    /// The unresolved conflict versions of a note.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`], or if the sync service cannot be asked.
    pub fn conflicts(&self, path: &str) -> Result<Vec<ConflictVersion>> {
        self.conflicts_at(&self.resolve(path)?)
    }

    pub(crate) fn conflicts_at(&self, full: &Path) -> Result<Vec<ConflictVersion>> {
        Ok(self
            .sync()
            .versions(full)?
            .iter()
            .map(ConflictVersion::from)
            .collect())
    }

    /// Settles a sync conflict: `text` becomes the note, every other version (the
    /// file as it is now and each conflict version) is stored as a snapshot in
    /// the note's history, then the stored versions are marked resolved.
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

        let id = front_matter::get(text, "id")
            .or_else(|| front_matter::get(&current_text, "id"))
            .map_or_else(new_ulid, Ok)?;
        if !is_ulid(&id) {
            return Err(VaultError::InvalidId(id));
        }

        // Everything not kept goes to history before anything is resolved.
        let chosen_hash = sha256_hex(text.as_bytes());
        let mut stored = BTreeSet::from([chosen_hash.clone()]);
        let losing =
            std::iter::once(current.as_slice()).chain(versions.iter().map(|v| v.bytes.as_slice()));
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
        self.read_note(path)
    }
}

/// Creation time of a file for `created`: its birth time where the file system
/// records one, else its modification time, else now.
fn created_ms(file: &Path) -> u64 {
    fs::metadata(file)
        .and_then(|m| m.created().or_else(|_| m.modified()))
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or_else(now_ms, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
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

    #[test]
    fn conflict_version_serializes_camel_case() {
        let json = serde_json::to_string(&ConflictVersion::from(&version("x"))).unwrap();
        assert!(json.contains("\"modifiedMs\":1792099714000"), "{json}");
        assert!(json.contains("\"device\":\"wklPhone\""), "{json}");
    }
}
