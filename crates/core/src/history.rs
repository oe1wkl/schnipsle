// SPDX-License-Identifier: Apache-2.0
//! Version history snapshots (format.md §7, ADR-0011).
//!
//! A snapshot is the complete note file, zstd-compressed, stored as
//! `.notesapp/history/<note-id>/<YYYYMMDDTHHMMSSZ>[-n].md.zst`. Snapshots are
//! written like notes (atomic, coordinated) and never replace one another.
//! Retention and a history UI come later.

use std::fs;
use std::io::{self, Read as _};

use ruzstd::decoding::StreamingDecoder;
use ruzstd::encoding::{CompressionLevel, compress_to_vec};

use crate::stamp::{compact_utc, is_ulid, now_ms};
use crate::vault::{Access, Result, TempFile, Vault, VaultError, accessor_ran_twice};

/// Folder of all snapshots, relative to the vault root.
pub const HISTORY_DIR: &str = ".notesapp/history";

impl Vault {
    /// Stores `content` (the complete note file) as a new snapshot of the note
    /// with `id`; returns the snapshot's path relative to the vault root.
    ///
    /// # Errors
    /// [`VaultError::InvalidId`] if `id` is not a ULID, or I/O errors.
    pub fn snapshot(&self, id: &str, content: impl AsRef<[u8]>) -> Result<String> {
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
        let vault = Vault::open(dir.path()).unwrap();
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
}
