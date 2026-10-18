// SPDX-License-Identifier: Apache-2.0
//! The trash (format.md §8, ADR-0011).
//!
//! Deleting moves a note's file unchanged to `.notesapp/trash/<key>.md` and
//! writes `.notesapp/trash/<key>.yaml` with `deleted` and `original_path`. The
//! key is the note's `id`; a note without a valid id gets a new ULID instead,
//! and a key already in use gets `-2`, `-3`, … . The note's history stays where
//! it is. `original_path` is relative to the vault root (`notes/…`). Restoring
//! moves the file back there or, if that is taken or not inside `notes/`, into
//! `notes/` under its file name, with a `-n` suffix on collision. It never
//! replaces anything.

use std::fs;
use std::io;

use serde::Serialize;

use crate::flat_yaml::{quote as yaml_quote, value as record_value};
use crate::front_matter;
use crate::layout::NOTES_DIR;
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
    /// Where it was, relative to the vault root (`notes/…`); empty if unknown.
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
        self.check_writable()?;
        let full = self.resolve(path)?;
        let bytes = self.coordinated(Access::Read, &full, |file| Ok(fs::read(file)?))?;
        let text = String::from_utf8_lossy(&bytes).into_owned();
        let valid_id = front_matter::get(&text, "id").filter(|id| is_ulid(id));
        if let Some(id) = &valid_id
            && let Err(e) = self.snapshot_if_new(id, &bytes)
        {
            // The trash keeps the file itself, so this snapshot is a second copy.
            eprintln!("no snapshot of {path} before deleting it: {e}");
        }
        let id = valid_id.map_or_else(new_ulid, |id| Ok(id.to_ascii_uppercase()))?;
        let dir = self.root().join(TRASH_DIR);
        fs::create_dir_all(&dir)?;
        let deleted = rfc3339_utc(now_ms());
        let original_path = format!("{NOTES_DIR}/{path}");
        let record = format!(
            "deleted: {deleted}\noriginal_path: {}\n",
            yaml_quote(&original_path)
        );

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
                    original_path,
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
    /// is taken, unknown or not inside `notes/`, into `notes/` under its file
    /// name, with a `-n` suffix on collision. If it lands in another folder,
    /// its relative links are rewritten to keep working (ADR-0016). Returns
    /// its path relative to `notes/`.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] for a bad key, or I/O errors.
    pub fn restore_note(&self, key: &str) -> Result<String> {
        self.check_writable()?;
        if !is_key(key) {
            return Err(VaultError::InvalidPath(key.to_owned()));
        }
        let dir = self.root().join(TRASH_DIR);
        let trashed = dir.join(format!("{key}.md"));
        let yaml = dir.join(format!("{key}.yaml"));
        let record = fs::read_to_string(&yaml).unwrap_or_default();
        let recorded = record_value(&record, "original_path").unwrap_or_default();
        let original = recorded
            .strip_prefix(NOTES_DIR)
            .and_then(|p| p.strip_prefix('/'))
            .filter(|p| self.resolve(p).is_ok())
            .map(str::to_owned);
        let stem = Some(file_stem(&recorded))
            .filter(|stem| self.resolve(&format!("{stem}.md")).is_ok())
            .unwrap_or(key);
        let fallback = (1..=MAX_SUFFIX).map(|n| {
            if n == 1 {
                format!("{stem}.md")
            } else {
                format!("{stem}-{n}.md")
            }
        });
        // The folder its links were written for (ADR-0016); unknown for
        // records of earlier builds.
        let old_dir: Option<Vec<String>> = recorded
            .strip_prefix(NOTES_DIR)
            .filter(|p| p.starts_with('/'))
            .map(|_| {
                let mut parts: Vec<String> = recorded.split('/').map(str::to_owned).collect();
                parts.pop();
                parts
            });
        for target in original.into_iter().chain(fallback) {
            match self.move_new(&trashed, &self.resolve(&target)?) {
                Ok(()) => {
                    let _ = fs::remove_file(&yaml);
                    if let Some(old_dir) = old_dir.filter(|d| *d != notes_dir_of(&target)) {
                        // The note is back either way; broken links are reported, not fatal.
                        if let Err(e) = self.relink_after_move(&target, &old_dir, &|p| p.to_vec()) {
                            eprintln!("links in {target} not updated: {e}");
                        }
                    }
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

/// The folder of a note path, below the vault root: `a/b.md` → `notes/a`.
fn notes_dir_of(path: &str) -> Vec<String> {
    let mut parts: Vec<String> = std::iter::once(NOTES_DIR)
        .chain(path.split('/'))
        .map(str::to_owned)
        .collect();
    parts.pop();
    parts
}

fn file_stem(path: &str) -> &str {
    let name = path.rsplit_once('/').map_or(path, |(_, name)| name);
    name.strip_suffix(".md").unwrap_or(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::history::HISTORY_DIR;

    const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn deleting_moves_the_file_unchanged_under_its_id() {
        let (dir, vault) = vault();
        let note = format!("---\nid: {ID}\n---\n# Antenna\n");
        fs::create_dir(dir.path().join("notes/projects")).unwrap();
        fs::write(dir.path().join("notes/projects/antenna.md"), &note).unwrap();
        vault.snapshot(ID, "older text").unwrap();

        let trashed = vault.trash_note("projects/antenna.md").unwrap();
        assert_eq!(trashed.key, ID);
        assert_eq!(trashed.title, "Antenna");
        assert!(!dir.path().join("notes/projects/antenna.md").exists());
        let trash = dir.path().join(TRASH_DIR);
        assert_eq!(
            fs::read_to_string(trash.join(format!("{ID}.md"))).unwrap(),
            note
        );
        let record = fs::read_to_string(trash.join(format!("{ID}.yaml"))).unwrap();
        assert!(
            record.contains("original_path: \"notes/projects/antenna.md\""),
            "{record}"
        );
        assert!(record.starts_with("deleted: 20"), "{record}");
        // History stays, with the deleted text added last (ADR-0011); the
        // note is no longer listed.
        let snapshots = vault.snapshots(ID).unwrap();
        assert_eq!(snapshots.len(), 2);
        assert_eq!(vault.read_snapshot(ID, &snapshots[1]).unwrap(), note);
        assert!(dir.path().join(HISTORY_DIR).join(ID).exists());
        assert!(vault.list_notes().unwrap().is_empty());
        assert_eq!(vault.list_trash().unwrap(), [trashed]);
    }

    #[test]
    fn a_note_without_id_gets_a_new_key_and_stays_unchanged() {
        let (dir, vault) = vault();
        fs::write(dir.path().join("notes/plain.md"), "no front matter").unwrap();
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
        fs::write(dir.path().join("notes/a.md"), &note).unwrap();
        fs::write(dir.path().join("notes/a 2.md"), &note).unwrap();
        assert_eq!(vault.trash_note("a.md").unwrap().key, ID);
        assert_eq!(vault.trash_note("a 2.md").unwrap().key, format!("{ID}-2"));
        assert_eq!(vault.list_trash().unwrap().len(), 2);
    }

    #[test]
    fn restore_goes_back_or_next_to_a_taken_path() {
        let (dir, vault) = vault();
        let note = format!("---\nid: {ID}\n---\n# Antenna\n");
        fs::write(dir.path().join("notes/antenna.md"), &note).unwrap();
        let trashed = vault.trash_note("antenna.md").unwrap();
        fs::write(
            dir.path().join("notes/antenna.md"),
            "# A new note meanwhile\n",
        )
        .unwrap();

        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "antenna-2.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/antenna-2.md")).unwrap(),
            note
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/antenna.md")).unwrap(),
            "# A new note meanwhile\n"
        );
        assert!(vault.list_trash().unwrap().is_empty());
        assert_eq!(fs::read_dir(dir.path().join(TRASH_DIR)).unwrap().count(), 0);
    }

    #[test]
    fn restore_falls_back_to_the_notes_folder() {
        let (dir, vault) = vault();
        fs::create_dir(dir.path().join("notes/projects")).unwrap();
        fs::write(dir.path().join("notes/projects/antenna.md"), "mine").unwrap();
        let trashed = vault.trash_note("projects/antenna.md").unwrap();
        fs::write(dir.path().join("notes/projects/antenna.md"), "new").unwrap();
        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "antenna.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/antenna.md")).unwrap(),
            "mine"
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/projects/antenna.md")).unwrap(),
            "new"
        );
    }

    #[test]
    fn restore_into_a_folder_that_is_gone() {
        let (dir, vault) = vault();
        fs::create_dir(dir.path().join("notes/old")).unwrap();
        fs::write(dir.path().join("notes/old/n.md"), "x").unwrap();
        let trashed = vault.trash_note("old/n.md").unwrap();
        fs::remove_dir(dir.path().join("notes/old")).unwrap();
        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "old/n.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/old/n.md")).unwrap(),
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

    #[test]
    fn restore_ignores_paths_outside_notes() {
        let (dir, vault) = vault();
        let trash = dir.path().join(TRASH_DIR);
        fs::create_dir_all(&trash).unwrap();
        let other = "01ARZ3NDEKTSV4RRFFQ69G5FAW";
        for (key, original) in [
            // Written by the M1 builds before the notes/ folder existed.
            (ID, "projekte/antenne.md"),
            (other, "../outside.md"),
        ] {
            fs::write(trash.join(format!("{key}.md")), key).unwrap();
            fs::write(
                trash.join(format!("{key}.yaml")),
                format!("original_path: {}\n", yaml_quote(original)),
            )
            .unwrap();
        }
        assert_eq!(vault.restore_note(ID).unwrap(), "antenne.md");
        assert_eq!(vault.restore_note(other).unwrap(), "outside.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/outside.md")).unwrap(),
            other
        );
        assert!(!dir.path().join("outside.md").exists());
        assert!(!dir.path().join("projekte").exists());
    }

    #[test]
    fn a_restore_into_another_folder_keeps_links_working() {
        let (dir, vault) = vault();
        let root = dir.path();
        let png = vault
            .add_attachment("projects/antenna.md", "plan.png", b"png")
            .unwrap();
        fs::create_dir_all(root.join("notes/projects")).unwrap();
        fs::write(root.join("notes/projects/photo.png"), "next to it").unwrap();
        let note = format!(
            "---\nid: {ID}\n---\n# Antenna\n{}\n![p](photo.png) ![gone](missing.png) \
             [web](https://example.org)\n",
            png.markdown
        );
        fs::write(root.join("notes/projects/antenna.md"), &note).unwrap();
        let trashed = vault.trash_note("projects/antenna.md").unwrap();
        fs::write(root.join("notes/projects/antenna.md"), "taken").unwrap();

        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "antenna.md");
        let restored = fs::read_to_string(root.join("notes/antenna.md")).unwrap();
        assert!(
            restored.contains(&format!("](../{})", png.path)),
            "{restored}"
        );
        assert!(restored.contains("![p](projects/photo.png)"), "{restored}");
        assert!(restored.contains("![gone](missing.png)"), "{restored}");
        assert!(
            restored.contains("[web](https://example.org)"),
            "{restored}"
        );
        assert!(restored.contains(&format!("id: {ID}")), "{restored}");
        // The text as deleted is in the history.
        let snapshots = vault.snapshots(ID).unwrap();
        let last = snapshots.last().unwrap();
        assert_eq!(vault.read_snapshot(ID, last).unwrap(), note);
        // Both images resolve from the restored note.
        for link in [
            png.path.replacen("attachments", "../attachments", 1),
            "projects/photo.png".into(),
        ] {
            assert!(
                vault.read_linked_file("antenna.md", &link).is_ok(),
                "{link}"
            );
        }
    }

    #[test]
    fn a_restore_to_the_same_folder_changes_nothing() {
        let (dir, vault) = vault();
        let note = format!("---\nid: {ID}\n---\n![p](photo.png)\n");
        fs::create_dir_all(dir.path().join("notes/p")).unwrap();
        fs::write(dir.path().join("notes/p/photo.png"), "x").unwrap();
        fs::write(dir.path().join("notes/p/n.md"), &note).unwrap();
        let trashed = vault.trash_note("p/n.md").unwrap();
        fs::write(dir.path().join("notes/p/n.md"), "taken").unwrap();
        fs::write(dir.path().join("notes/n.md"), "taken too").unwrap();
        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "n-2.md");
        // Restored one level up: the link is rewritten …
        assert!(
            fs::read_to_string(dir.path().join("notes/n-2.md"))
                .unwrap()
                .contains("![p](p/photo.png)")
        );
        // … but a restore to its own path leaves the text alone.
        let back = vault.trash_note("n-2.md").unwrap();
        let before =
            fs::read_to_string(dir.path().join(TRASH_DIR).join(format!("{}.md", back.key)))
                .unwrap();
        assert_eq!(vault.restore_note(&back.key).unwrap(), "n-2.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/n-2.md")).unwrap(),
            before
        );
    }
}
