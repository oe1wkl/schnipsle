// SPDX-License-Identifier: Apache-2.0
//! The trash (format.md §8, ADR-0011).
//!
//! Deleting moves a note's file unchanged to `.notesapp/trash/<key>.md` and
//! writes `.notesapp/trash/<key>.yaml` with `deleted` and `original_path`. The
//! key is the note's `id`; a note without a valid id gets a new ULID instead,
//! and a key already in use gets `-2`, `-3`, … . The note's history stays where
//! it is. Restoring moves the file back to its original path or, if that is
//! taken, to the notes folder (format.md §8: `notes/`; until the vault layout of
//! §1 is in place, the vault root) with a `-n` suffix on collision. It never
//! replaces anything.

use std::fmt::Write as _;
use std::fs;
use std::io;

use serde::Serialize;

use crate::front_matter;
use crate::stamp::{is_ulid, new_ulid, now_ms, rfc3339_utc};
use crate::vault::{Access, Result, TempFile, Vault, VaultError, accessor_ran_twice, title_of};

/// Folder of the trash, relative to the vault root.
pub const TRASH_DIR: &str = ".notesapp/trash";

const MAX_SUFFIX: u32 = 1000;

/// A note in the trash. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TrashedNote {
    /// Name of its files in the trash, without extension; pass it to
    /// [`Vault::restore_note`].
    pub key: String,
    /// Title per format.md §5.2.
    pub title: String,
    /// Where it was, vault-relative; empty if unknown.
    pub original_path: String,
    /// When it was deleted, RFC 3339; empty if unknown.
    pub deleted: String,
}

impl Vault {
    /// Moves a note to the trash, unchanged; returns its trash entry.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`], or I/O errors (e.g. the note is gone); the
    /// note stays where it is then.
    pub fn trash_note(&self, path: &str) -> Result<TrashedNote> {
        let full = self.resolve(path)?;
        let text = self.coordinated(Access::Read, &full, |file| {
            Ok(String::from_utf8_lossy(&fs::read(file)?).into_owned())
        })?;
        let id = front_matter::get(&text, "id")
            .filter(|id| is_ulid(id))
            .map_or_else(new_ulid, |id| Ok(id.to_ascii_uppercase()))?;
        let dir = self.root().join(TRASH_DIR);
        fs::create_dir_all(&dir)?;
        let deleted = rfc3339_utc(now_ms());
        let record = format!("deleted: {deleted}\noriginal_path: {}\n", yaml_quote(path));

        for n in 1..=MAX_SUFFIX {
            let key = if n == 1 {
                id.clone()
            } else {
                format!("{id}-{n}")
            };
            let yaml = dir.join(format!("{key}.yaml"));
            // The record first: a crash before the move leaves only a stray
            // record, never a trashed note that forgot where it came from.
            let mut tmp = Some(TempFile::write(&yaml, record.as_bytes())?);
            let placed = self.coordinated(Access::Write, &yaml, |file| {
                Ok(tmp.take().ok_or_else(accessor_ran_twice)?.place_new(file)?)
            });
            match placed {
                Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                other => other?,
            }
            return match self.move_new(&full, &dir.join(format!("{key}.md"))) {
                Ok(()) => Ok(TrashedNote {
                    title: title_of(&text, file_stem(path)),
                    key,
                    original_path: path.to_owned(),
                    deleted,
                }),
                Err(e) => {
                    let _ = fs::remove_file(&yaml);
                    if matches!(&e, VaultError::Io(io) if io.kind() == io::ErrorKind::AlreadyExists)
                    {
                        continue; // a stray `<key>.md` without record
                    }
                    Err(e)
                }
            };
        }
        Err(io::Error::other("no free name in the trash").into())
    }

    /// The notes in the trash, most recently deleted first.
    ///
    /// # Errors
    /// I/O errors reading the trash folder; an empty or missing trash is fine.
    pub fn list_trash(&self) -> Result<Vec<TrashedNote>> {
        let dir = self.root().join(TRASH_DIR);
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        let mut notes = Vec::new();
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            let Some(key) = name.strip_suffix(".md").filter(|k| is_key(k)) else {
                continue;
            };
            let text = fs::read(entry.path())
                .map(|b| String::from_utf8_lossy(&b).into_owned())
                .unwrap_or_default();
            let record = fs::read_to_string(dir.join(format!("{key}.yaml"))).unwrap_or_default();
            notes.push(TrashedNote {
                key: key.to_owned(),
                title: title_of(&text, key),
                original_path: record_value(&record, "original_path").unwrap_or_default(),
                deleted: record_value(&record, "deleted").unwrap_or_default(),
            });
        }
        notes.sort_by(|a, b| b.deleted.cmp(&a.deleted).then_with(|| a.key.cmp(&b.key)));
        Ok(notes)
    }

    /// Moves a note out of the trash, back to its original path or, if that
    /// is taken or unknown, into the notes folder under its file name, with a
    /// `-n` suffix on collision. Returns where it is now.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] for a bad key, or I/O errors.
    pub fn restore_note(&self, key: &str) -> Result<String> {
        if !is_key(key) {
            return Err(VaultError::InvalidPath(key.to_owned()));
        }
        let dir = self.root().join(TRASH_DIR);
        let trashed = dir.join(format!("{key}.md"));
        let yaml = dir.join(format!("{key}.yaml"));
        let record = fs::read_to_string(&yaml).unwrap_or_default();
        let original = record_value(&record, "original_path")
            .filter(|p| self.resolve(p).is_ok())
            .unwrap_or_else(|| format!("{key}.md"));
        let stem = original
            .strip_suffix(".md")
            .map_or(key, |p| p.rsplit_once('/').map_or(p, |(_, name)| name));
        // Then the notes folder of format.md §1: the vault root until that
        // layout exists.
        let fallback = (1..=MAX_SUFFIX).map(|n| {
            if n == 1 {
                format!("{stem}.md")
            } else {
                format!("{stem}-{n}.md")
            }
        });
        for target in std::iter::once(original.clone()).chain(fallback) {
            match self.move_new(&trashed, &self.resolve(&target)?) {
                Ok(()) => {
                    let _ = fs::remove_file(&yaml);
                    return Ok(target);
                }
                Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Err(io::Error::other("no free name to restore to").into())
    }
}

/// A trash key: a ULID, optionally with `-n`.
fn is_key(key: &str) -> bool {
    let (id, n) = key.split_once('-').unwrap_or((key, "2"));
    is_ulid(id) && n.parse::<u32>().is_ok_and(|n| n >= 2)
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    name.strip_suffix(".md").unwrap_or(name)
}

/// A YAML double-quoted scalar.
fn yaml_quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The value of `key` in a record this module wrote (plain or double-quoted).
fn record_value(record: &str, key: &str) -> Option<String> {
    let value = record
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))?
        .trim();
    let Some(inner) = value.strip_prefix('"').and_then(|v| v.strip_suffix('"')) else {
        return Some(value.to_owned());
    };
    let mut out = String::with_capacity(inner.len());
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('u') => {
                let hex: String = chars.by_ref().take(4).collect();
                out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HISTORY_DIR;

    const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn deleting_moves_the_file_unchanged_under_its_id() {
        let (dir, vault) = vault();
        let note = format!("---\nid: {ID}\n---\n# Antenna\n");
        fs::create_dir(dir.path().join("projects")).unwrap();
        fs::write(dir.path().join("projects/antenna.md"), &note).unwrap();
        vault.snapshot(ID, "older text").unwrap();

        let trashed = vault.trash_note("projects/antenna.md").unwrap();
        assert_eq!(trashed.key, ID);
        assert_eq!(trashed.title, "Antenna");
        assert!(!dir.path().join("projects/antenna.md").exists());
        let trash = dir.path().join(TRASH_DIR);
        assert_eq!(
            fs::read_to_string(trash.join(format!("{ID}.md"))).unwrap(),
            note
        );
        let record = fs::read_to_string(trash.join(format!("{ID}.yaml"))).unwrap();
        assert!(
            record.contains("original_path: \"projects/antenna.md\""),
            "{record}"
        );
        assert!(record.starts_with("deleted: 20"), "{record}");
        // History stays; the note is no longer listed.
        assert_eq!(vault.snapshots(ID).unwrap().len(), 1);
        assert!(dir.path().join(HISTORY_DIR).join(ID).exists());
        assert!(vault.list_notes().unwrap().is_empty());
        assert_eq!(vault.list_trash().unwrap(), [trashed]);
    }

    #[test]
    fn a_note_without_id_gets_a_new_key_and_stays_unchanged() {
        let (dir, vault) = vault();
        fs::write(dir.path().join("plain.md"), "no front matter").unwrap();
        let trashed = vault.trash_note("plain.md").unwrap();
        assert!(is_ulid(&trashed.key));
        let moved = dir
            .path()
            .join(TRASH_DIR)
            .join(format!("{}.md", trashed.key));
        assert_eq!(fs::read_to_string(moved).unwrap(), "no front matter");
    }

    #[test]
    fn same_ids_get_numbered_keys() {
        let (dir, vault) = vault();
        let note = format!("---\nid: {ID}\n---\nx");
        fs::write(dir.path().join("a.md"), &note).unwrap();
        fs::write(dir.path().join("a 2.md"), &note).unwrap();
        assert_eq!(vault.trash_note("a.md").unwrap().key, ID);
        assert_eq!(vault.trash_note("a 2.md").unwrap().key, format!("{ID}-2"));
        assert_eq!(vault.list_trash().unwrap().len(), 2);
    }

    #[test]
    fn restore_goes_back_or_next_to_a_taken_path() {
        let (dir, vault) = vault();
        let note = format!("---\nid: {ID}\n---\n# Antenna\n");
        fs::write(dir.path().join("antenna.md"), &note).unwrap();
        let trashed = vault.trash_note("antenna.md").unwrap();
        fs::write(dir.path().join("antenna.md"), "# A new note meanwhile\n").unwrap();

        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "antenna-2.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("antenna-2.md")).unwrap(),
            note
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("antenna.md")).unwrap(),
            "# A new note meanwhile\n"
        );
        assert!(vault.list_trash().unwrap().is_empty());
        assert_eq!(fs::read_dir(dir.path().join(TRASH_DIR)).unwrap().count(), 0);
    }

    #[test]
    fn restore_falls_back_to_the_notes_folder() {
        let (dir, vault) = vault();
        fs::create_dir(dir.path().join("projects")).unwrap();
        fs::write(dir.path().join("projects/antenna.md"), "mine").unwrap();
        let trashed = vault.trash_note("projects/antenna.md").unwrap();
        fs::write(dir.path().join("projects/antenna.md"), "new").unwrap();
        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "antenna.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("antenna.md")).unwrap(),
            "mine"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("projects/antenna.md")).unwrap(),
            "new"
        );
    }

    #[test]
    fn restore_into_a_folder_that_is_gone() {
        let (dir, vault) = vault();
        fs::create_dir(dir.path().join("old")).unwrap();
        fs::write(dir.path().join("old/n.md"), "x").unwrap();
        let trashed = vault.trash_note("old/n.md").unwrap();
        fs::remove_dir(dir.path().join("old")).unwrap();
        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "old/n.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("old/n.md")).unwrap(),
            "x"
        );
    }

    #[test]
    fn keys_and_records_are_checked() {
        let (_dir, vault) = vault();
        for bad in [
            "../x",
            "",
            "not-a-key",
            &format!("{ID}-1"),
            &format!("{ID}/x"),
        ] {
            assert!(
                matches!(vault.restore_note(bad), Err(VaultError::InvalidPath(_))),
                "{bad}"
            );
        }
        let odd = "dir/Notiz \"mit\" Zeichen\\.md";
        let record = format!("original_path: {}\n", yaml_quote(odd));
        assert_eq!(record_value(&record, "original_path").as_deref(), Some(odd));
        assert_eq!(
            record_value("deleted: 2026-10-16T10:00:00Z", "deleted").as_deref(),
            Some("2026-10-16T10:00:00Z")
        );
    }
}
