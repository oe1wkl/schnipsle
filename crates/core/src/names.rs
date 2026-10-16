// SPDX-License-Identifier: Apache-2.0
//! File names derived from note titles (format.md §3).
//!
//! The file name is cosmetic; notes are identified by `id`. Writers derive it
//! from the title and resolve collisions with `-2`, `-3`, … . Renames never
//! replace an existing file.

use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

use crate::vault::{Access, Result, SavedNote, Vault, VaultError, title_of};

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
fn candidate(folder: &str, stem: &str, n: u32) -> String {
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

/// A note just created, with the path chosen for it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NewNote {
    /// Vault-relative path.
    pub path: String,
    /// What was written.
    pub saved: SavedNote,
}

impl Vault {
    /// Creates a note in `folder` (vault-relative, `""` for the root), named
    /// after the title of `text` (format.md §3).
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] or I/O errors.
    pub fn create_titled(&self, folder: &str, text: &str) -> Result<NewNote> {
        let stem = slug(&title_of(text, "note"));
        for n in 1..=MAX_SUFFIX {
            let path = candidate(folder, &stem, n);
            match self.create_note(&path, text) {
                Ok(saved) => return Ok(NewNote { path, saved }),
                Err(VaultError::AlreadyExists(_)) => {}
                Err(e) => return Err(e),
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
        let vault = Vault::open(dir.path()).unwrap();
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
        fs::create_dir(dir.path().join("sub")).unwrap();
        let third = vault.create_titled("sub", "Shopping list\n").unwrap();
        assert_eq!(
            [
                first.path.as_str(),
                second.path.as_str(),
                third.path.as_str()
            ],
            ["new-note.md", "new-note-2.md", "sub/shopping-list.md"]
        );
        let on_disk = fs::read_to_string(dir.path().join("new-note.md")).unwrap();
        assert_eq!(on_disk, first.saved.text);
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
        assert!(!dir.path().join("new-note.md").exists());
        assert_eq!(
            fs::read_to_string(dir.path().join(&path)).unwrap(),
            saved.text
        );
    }

    #[test]
    fn keeps_the_name_when_the_title_did_not_change_or_already_fits() {
        let (dir, vault) = vault();
        fs::write(dir.path().join("Einkaufsliste.md"), "# Einkaufsliste\n").unwrap();
        // Opened and left: same title, even though the name is not a slug.
        assert_eq!(
            vault
                .rename_after_edit("Einkaufsliste.md", "Einkaufsliste")
                .unwrap(),
            "Einkaufsliste.md"
        );
        // Title changed only in case: the name still fits.
        fs::write(dir.path().join("Einkaufsliste.md"), "# EINKAUFSLISTE\n").unwrap();
        assert_eq!(
            vault
                .rename_after_edit("Einkaufsliste.md", "Einkaufsliste")
                .unwrap(),
            "Einkaufsliste.md"
        );
        fs::write(dir.path().join("notes-2.md"), "# Notes\n").unwrap();
        assert_eq!(
            vault.rename_after_edit("notes-2.md", "Old").unwrap(),
            "notes-2.md"
        );
    }

    #[test]
    fn a_rename_never_replaces_another_note() {
        let (dir, vault) = vault();
        fs::write(dir.path().join("antenna.md"), "# Antenna\nthe other note\n").unwrap();
        fs::write(dir.path().join(".antenna-2.md.icloud"), "placeholder").unwrap();
        fs::write(dir.path().join("draft.md"), "# Antenna\nmine\n").unwrap();
        let path = vault.rename_after_edit("draft.md", "Draft").unwrap();
        assert_eq!(path, "antenna-3.md");
        assert_eq!(
            fs::read_to_string(dir.path().join("antenna.md")).unwrap(),
            "# Antenna\nthe other note\n"
        );
        assert_eq!(
            front_matter::body(&fs::read_to_string(dir.path().join("antenna-3.md")).unwrap()),
            "# Antenna\nmine\n"
        );
    }

    #[test]
    fn renames_stay_in_their_folder() {
        let (dir, vault) = vault();
        fs::create_dir(dir.path().join("projects")).unwrap();
        fs::write(dir.path().join("projects/x.md"), "# Tuner\n").unwrap();
        assert_eq!(
            vault.rename_after_edit("projects/x.md", "x").unwrap(),
            "projects/tuner.md"
        );
    }
}
