// SPDX-License-Identifier: Apache-2.0
//! Version history snapshots (format.md §7, ADR-0011).
//!
//! A snapshot is the complete note file, zstd-compressed, stored as
//! `.notesapp/history/<note-id>/<YYYYMMDDTHHMMSSZ>[-n].md.zst`. Snapshots are
//! written like notes (atomic, coordinated) and never replace one another.
//! [`Vault::snapshot_note`] skips a version the history already ends with.
//!
//! Retention (ADR-0011): [`Vault::thin_history`] keeps every snapshot for 7
//! days, then the last of each day until 90 days, then the last of each ISO
//! week. Ages come from the UTC stamps in the names; a name it cannot read
//! is kept. The two periods can be changed in `.notesapp/settings.yaml`
//! (format.md §7, ADR-0017), see [`Vault::retention`].
//!
//! The history view works with note paths: [`Vault::versions`] lists the
//! snapshots of a note, [`Vault::restore_version`] makes one the note's text
//! again (storing the current text first), [`Vault::copy_version`] turns one
//! into a new note with its own `id`.

use std::collections::HashMap;
use std::fs;
use std::io::{self, Read as _};

use ruzstd::decoding::StreamingDecoder;
use ruzstd::encoding::{CompressionLevel, compress_to_vec};

use serde::Serialize;

use crate::front_matter;
use crate::names::NewNote;
use crate::stamp::{compact_utc, is_ulid, new_ulid, now_ms, parse_compact_utc, rfc3339_utc};
use crate::vault::{
    Access, Result, SavedNote, TempFile, Vault, VaultError, accessor_ran_twice, sha256_hex,
};

/// A snapshot of a note, as listed by [`Vault::versions`]. Serialized with
/// camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Version {
    /// The snapshot's file name; pass it to the other version methods.
    pub name: String,
    /// When it was taken, from its name; `None` for a name the app did not write.
    pub taken_ms: Option<u64>,
}

/// How long snapshots are kept (ADR-0011, format.md §7): every one for
/// `all_days`, then the last of each UTC day until `daily_days`, then the
/// last of each ISO week.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Retention {
    /// Days during which every snapshot is kept.
    pub all_days: u64,
    /// Days until which the last snapshot of each day is kept.
    pub daily_days: u64,
}

impl Default for Retention {
    fn default() -> Self {
        Self {
            all_days: 7,
            daily_days: 90,
        }
    }
}

impl Retention {
    /// The retention set in a `settings.yaml` (format.md §7):
    /// `history_keep_all_days` and `history_keep_daily_days`, whole numbers of
    /// days. A missing or invalid value keeps its default; a daily period
    /// shorter than the first is extended to it.
    #[must_use]
    pub fn from_settings(settings: &str) -> Self {
        let defaults = Self::default();
        let days = |key: &str, default: u64| {
            crate::flat_yaml::value(settings, key)
                .and_then(|v| v.parse::<u64>().ok())
                .unwrap_or(default)
        };
        let all_days = days("history_keep_all_days", defaults.all_days);
        let daily_days = days("history_keep_daily_days", defaults.daily_days).max(all_days);
        Self {
            all_days,
            daily_days,
        }
    }
}

/// Folder of all snapshots, relative to the vault root.
pub const HISTORY_DIR: &str = ".notesapp/history";

impl Vault {
    /// Stores `content` (the complete note file) as a new snapshot of the note
    /// with `id`; returns the snapshot's path relative to the vault root.
    ///
    /// # Errors
    /// [`VaultError::InvalidId`] if `id` is not a ULID, or I/O errors.
    pub fn snapshot(&self, id: &str, content: impl AsRef<[u8]>) -> Result<String> {
        self.check_writable()?;
        let id = checked_id(id)?;
        let dir = self.root().join(HISTORY_DIR).join(&id);
        fs::create_dir_all(&dir)?;
        // ruzstd only implements its fastest level; the others panic.
        let compressed = compress_to_vec(content.as_ref(), CompressionLevel::Fastest);
        let stamp = compact_utc(now_ms());
        for n in 0..1000 {
            let name = if n == 0 {
                format!("{stamp}.md.zst")
            } else {
                format!("{stamp}-{n}.md.zst")
            };
            let target = dir.join(&name);
            let mut tmp = Some(TempFile::write(&target, &compressed)?);
            let placed = self.coordinated(Access::Write, &target, |file| {
                let tmp = tmp.take().ok_or_else(accessor_ran_twice)?;
                Ok(tmp.place_new(file)?)
            });
            match placed {
                Ok(()) => return Ok(format!("{HISTORY_DIR}/{id}/{name}")),
                Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("too many snapshots in the same second").into())
    }

    /// Stores the note at `path` (relative to `notes/`) as a snapshot under
    /// its `id`: `text` if given (e.g. unsaved text about to be replaced),
    /// else the file on disk. Nothing is stored if the history already ends
    /// with this version, or if neither `text` nor the file has a valid `id`.
    /// Returns the new snapshot's path relative to the vault root.
    ///
    /// # Errors
    /// [`VaultError::ReadOnly`], [`VaultError::InvalidPath`], or I/O errors.
    pub fn snapshot_note(&self, path: &str, text: Option<&str>) -> Result<Option<String>> {
        self.check_writable()?;
        let full = self.resolve(path)?;
        let on_disk = || self.coordinated(Access::Read, &full, |file| Ok(fs::read(file)?));
        let id_in = |bytes: &[u8]| {
            front_matter::get(&String::from_utf8_lossy(bytes), "id").filter(|id| is_ulid(id))
        };
        let (content, id) = if let Some(text) = text {
            let id = id_in(text.as_bytes()).or_else(|| on_disk().ok().and_then(|b| id_in(&b)));
            (text.as_bytes().to_vec(), id)
        } else {
            let bytes = on_disk()?;
            let id = id_in(&bytes);
            (bytes, id)
        };
        match id {
            Some(id) => self.snapshot_if_new(&id, &content),
            None => Ok(None),
        }
    }

    /// Stores `content` as a snapshot of the note with `id` unless the
    /// history already ends with it; returns the new snapshot's path.
    ///
    /// # Errors
    /// As for [`Vault::snapshot`].
    pub fn snapshot_if_new(&self, id: &str, content: &[u8]) -> Result<Option<String>> {
        if let Some(last) = self.snapshots(id)?.last()
            && let Ok(text) = self.read_snapshot(id, last)
            && text.as_bytes() == content
        {
            return Ok(None);
        }
        self.snapshot(id, content).map(Some)
    }

    /// The retention set in `.notesapp/settings.yaml`, or the default if the
    /// file is missing or cannot be read.
    #[must_use]
    pub fn retention(&self) -> Retention {
        let file = self.root().join(crate::layout::SETTINGS_FILE);
        let text = self.coordinated(Access::Read, &file, |file| match fs::read(file) {
            Ok(bytes) => Ok(String::from_utf8_lossy(&bytes).into_owned()),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(String::new()),
            Err(e) => Err(e.into()),
        });
        text.map_or_else(
            |e| {
                eprintln!("settings not readable, default retention: {e}");
                Retention::default()
            },
            |text| Retention::from_settings(&text),
        )
    }

    /// Removes the snapshots `retention` no longer keeps, in every note's
    /// history, as of `now_ms`. Returns how many were removed.
    ///
    /// # Errors
    /// [`VaultError::ReadOnly`], or I/O errors reading the history folder;
    /// a snapshot that cannot be removed is skipped.
    pub fn thin_history(&self, retention: Retention, now_ms: u64) -> Result<usize> {
        self.check_writable()?;
        let root = self.root().join(HISTORY_DIR);
        let entries = match fs::read_dir(&root) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(0),
            Err(e) => return Err(e.into()),
        };
        let mut removed = 0;
        for entry in entries.flatten() {
            let id = entry.file_name().to_string_lossy().into_owned();
            if !is_ulid(&id) || !entry.file_type().is_ok_and(|t| t.is_dir()) {
                continue;
            }
            let names = self.snapshots(&id)?;
            for name in to_thin(&names, retention, now_ms / 1000) {
                let file = entry.path().join(name);
                let gone =
                    self.coordinated(Access::Delete, &file, |file| match fs::remove_file(file) {
                        Ok(()) => Ok(true),
                        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(false), // thinned elsewhere
                        Err(e) => Err(e.into()),
                    });
                match gone {
                    Ok(true) => removed += 1,
                    Ok(false) => {}
                    Err(e) => eprintln!("snapshot {} not removed: {e}", file.display()),
                }
            }
        }
        Ok(removed)
    }

    /// The `id` of the note at `path` as on disk, if it has a valid one.
    fn id_on_disk(&self, path: &str) -> Result<Option<String>> {
        let full = self.resolve(path)?;
        let bytes = self.coordinated(Access::Read, &full, |file| Ok(fs::read(file)?))?;
        Ok(front_matter::get(&String::from_utf8_lossy(&bytes), "id").filter(|id| is_ulid(id)))
    }

    /// The `id` of the note at `path`, for its history.
    fn history_id(&self, path: &str) -> Result<String> {
        self.id_on_disk(path)?
            .ok_or_else(|| VaultError::InvalidId(format!("{path} has no id")))
    }

    /// The snapshots of the note at `path` (relative to `notes/`), newest
    /// first; none for a note without `id`.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`], or I/O errors.
    pub fn versions(&self, path: &str) -> Result<Vec<Version>> {
        let Some(id) = self.id_on_disk(path)? else {
            return Ok(Vec::new());
        };
        let mut names = self.snapshots(&id)?;
        names.reverse();
        Ok(names
            .into_iter()
            .map(|name| Version {
                taken_ms: parse_compact_utc(sort_key(&name).0).map(|s| s * 1000),
                name,
            })
            .collect())
    }

    /// The text of the snapshot `name` of the note at `path`.
    ///
    /// # Errors
    /// [`VaultError::InvalidId`] for a note without `id`,
    /// [`VaultError::InvalidPath`], or I/O errors.
    pub fn read_version(&self, path: &str, name: &str) -> Result<String> {
        self.read_snapshot(&self.history_id(path)?, name)
    }

    /// Makes the snapshot `name` the text of the note at `path` again. The
    /// current text is stored as a snapshot first, so restoring loses nothing;
    /// `expected_hash` guards against a change made elsewhere meanwhile, as
    /// for [`Vault::write_note`]. Returns what is now on disk.
    ///
    /// # Errors
    /// [`VaultError::Conflict`] if the note changed since it was read (nothing
    /// is written then), [`VaultError::ReadOnly`], or as for [`Vault::read_version`].
    pub fn restore_version(
        &self,
        path: &str,
        name: &str,
        expected_hash: &str,
    ) -> Result<SavedNote> {
        self.check_writable()?;
        let id = self.history_id(path)?;
        let text = self.read_snapshot(&id, name)?;
        let full = self.resolve(path)?;
        let current = self.coordinated(Access::Read, &full, |file| Ok(fs::read(file)?))?;
        let current_hash = sha256_hex(&current);
        if current_hash != expected_hash {
            return Err(VaultError::Conflict {
                path: path.to_owned(),
                current_hash: Some(current_hash),
            });
        }
        self.snapshot_if_new(&id, &current)?;
        self.write_note(path, &text, expected_hash)
    }

    /// Creates a new note from the snapshot `name` of the note at `path`, in
    /// the same folder and named after its title, with a new `id` and
    /// `created` (format.md §4). The note itself is not changed.
    ///
    /// # Errors
    /// [`VaultError::ReadOnly`], or as for [`Vault::read_version`].
    pub fn copy_version(&self, path: &str, name: &str) -> Result<NewNote> {
        self.check_writable()?;
        let text = self.read_version(path, name)?;
        let text = front_matter::set(&text, "id", &new_ulid()?);
        let text = front_matter::set(&text, "created", &rfc3339_utc(now_ms()));
        let folder = path.rsplit_once('/').map_or("", |(folder, _)| folder);
        self.create_titled(folder, &text)
    }

    /// The names of a note's snapshots, oldest first.
    ///
    /// # Errors
    /// [`VaultError::InvalidId`] or I/O errors; a note without history has none.
    pub fn snapshots(&self, id: &str) -> Result<Vec<String>> {
        let id = checked_id(id)?;
        let dir = self.root().join(HISTORY_DIR).join(id);
        let entries = match fs::read_dir(dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut names: Vec<_> = entries
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| !n.starts_with('.') && n.ends_with(".md.zst"))
            .collect();
        // `-n` suffixes sort after the plain name of the same second.
        names.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        Ok(names)
    }

    /// Reads one snapshot of the note with `id`.
    ///
    /// # Errors
    /// [`VaultError::InvalidId`], [`VaultError::InvalidPath`] for a bad name,
    /// or I/O errors (including a corrupt snapshot).
    pub fn read_snapshot(&self, id: &str, name: &str) -> Result<String> {
        let id = checked_id(id)?;
        if name.starts_with('.') || name.contains(['/', '\\']) || !name.ends_with(".md.zst") {
            return Err(VaultError::InvalidPath(name.to_owned()));
        }
        let path = self.root().join(HISTORY_DIR).join(id).join(name);
        self.coordinated(Access::Read, &path, |file| {
            let bytes = fs::read(file)?;
            let mut decoder = StreamingDecoder::new(bytes.as_slice()).map_err(io::Error::other)?;
            let mut text = String::new();
            decoder.read_to_string(&mut text)?;
            Ok(text)
        })
    }
}

/// Validates an id from a note file and normalizes it to upper case, so it is
/// safe as a folder name and the same on case-sensitive file systems.
fn checked_id(id: &str) -> Result<String> {
    if is_ulid(id) {
        Ok(id.to_ascii_uppercase())
    } else {
        Err(VaultError::InvalidId(id.to_owned()))
    }
}

/// The snapshots in `names` (oldest first) that `retention` no longer keeps
/// at `now` (Unix seconds).
fn to_thin(names: &[String], retention: Retention, now: u64) -> Vec<&str> {
    const DAY: u64 = 86_400;
    // Per bucket (tier and day or ISO week), the last snapshot is kept.
    let mut last: HashMap<(bool, u64), &str> = HashMap::new();
    let mut candidates = Vec::new();
    for name in names {
        let Some(secs) = parse_compact_utc(sort_key(name).0) else {
            continue; // not a name we wrote: keep it
        };
        let age_days = now.saturating_sub(secs) / DAY;
        if age_days < retention.all_days {
            continue;
        }
        let day = secs / DAY;
        // ISO weeks start on Monday; 1970-01-01 was a Thursday.
        let bucket = if age_days < retention.daily_days {
            (false, day)
        } else {
            (true, (day + 3) / 7)
        };
        last.insert(bucket, name);
        candidates.push((bucket, name.as_str()));
    }
    candidates
        .into_iter()
        .filter(|(bucket, name)| last.get(bucket) != Some(name))
        .map(|(_, name)| name)
        .collect()
}

/// `20261015T212937Z-2.md.zst` → (`20261015T212937Z`, 2)
fn sort_key(name: &str) -> (&str, u32) {
    let stem = name.trim_end_matches(".md.zst");
    stem.split_once('-').map_or((stem, 0), |(stamp, n)| {
        (stamp, n.parse().unwrap_or(u32::MAX))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn snapshots_roundtrip_and_never_collide() {
        let (dir, vault) = vault();
        let text = "---\nid: x\n---\n# Note\n\nSome text äöü\n".repeat(20);
        let first = vault.snapshot(ID, &text).unwrap();
        let second = vault.snapshot(&ID.to_lowercase(), "second").unwrap();
        assert!(
            first.starts_with(&format!("{HISTORY_DIR}/{ID}/")),
            "{first}"
        );
        assert_ne!(first, second);

        let names = vault.snapshots(ID).unwrap();
        assert_eq!(names.len(), 2);
        assert_eq!(vault.read_snapshot(ID, &names[0]).unwrap(), text);
        assert_eq!(vault.read_snapshot(ID, &names[1]).unwrap(), "second");
        // Compressed, and not visible as notes.
        let size = fs::metadata(dir.path().join(&first)).unwrap().len();
        assert!(size < u64::try_from(text.len()).unwrap() / 2, "{size}");
        assert!(vault.list_notes().unwrap().is_empty());
    }

    #[test]
    fn ids_and_names_are_checked() {
        let (dir, vault) = vault();
        for bad in ["../../x", "", "not-a-ulid"] {
            assert!(matches!(
                vault.snapshot(bad, "t"),
                Err(VaultError::InvalidId(_))
            ));
        }
        assert!(!dir.path().join(HISTORY_DIR).exists());
        for bad in ["../x.md.zst", ".hidden.md.zst", "x.md"] {
            assert!(matches!(
                vault.read_snapshot(ID, bad),
                Err(VaultError::InvalidPath(_))
            ));
        }
        assert!(vault.snapshots(ID).unwrap().is_empty());
    }

    #[test]
    fn suffixes_sort_after_their_second() {
        let mut names = vec![
            "20261015T212937Z-10.md.zst",
            "20261015T212938Z.md.zst",
            "20261015T212937Z-2.md.zst",
            "20261015T212937Z.md.zst",
        ];
        names.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));
        assert_eq!(
            names,
            [
                "20261015T212937Z.md.zst",
                "20261015T212937Z-2.md.zst",
                "20261015T212937Z-10.md.zst",
                "20261015T212938Z.md.zst",
            ]
        );
    }

    #[test]
    fn snapshot_note_skips_what_the_history_ends_with() {
        let (_dir, vault) = vault();
        let one = format!("---\nid: {ID}\n---\none\n");
        vault.create_note("n.md", "plain").unwrap();
        let on_disk = vault.read_note("n.md").unwrap();
        let id = front_matter::get(&on_disk.text, "id").unwrap();

        assert!(vault.snapshot_note("n.md", None).unwrap().is_some());
        assert_eq!(vault.snapshot_note("n.md", None).unwrap(), None); // unchanged
        assert_eq!(vault.snapshots(&id).unwrap().len(), 1);

        // Unsaved text without front matter is filed under the id on disk.
        assert!(
            vault
                .snapshot_note("n.md", Some("unsaved"))
                .unwrap()
                .is_some()
        );
        let names = vault.snapshots(&id).unwrap();
        assert_eq!(
            vault.read_snapshot(&id, names.last().unwrap()).unwrap(),
            "unsaved"
        );

        // Text with its own id goes there.
        assert!(vault.snapshot_note("n.md", Some(&one)).unwrap().is_some());
        assert_eq!(vault.snapshots(ID).unwrap().len(), 1);

        // No id anywhere: nothing to file it under.
        std::fs::write(vault.notes_dir().join("bare.md"), "no id").unwrap();
        assert_eq!(vault.snapshot_note("bare.md", None).unwrap(), None);
    }

    fn day(secs: u64) -> String {
        format!("{}.md.zst", compact_utc(secs * 1000))
    }

    #[test]
    fn retention_keeps_all_then_daily_then_weekly() {
        const D: u64 = 86_400;
        let now = 1_792_099_777; // Thursday 2026-10-15 21:29:37 UTC
        let names: Vec<String> = [
            // older than 90 days, same ISO week (Mon 2026-06-01 … Sun 06-07)
            now - 136 * D, // Mon
            now - 133 * D, // Thu
            now - 130 * D, // Sun: the last of that week
            // 10 days old, three on the same day: the last one stays
            now - 10 * D - 3600,
            now - 10 * D - 60,
            now - 10 * D,
            // younger than 7 days: all stay
            now - 2 * D,
            now - 2 * D + 1,
            now,
        ]
        .into_iter()
        .map(day)
        .chain([
            "20261015T212937Z-2.md.zst".to_owned(),
            "unknown.md.zst".to_owned(),
        ])
        .collect();
        let mut sorted = names.clone();
        sorted.sort_by(|a, b| sort_key(a).cmp(&sort_key(b)));

        let gone = to_thin(&sorted, Retention::default(), now);
        let expected: Vec<String> = [
            now - 136 * D,
            now - 133 * D,
            now - 10 * D - 3600,
            now - 10 * D - 60,
        ]
        .into_iter()
        .map(day)
        .collect();
        assert_eq!(gone, expected);
    }

    #[test]
    fn thinning_removes_files_and_spares_others() {
        let (dir, vault) = vault();
        let now = 1_792_099_777;
        let history = dir.path().join(HISTORY_DIR).join(ID);
        std::fs::create_dir_all(&history).unwrap();
        let old = [now - 202 * 86_400, now - 201 * 86_400]; // Friday, Saturday
        for secs in old {
            std::fs::write(history.join(day(secs)), "x").unwrap();
        }
        std::fs::write(history.join("notes.txt"), "not a snapshot").unwrap();
        std::fs::create_dir_all(dir.path().join(HISTORY_DIR).join("not-an-id")).unwrap();

        assert_eq!(
            vault
                .thin_history(Retention::default(), now * 1000)
                .unwrap(),
            1
        );
        assert!(!history.join(day(old[0])).exists());
        assert!(history.join(day(old[1])).exists());
        assert!(history.join("notes.txt").exists());
        assert_eq!(
            vault
                .thin_history(Retention::default(), now * 1000)
                .unwrap(),
            0
        );
    }

    #[test]
    fn versions_list_read_restore_and_copy() {
        let (dir, vault) = vault();
        let first = vault.create_titled("P", "# Antenne\none\n").unwrap();
        let path = first.path.clone();
        assert_eq!(vault.versions(&path).unwrap(), []);
        vault.snapshot_note(&path, None).unwrap();
        let two = vault
            .write_note(&path, "# Antenne\ntwo\n", &first.saved.hash)
            .unwrap();

        let versions = vault.versions(&path).unwrap();
        assert_eq!(versions.len(), 1);
        assert!(versions[0].taken_ms.is_some_and(|ms| ms <= now_ms()));
        let old = vault.read_version(&path, &versions[0].name).unwrap();
        assert_eq!(old, first.saved.text);

        // A stale hash restores nothing.
        assert!(matches!(
            vault.restore_version(&path, &versions[0].name, &first.saved.hash),
            Err(VaultError::Conflict { .. })
        ));
        let restored = vault
            .restore_version(&path, &versions[0].name, &two.hash)
            .unwrap();
        assert_eq!(front_matter::body(&restored.text), "# Antenne\none\n");
        let id = front_matter::get(&first.saved.text, "id").unwrap();
        assert_eq!(front_matter::get(&restored.text, "id").unwrap(), id);
        // The replaced text is the newest version now.
        let versions = vault.versions(&path).unwrap();
        assert_eq!(versions.len(), 2);
        assert_eq!(
            vault.read_version(&path, &versions[0].name).unwrap(),
            two.text
        );

        let copy = vault.copy_version(&path, &versions[1].name).unwrap();
        assert_eq!(copy.path, "P/antenne-2.md");
        let copy_id = front_matter::get(&copy.saved.text, "id").unwrap();
        assert_ne!(copy_id, id);
        assert!(is_ulid(&copy_id));
        assert_eq!(front_matter::body(&copy.saved.text), "# Antenne\none\n");
        assert_eq!(
            std::fs::read_to_string(dir.path().join("notes").join(&path)).unwrap(),
            restored.text
        );
    }

    #[test]
    fn a_note_without_id_has_no_versions() {
        let (_dir, vault) = vault();
        std::fs::write(vault.notes_dir().join("bare.md"), "no id").unwrap();
        assert_eq!(vault.versions("bare.md").unwrap(), []);
        assert!(matches!(
            vault.read_version("bare.md", "20261015T212937Z.md.zst"),
            Err(VaultError::InvalidId(_))
        ));
    }

    #[test]
    fn retention_from_settings() {
        let defaults = Retention::default();
        assert_eq!(Retention::from_settings(""), defaults);
        assert_eq!(
            Retention::from_settings("history_keep_all_days: 14\nhistory_keep_daily_days: 365\n"),
            Retention {
                all_days: 14,
                daily_days: 365
            }
        );
        // Invalid values keep their default; daily never ends before "all".
        assert_eq!(
            Retention::from_settings("history_keep_all_days: -3\nhistory_keep_daily_days: lots\n"),
            defaults
        );
        assert_eq!(
            Retention::from_settings("history_keep_all_days: 120 # four months\n"),
            Retention {
                all_days: 120,
                daily_days: 120
            }
        );
    }

    /// format.md §7: the example settings in `tests/fixtures/format/7-settings/`.
    #[test]
    fn golden_settings() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/7-settings");
        let settings = std::fs::read_to_string(fixtures.join("settings.yaml")).unwrap();
        assert_eq!(
            Retention::from_settings(&settings),
            Retention {
                all_days: 14,
                daily_days: 180
            }
        );
    }

    #[test]
    fn vault_reads_its_settings() {
        let (dir, vault) = vault();
        assert_eq!(vault.retention(), Retention::default());
        std::fs::write(
            dir.path().join(crate::layout::SETTINGS_FILE),
            "history_keep_all_days: 3\nother_setting: kept\n",
        )
        .unwrap();
        assert_eq!(
            vault.retention(),
            Retention {
                all_days: 3,
                daily_days: 90
            }
        );
    }
}
