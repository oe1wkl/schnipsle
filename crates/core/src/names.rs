// SPDX-License-Identifier: Apache-2.0
//! File names derived from note titles (format.md §3).
//!
//! The file name is cosmetic; notes are identified by `id`. Writers derive it
//! from the title and resolve collisions with `-2`, `-3`, … . Renames never
//! replace an existing file.

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::vault::{Access, Result, SavedNote, Vault, VaultError, placeholder_of, title_of};

/// Most characters of a file name stem.
const MAX_STEM: usize = 80;

/// How many `-n` suffixes are tried before giving up.
const MAX_SUFFIX: u32 = 1000;

/// The file name stem for `title`: lowercased, whitespace to `-`, characters
/// other than letters, digits, `-`, `_` and `.` removed, no leading `.`, at most
/// 80 characters; `note` if nothing is left.
#[must_use]
pub fn slug(title: &str) -> String {
    let mut out = String::new();
    let mut pending_dash = false;
    for c in title.nfc().flat_map(char::to_lowercase) {
        if c.is_alphanumeric() || c == '_' || c == '.' {
            if pending_dash && !out.is_empty() {
                out.push('-');
            }
            pending_dash = false;
            out.push(c);
        } else if c.is_whitespace() || c == '-' {
            pending_dash = true;
        }
    }
    let mut out: String = out.trim_start_matches('.').chars().take(MAX_STEM).collect();
    while out.ends_with(['-', '.']) {
        out.pop();
    }
    if out.is_empty() {
        "note".to_owned()
    } else {
        out
    }
}

/// `true` if `file_stem` already is `stem` or a numbered `stem-n`, ignoring
/// case: file systems such as APFS do not tell `Note.md` from `note.md`.
fn matches_stem(file_stem: &str, stem: &str) -> bool {
    let file_stem = file_stem.to_lowercase();
    file_stem == stem
        || file_stem
            .strip_prefix(stem)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|n| n.parse::<u32>().is_ok_and(|n| n >= 2))
}

/// `folder/stem.md`, or `folder/stem-n.md` for `n` ≥ 2.
pub(crate) fn candidate(folder: &str, stem: &str, n: u32) -> String {
    let name = if n == 1 {
        format!("{stem}.md")
    } else {
        format!("{stem}-{n}.md")
    };
    if folder.is_empty() {
        name
    } else {
        format!("{folder}/{name}")
    }
}

/// `a/b/Note.md` → (`a/b`, `Note`)
fn split_path(path: &str) -> (&str, &str) {
    let (folder, name) = path.rsplit_once('/').unwrap_or(("", path));
    let stem = name.get(..name.len().saturating_sub(3)).unwrap_or(name);
    (folder, stem)
}

/// Creates a note at the first free `folder/stem(-n).md` with `create`.
fn create_unique(
    folder: &str,
    stem: &str,
    create: impl Fn(&str) -> Result<SavedNote>,
) -> Result<NewNote> {
    for n in 1..=MAX_SUFFIX {
        let path = candidate(folder, stem, n);
        match create(&path) {
            Ok(saved) => return Ok(NewNote { path, saved }),
            Err(VaultError::AlreadyExists(_)) => {}
            Err(e) => return Err(e),
        }
    }
    Err(VaultError::AlreadyExists(candidate(
        folder, stem, MAX_SUFFIX,
    )))
}

/// A note just created, with the path chosen for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewNote {
    /// Path relative to `notes/`.
    pub path: String,
    /// What was written.
    pub saved: SavedNote,
}

impl Vault {
    /// Creates a note in `folder` (relative to `notes/`, `""` for `notes/` itself), named
    /// after the title of `text` (format.md §3).
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] or I/O errors.
    pub fn create_titled(&self, folder: &str, text: &str) -> Result<NewNote> {
        let stem = slug(&title_of(text, "note"));
        create_unique(folder, &stem, |path| self.create_note(path, text))
    }

    /// Creates a note with exactly `text` (see [`Vault::create_note_as_is`])
    /// in `folder`, named after `title` (format.md §3, §12.1).
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] or I/O errors.
    pub fn create_titled_as_is(&self, folder: &str, title: &str, text: &str) -> Result<NewNote> {
        create_unique(folder, &slug(title), |path| {
            self.create_note_as_is(path, text)
        })
    }

    /// The path `folder/<slug of title>.md`, with `-2`, `-3`, … if a note of
    /// that name exists or `reserved` says the path is spoken for (compared
    /// in lowercase); what [`Vault::create_titled_as_is`] would pick.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] or [`VaultError::AlreadyExists`] if no
    /// name is free.
    pub fn free_note_path(
        &self,
        folder: &str,
        title: &str,
        reserved: impl Fn(&str) -> bool,
    ) -> Result<String> {
        let stem = slug(title);
        for n in 1..=MAX_SUFFIX {
            let path = candidate(folder, &stem, n);
            let full = self.resolve(&path)?;
            if !reserved(&path.to_lowercase()) && !full.exists() && !placeholder_of(&full).exists()
            {
                return Ok(path);
            }
        }
        Err(VaultError::AlreadyExists(candidate(
            folder, &stem, MAX_SUFFIX,
        )))
    }

    /// After the user leaves a note: if its title is no longer `title_at_open`,
    /// renames the file to match the new title, in the same folder (format.md
    /// §3). Returns the note's path, new or unchanged. A file that already
    /// carries the title's name (also as `name-2`, or in other case) stays.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] or I/O errors; the note keeps its name then.
    pub fn rename_after_edit(&self, path: &str, title_at_open: &str) -> Result<String> {
        let full = self.resolve(path)?;
        let text = self.coordinated(Access::Read, &full, |file| {
            Ok(String::from_utf8_lossy(&std::fs::read(file)?).into_owned())
        })?;
        let (folder, file_stem) = split_path(path);
        let title = title_of(&text, file_stem);
        let stem = slug(&title);
        if title == title_at_open || matches_stem(file_stem, &stem) {
            return Ok(path.to_owned());
        }
        for n in 1..=MAX_SUFFIX {
            let target = candidate(folder, &stem, n);
            match self.move_new(&full, &self.resolve(&target)?) {
                Ok(()) => return Ok(target),
                Err(VaultError::Io(e)) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
                Err(e) => return Err(e),
            }
        }
        Ok(path.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::front_matter;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn slugs() {
        for (title, expected) in [
            ("Antenna test #radio", "antenna-test-radio"),
            ("Einkaufsliste", "einkaufsliste"),
            ("Äpfel & Birnen", "äpfel-birnen"),
            ("  Lots   of\tspace  ", "lots-of-space"),
            ("v1.2 release-notes", "v1.2-release-notes"),
            ("a/b\\c:d*e?f\"g<h>i|j", "abcdefghij"),
            ("...hidden", "hidden"),
            ("trailing - ", "trailing"),
            ("!!!", "note"),
            ("", "note"),
        ] {
            assert_eq!(slug(title), expected, "{title}");
        }
        assert_eq!(slug(&"x".repeat(200)).chars().count(), 80);
        // Decomposed input comes out composed (NFC).
        assert_eq!(slug("A\u{0308}pfel"), "äpfel");
    }

    #[test]
    fn stem_matching() {
        assert!(matches_stem("antenna", "antenna"));
        assert!(matches_stem("Antenna", "antenna"));
        assert!(matches_stem("antenna-2", "antenna"));
        assert!(matches_stem("antenna-17", "antenna"));
        assert!(!matches_stem("antenna-1", "antenna"));
        assert!(!matches_stem("antenna-x", "antenna"));
        assert!(!matches_stem("antennas", "antenna"));
    }

    #[test]
    fn created_notes_are_named_after_their_title() {
        let (dir, vault) = vault();
        let first = vault.create_titled("", "# New note\n\n").unwrap();
        let second = vault.create_titled("", "# New note\n\n").unwrap();
        fs::create_dir(dir.path().join("notes/sub")).unwrap();
        let third = vault.create_titled("sub", "Shopping list\n").unwrap();
        assert_eq!(
            [
                first.path.as_str(),
                second.path.as_str(),
                third.path.as_str()
            ],
            ["new-note.md", "new-note-2.md", "sub/shopping-list.md"]
        );
        let on_disk = fs::read_to_string(dir.path().join("notes/new-note.md")).unwrap();
        assert_eq!(on_disk, first.saved.text);
    }

    #[test]
    fn imported_notes_are_written_as_given() {
        let (_dir, vault) = vault();
        let text = "---\nid: 01JA7R0B2C3D4E5F6G7H8J9K0A\ncreated: 2020-01-01T00:00:00Z\n\
                    modified: 2021-02-02T00:00:00Z\n---\nBody\n";
        vault.create_note("Import/tuner.md", "# Other\n").unwrap();
        assert_eq!(
            vault
                .free_note_path("Import", "Tuner", |p| p == "import/tuner-2.md")
                .unwrap(),
            "Import/tuner-3.md"
        );
        let note = vault.create_titled_as_is("Import", "Tuner", text).unwrap();
        assert_eq!(note.path, "Import/tuner-2.md");
        assert_eq!(note.saved.text, text);
        assert_eq!(vault.read_note(&note.path).unwrap().text, text);

        let found = vault.find_note("01JA7R0B2C3D4E5F6G7H8J9K0A", Some(&note.path));
        assert_eq!(found.as_deref(), Some(note.path.as_str()));
        assert_eq!(
            vault.find_note("01JA7R0B2C3D4E5F6G7H8J9K0A", Some("Import/tuner.md")),
            None
        );

        let newer = text.replace("Body", "Newer body");
        let saved = vault
            .replace_note_as_is(&note.path, &newer, &note.saved.hash)
            .unwrap();
        assert_eq!(saved.text, newer);
        assert!(matches!(
            vault.replace_note_as_is(&note.path, text, &note.saved.hash),
            Err(VaultError::Conflict { .. })
        ));
    }

    #[test]
    fn renames_when_the_title_changed() {
        let (dir, vault) = vault();
        let note = vault.create_titled("", "# New note\n").unwrap();
        let saved = vault
            .write_note(
                &note.path,
                &note.saved.text.replace("# New note", "# Antenna test"),
                &note.saved.hash,
            )
            .unwrap();
        let path = vault.rename_after_edit(&note.path, "New note").unwrap();
        assert_eq!(path, "antenna-test.md");
        assert!(!dir.path().join("notes/new-note.md").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join("notes").join(&path)).unwrap(),
            saved.text
        );
    }

    #[test]
    fn keeps_the_name_when_the_title_did_not_change_or_already_fits() {
        let (dir, vault) = vault();
        fs::write(
            dir.path().join("notes/Einkaufsliste.md"),
            "# Einkaufsliste\n",
        )
        .unwrap();
        // Opened and left: same title, even though the name is not a slug.
        assert_eq!(
            vault
                .rename_after_edit("Einkaufsliste.md", "Einkaufsliste")
                .unwrap(),
            "Einkaufsliste.md"
        );
        // Title changed only in case: the name still fits.
        fs::write(
            dir.path().join("notes/Einkaufsliste.md"),
            "# EINKAUFSLISTE\n",
        )
        .unwrap();
        assert_eq!(
            vault
                .rename_after_edit("Einkaufsliste.md", "Einkaufsliste")
                .unwrap(),
            "Einkaufsliste.md"
        );
        fs::write(dir.path().join("notes/notes-2.md"), "# Notes\n").unwrap();
        assert_eq!(
            vault.rename_after_edit("notes-2.md", "Old").unwrap(),
            "notes-2.md"
        );
    }

    #[test]
    fn a_rename_never_replaces_another_note() {
        let (dir, vault) = vault();
        fs::write(
            dir.path().join("notes/antenna.md"),
            "# Antenna\nthe other note\n",
        )
        .unwrap();
        fs::write(dir.path().join("notes/.antenna-2.md.icloud"), "placeholder").unwrap();
        fs::write(dir.path().join("notes/draft.md"), "# Antenna\nmine\n").unwrap();
        let path = vault.rename_after_edit("draft.md", "Draft").unwrap();
        assert_eq!(path, "antenna-3.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("notes/antenna.md")).unwrap(),
            "# Antenna\nthe other note\n"
        );
        assert_eq!(
            front_matter::body(&fs::read_to_string(dir.path().join("notes/antenna-3.md")).unwrap()),
            "# Antenna\nmine\n"
        );
    }

    #[test]
    fn renames_stay_in_their_folder() {
        let (dir, vault) = vault();
        fs::create_dir(dir.path().join("notes/projects")).unwrap();
        fs::write(dir.path().join("notes/projects/x.md"), "# Tuner\n").unwrap();
        assert_eq!(
            vault.rename_after_edit("projects/x.md", "x").unwrap(),
            "projects/tuner.md"
        );
    }
}
