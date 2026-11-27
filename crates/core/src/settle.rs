// SPDX-License-Identifier: Apache-2.0
//! The two choices of the conflict banner (ADR-0042): the note changed or
//! was deleted elsewhere while the editor had unsaved text. "Keep mine"
//! ([`Vault::keep_mine`]) writes the editor's text, "Use other version"
//! ([`Vault::use_other`]) keeps what is on disk.
//!
//! Either way the version not chosen is stored first: in the note's history
//! (format.md §7), filed under the `id` the note has afterwards, or in the
//! trash (§8) if the note was deleted elsewhere. A note without a valid `id`
//! gets one, so its history can be found from it (ADR-0011). If the version
//! cannot be stored, nothing is written and the call fails.

use std::borrow::Cow;
use std::fs;
use std::io;

use serde::Serialize;

use crate::front_matter;
use crate::stamp::{is_ulid, new_ulid, rfc3339_utc};
use crate::trash::TrashedNote;
use crate::vault::{
    Access, NoteContent, Result, SavedNote, Vault, VaultError, created_ms, not_downloaded,
    placeholder_of, sha256_hex,
};

/// What "Use other version" left. Serialized with camelCase names.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct OtherUsed {
    /// The note as it is now on disk; `None` if it was deleted elsewhere.
    pub note: Option<NoteContent>,
    /// Where the editor's text went if the note was deleted elsewhere.
    pub trashed: Option<TrashedNote>,
}

/// The `id` in `text` if it is a valid one (a ULID).
fn valid_id(text: &str) -> Option<String> {
    front_matter::get(text, "id").filter(|id| is_ulid(id))
}

impl Vault {
    /// "Keep mine": writes `text` as the note at `path` (relative to
    /// `notes/`) after storing the version on disk, whose hash is
    /// `other_hash`, in the note's history. `None` means the note was
    /// deleted elsewhere; it is then created again.
    ///
    /// The snapshot is filed under the `id` of `text`, else that of the
    /// file; if neither has a valid one, `text` gets a new `id`. Returns
    /// what is now on disk.
    ///
    /// # Errors
    /// [`VaultError::Conflict`] if the note is no longer the version with
    /// `other_hash` (or is back after being deleted), with the hash now on
    /// disk; [`VaultError::ReadOnly`], [`VaultError::InvalidPath`], or I/O
    /// errors, such as a full disk. Nothing is written then.
    pub fn keep_mine(&self, path: &str, text: &str, other_hash: Option<&str>) -> Result<SavedNote> {
        self.check_writable()?;
        let full = self.resolve(path)?;
        let Some(other_hash) = other_hash else {
            return self.create_note(path, text).map_err(|e| match e {
                VaultError::AlreadyExists(_) => match self.read_note_text(path) {
                    Ok(note) => VaultError::Conflict {
                        path: path.to_owned(),
                        current_hash: Some(note.hash),
                    },
                    Err(_) => e,
                },
                e => e,
            });
        };
        let other = self.read_expected(path, &full, other_hash)?;
        let (text, id) = match valid_id(text) {
            Some(id) => (Cow::Borrowed(text), id),
            None => match valid_id(&String::from_utf8_lossy(&other)) {
                // `write_note` keeps the file's id, unless `text` has another one.
                Some(id) if front_matter::get(text, "id").is_none() => (Cow::Borrowed(text), id),
                found => {
                    let id = found.map_or_else(new_ulid, Ok)?;
                    (Cow::Owned(front_matter::set(text, "id", &id)), id)
                }
            },
        };
        self.snapshot_if_new(&id, &other)?;
        self.write_note(path, &text, other_hash)
    }

    /// "Use other version": keeps the note at `path` as it is on disk and
    /// stores `text`, the editor's version, in its history. If the note has
    /// no valid `id`, its front matter gets one (the `id` of `text` if that
    /// is valid, else a new one) and `created` (that of `text`, else the
    /// file's time, ADR-0044); nothing else in it changes.
    /// If the note was deleted elsewhere, `text` goes into the trash as the
    /// note deleted from `path`, so it can be restored.
    ///
    /// # Errors
    /// [`VaultError::Conflict`] if the note changed while its `id` was being
    /// added; [`VaultError::NotDownloaded`] for a note only in iCloud;
    /// [`VaultError::ReadOnly`], [`VaultError::InvalidPath`], or I/O errors,
    /// such as a full disk, or a note that is not UTF-8 and has no `id`.
    /// The note is unchanged then.
    pub fn use_other(&self, path: &str, text: &str) -> Result<OtherUsed> {
        self.check_writable()?;
        let full = self.resolve(path)?;
        // As in `read_note_text`: reading would wait for the download.
        if not_downloaded(&full) {
            return Err(VaultError::NotDownloaded(path.to_owned()));
        }
        let on_disk = self.coordinated(Access::Read, &full, |file| match fs::read(file) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        })?;
        let Some(bytes) = on_disk else {
            if placeholder_of(&full).exists() {
                return Err(VaultError::NotDownloaded(path.to_owned()));
            }
            return Ok(OtherUsed {
                note: None,
                trashed: Some(self.trash_text(path, text)?),
            });
        };
        if let Some(id) = valid_id(&String::from_utf8_lossy(&bytes)) {
            self.snapshot_if_new(&id, text.as_bytes())?;
        } else {
            let hash = sha256_hex(&bytes);
            let current = String::from_utf8(bytes)
                .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
            let id = valid_id(text).map_or_else(new_ulid, Ok)?;
            // The snapshot first: if it fails, the note stays as it is.
            self.snapshot_if_new(&id, text.as_bytes())?;
            let mut stamped = current;
            if front_matter::get(&stamped, "created").is_none() {
                let created = front_matter::get(text, "created")
                    .unwrap_or_else(|| rfc3339_utc(created_ms(&full)));
                stamped = front_matter::set(&stamped, "created", &created);
            }
            stamped = front_matter::set(&stamped, "id", &id);
            self.replace_expected(path, &full, &hash, stamped)?;
        }
        Ok(OtherUsed {
            note: Some(self.read_note(path)?),
            trashed: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::sha256_hex;

    const ID: &str = "01HF7YAT04EXCA2ARJ0J9F8K0P";

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    /// Every text in the history of `id`, oldest first.
    fn history(vault: &Vault, id: &str) -> Vec<String> {
        vault
            .snapshots(id)
            .unwrap()
            .iter()
            .map(|name| vault.read_snapshot(id, name).unwrap())
            .collect()
    }

    fn write(dir: &tempfile::TempDir, path: &str, text: &str) -> String {
        fs::write(dir.path().join("notes").join(path), text).unwrap();
        sha256_hex(text.as_bytes())
    }

    #[test]
    fn keep_mine_stores_the_other_version_under_the_notes_id() {
        let (dir, vault) = vault();
        let saved = vault.create_note("n.md", "# N\n\nmine\n").unwrap();
        let id = front_matter::get(&saved.text, "id").unwrap();
        let other = format!("---\nid: {id}\n---\n# N\n\nother\n");
        let other_hash = write(&dir, "n.md", &other);
        let kept = vault
            .keep_mine("n.md", "# N\n\nmine, edited\n", Some(&other_hash))
            .unwrap();
        assert!(kept.text.ends_with("mine, edited\n"));
        assert_eq!(front_matter::get(&kept.text, "id").as_deref(), Some(&*id));
        assert_eq!(history(&vault, &id), [other]);
    }

    #[test]
    fn keep_mine_gives_a_note_without_id_one() {
        let (dir, vault) = vault();
        let other_hash = write(&dir, "plain.md", "# Plain\n\nfrom another app\n");
        let kept = vault
            .keep_mine("plain.md", "# Plain\n\nmine\n", Some(&other_hash))
            .unwrap();
        let id = front_matter::get(&kept.text, "id").unwrap();
        assert!(is_ulid(&id));
        assert_eq!(history(&vault, &id), ["# Plain\n\nfrom another app\n"]);
        assert_eq!(vault.versions("plain.md").unwrap().len(), 1);
    }

    #[test]
    fn keep_mine_replaces_an_id_that_cannot_name_a_history() {
        let (dir, vault) = vault();
        let other_hash = write(&dir, "x.md", "---\nid: 7\n---\nother\n");
        let kept = vault
            .keep_mine("x.md", "---\nid: 7\n---\nmine\n", Some(&other_hash))
            .unwrap();
        let id = front_matter::get(&kept.text, "id").unwrap();
        assert!(is_ulid(&id));
        assert_eq!(history(&vault, &id), ["---\nid: 7\n---\nother\n"]);
    }

    #[test]
    fn keep_mine_writes_nothing_if_the_other_version_changed_again() {
        let (dir, vault) = vault();
        let first = write(&dir, "n.md", "one\n");
        write(&dir, "n.md", "two\n");
        let err = vault.keep_mine("n.md", "mine\n", Some(&first)).unwrap_err();
        assert!(
            matches!(&err, VaultError::Conflict { current_hash: Some(h), .. } if *h == sha256_hex(b"two\n"))
        );
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/n.md")).unwrap(),
            "two\n"
        );
    }

    #[test]
    fn keep_mine_creates_a_note_deleted_elsewhere_again() {
        let (_dir, vault) = vault();
        let kept = vault
            .keep_mine("gone.md", "# Gone\n\nmine\n", None)
            .unwrap();
        assert_eq!(vault.read_note("gone.md").unwrap().hash, kept.hash);
    }

    #[test]
    fn keep_mine_reports_a_note_that_is_back() {
        let (dir, vault) = vault();
        let back = write(&dir, "back.md", "back\n");
        let err = vault.keep_mine("back.md", "mine\n", None).unwrap_err();
        assert!(matches!(err, VaultError::Conflict { current_hash: Some(h), .. } if h == back));
    }

    #[test]
    fn use_other_stores_the_editors_text() {
        let (dir, vault) = vault();
        let saved = vault.create_note("n.md", "# N\n\nfirst\n").unwrap();
        let id = front_matter::get(&saved.text, "id").unwrap();
        let other = format!("---\nid: {id}\n---\n# N\n\nother\n");
        write(&dir, "n.md", &other);
        let used = vault.use_other("n.md", "# N\n\nmine\n").unwrap();
        assert_eq!(used.note.unwrap().text, other);
        assert!(used.trashed.is_none());
        assert_eq!(history(&vault, &id), ["# N\n\nmine\n"]);
    }

    #[test]
    fn use_other_gives_a_note_without_id_the_editors_id() {
        let (dir, vault) = vault();
        write(
            &dir,
            "plain.md",
            "# Plain\n\nfront matter removed elsewhere\n",
        );
        let mine = format!("---\nid: {ID}\ncreated: 2026-01-02T03:04:05Z\n---\n# Plain\n\nmine\n");
        let used = vault.use_other("plain.md", &mine).unwrap();
        let note = used.note.unwrap();
        assert_eq!(front_matter::get(&note.text, "id").as_deref(), Some(ID));
        // The creation time of the editor's text, not the file's (ADR-0044).
        assert_eq!(
            front_matter::get(&note.text, "created").as_deref(),
            Some("2026-01-02T03:04:05Z")
        );
        assert_eq!(
            front_matter::body(&note.text),
            "# Plain\n\nfront matter removed elsewhere\n"
        );
        assert_eq!(history(&vault, ID), [mine]);
        assert_eq!(vault.versions("plain.md").unwrap().len(), 1);
    }

    #[test]
    fn use_other_gives_a_note_without_any_id_a_new_one() {
        let (dir, vault) = vault();
        write(&dir, "plain.md", "other\n");
        let used = vault.use_other("plain.md", "mine\n").unwrap();
        let note = used.note.unwrap();
        let id = front_matter::get(&note.text, "id").unwrap();
        assert!(is_ulid(&id));
        assert_eq!(history(&vault, &id), ["mine\n"]);
    }

    #[test]
    fn use_other_puts_the_text_of_a_note_deleted_elsewhere_into_the_trash() {
        let (_dir, vault) = vault();
        let mine = format!("---\nid: {ID}\n---\n# Gone\n\nmine\n");
        let used = vault.use_other("sub/gone.md", &mine).unwrap();
        assert!(used.note.is_none());
        let trashed = used.trashed.unwrap();
        assert_eq!(trashed.key, ID);
        assert_eq!(trashed.original_path, "notes/sub/gone.md");
        assert_eq!(vault.read_trashed(&trashed.key).unwrap(), mine);
        assert_eq!(vault.restore_note(&trashed.key).unwrap(), "sub/gone.md");
    }

    #[cfg(unix)]
    #[test]
    fn nothing_changes_when_the_version_cannot_be_stored() {
        use std::os::unix::fs::PermissionsExt as _;
        let (dir, vault) = vault();
        let other_hash = write(&dir, "plain.md", "other\n");
        let history = dir.path().join(".notesapp/history");
        fs::create_dir_all(&history).unwrap();
        fs::set_permissions(&history, fs::Permissions::from_mode(0o555)).unwrap();
        if fs::create_dir(history.join("probe")).is_ok() {
            eprintln!("skipped: permissions are not enforced here (root?)");
            return;
        }
        let keep = vault.keep_mine("plain.md", "mine\n", Some(&other_hash));
        let other = vault.use_other("plain.md", "mine\n");
        fs::set_permissions(&history, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(keep.is_err() && other.is_err());
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/plain.md")).unwrap(),
            "other\n"
        );
    }
}
