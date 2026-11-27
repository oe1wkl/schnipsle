// SPDX-License-Identifier: Apache-2.0
//! The manual as notes (ADR-0048 rule 6, ADR-0051).
//!
//! The manual is a vault in the repository, `docs/manual/`: its notes are in
//! `notes/schnipsle-manual/`, its pictures in `attachments/` (format.md §6).
//! The app carries these files and adds them to a vault with
//! [`Vault::add_manual`]: the notes into the same folder `notes/schnipsle-manual/`,
//! so their relative links stay as written, the pictures into `attachments/`.
//!
//! A manual note is added only if it is missing: no note in the vault has its
//! `id`, none in the trash has it, and neither its file nor its iCloud
//! placeholder is in that folder. So a note the user moved, changed or
//! deleted is never added again.

use std::fs;
use std::path::Path;

use serde::Serialize;

use crate::front_matter;
use crate::layout::NOTES_DIR;
use crate::stamp::is_ulid;
use crate::vault::{Result, Vault, VaultError, placeholder_of};

/// The folder of the manual's notes, inside `notes/`.
pub const FOLDER: &str = "schnipsle-manual";

/// The tag every manual note carries.
pub const TAG: &str = "schnipsle_manual";

/// The `id` of the book note "Schnipsle manual", which lists the other notes
/// in reading order and which Help › Manual opens (ADR-0048 rule 7).
pub const BOOK_ID: &str = "01M8EAJZW0P1F5XGY1TW9WH7P8";

/// The book note's file name in [`FOLDER`].
pub const BOOK_FILE: &str = "schnipsle-manual.md";

/// The folder of attachments, below the vault root (format.md §6).
const ATTACHMENTS_DIR: &str = "attachments";

/// A note of the manual, as the app carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManualNote {
    /// File name in [`FOLDER`], e.g. `music.md`.
    pub name: String,
    /// The `id` of its front matter.
    pub id: String,
    /// The whole file, front matter included.
    pub text: String,
}

/// The manual's notes and pictures.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Manual {
    /// The notes, by file name.
    pub notes: Vec<ManualNote>,
    /// The pictures, as paths below the vault root (`attachments/3f/3f9a….png`)
    /// with their bytes.
    pub attachments: Vec<(String, Vec<u8>)>,
}

/// What [`Vault::add_manual`] did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ManualAdded {
    /// The notes added, as paths relative to `notes/`.
    pub added: Vec<String>,
    /// How many manual notes were there already, or in the trash.
    pub kept: usize,
}

impl Manual {
    /// The manual from its files, given as paths relative to the manual
    /// vault's root (`notes/schnipsle-manual/music.md`,
    /// `attachments/3f/3f9a….png`). Other files, such as a `README.md` or
    /// `.notesapp/`, are left out.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] for a note without a valid `id`, or two
    /// notes with the same `id`.
    pub fn from_files<'a>(files: impl IntoIterator<Item = (&'a str, &'a [u8])>) -> Result<Self> {
        let notes_prefix = format!("{NOTES_DIR}/{FOLDER}/");
        let attachments_prefix = format!("{ATTACHMENTS_DIR}/");
        let mut manual = Self::default();
        for (path, bytes) in files {
            let path = path.replace('\\', "/");
            if let Some(name) = path.strip_prefix(&notes_prefix) {
                if name.contains('/')
                    || name.starts_with('.')
                    || !name.to_ascii_lowercase().ends_with(".md")
                {
                    continue;
                }
                let text = String::from_utf8_lossy(bytes).into_owned();
                let id = front_matter::get(&text, "id")
                    .filter(|id| is_ulid(id))
                    .ok_or_else(|| VaultError::InvalidPath(format!("{path}: no valid id")))?;
                if manual.notes.iter().any(|n| n.id == id) {
                    return Err(VaultError::InvalidPath(format!("{path}: id {id} twice")));
                }
                manual.notes.push(ManualNote {
                    name: name.to_owned(),
                    id,
                    text,
                });
            } else if let Some(rest) = path.strip_prefix(&attachments_prefix)
                && !rest.split('/').any(|part| part.starts_with('.'))
            {
                manual.attachments.push((path.clone(), bytes.to_vec()));
            }
        }
        manual.notes.sort_by(|a, b| a.name.cmp(&b.name));
        manual.attachments.sort();
        Ok(manual)
    }

    /// The manual in the folder `root`, a copy of `docs/manual/`.
    ///
    /// # Errors
    /// I/O errors, and those of [`Manual::from_files`].
    pub fn load(root: &Path) -> Result<Self> {
        let mut files = Vec::new();
        let notes = root.join(NOTES_DIR).join(FOLDER);
        for entry in fs::read_dir(&notes)? {
            let entry = entry?;
            if entry.file_type()?.is_file() {
                let name = entry.file_name().to_string_lossy().into_owned();
                files.push((
                    format!("{NOTES_DIR}/{FOLDER}/{name}"),
                    fs::read(entry.path())?,
                ));
            }
        }
        let attachments = root.join(ATTACHMENTS_DIR);
        if attachments.is_dir() {
            for dir in fs::read_dir(&attachments)? {
                let dir = dir?;
                if !dir.file_type()?.is_dir() {
                    continue;
                }
                let dir_name = dir.file_name().to_string_lossy().into_owned();
                for file in fs::read_dir(dir.path())? {
                    let file = file?;
                    if file.file_type()?.is_file() {
                        let name = file.file_name().to_string_lossy().into_owned();
                        files.push((
                            format!("{ATTACHMENTS_DIR}/{dir_name}/{name}"),
                            fs::read(file.path())?,
                        ));
                    }
                }
            }
        }
        Self::from_files(files.iter().map(|(p, b)| (p.as_str(), b.as_slice())))
    }
}

/// Where the book note is when nobody moved it, relative to `notes/`.
#[must_use]
pub fn book_path() -> String {
    format!("{FOLDER}/{BOOK_FILE}")
}

impl Vault {
    /// Adds the manual notes that are missing, and the pictures they need.
    /// `has_id` tells whether a note in the vault has an `id` (the search
    /// index knows); the trash and the manual's own folder are checked here.
    /// A manual note's file is written exactly as the app carries it.
    ///
    /// # Errors
    /// [`VaultError::ReadOnly`], and I/O errors while writing.
    pub fn add_manual(
        &self,
        manual: &Manual,
        has_id: impl Fn(&str) -> bool,
    ) -> Result<ManualAdded> {
        self.check_writable()?;
        for (path, bytes) in &manual.attachments {
            let target = self.full_path(path);
            if target.exists() || placeholder_of(&target).exists() {
                continue;
            }
            if let Some(dir) = target.parent() {
                fs::create_dir_all(dir)?;
            }
            self.place_new_file(&target, bytes)?;
        }
        let mut done = ManualAdded::default();
        for note in &manual.notes {
            let path = format!("{FOLDER}/{}", note.name);
            let file = self.notes_dir().join(FOLDER).join(&note.name);
            let there = file.exists()
                || placeholder_of(&file).exists()
                || self.find_trashed(&note.id).is_some()
                || has_id(&note.id);
            if there {
                done.kept += 1;
                continue;
            }
            match self.create_note_as_is(&path, &note.text) {
                Ok(_) => done.added.push(path),
                Err(VaultError::AlreadyExists(_)) => done.kept += 1,
                Err(e) => return Err(e),
            }
        }
        Ok(done)
    }

    /// The path (relative to `notes/`) of the manual's book note: where the
    /// manual put it, or wherever the id lookup finds it. Also a book note
    /// that is not downloaded yet at its place. `None` if the vault has none.
    #[must_use]
    pub fn manual_book(&self) -> Option<String> {
        let path = book_path();
        self.find_note(BOOK_ID, Some(&path)).or_else(|| {
            let file = self.notes_dir().join(FOLDER).join(BOOK_FILE);
            let unread = self.read_note_text(&path).is_err();
            (unread && (file.exists() || placeholder_of(&file).exists())).then_some(path)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::{BTreeMap, BTreeSet};
    use std::path::PathBuf;

    use comrak::nodes::NodeValue;
    use comrak::{Arena, parse_document};

    use crate::tags::{extract_tags, parse_options};
    use crate::vault::title_of;

    fn repository_manual() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/manual")
    }

    fn shipped() -> Manual {
        Manual::load(&repository_manual()).unwrap()
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    /// The destinations of the links in `text` that are not web addresses.
    fn relative_links(text: &str) -> Vec<String> {
        let arena = Arena::new();
        let root = parse_document(&arena, text, &parse_options());
        root.descendants()
            .filter_map(|node| match &node.data.borrow().value {
                NodeValue::Link(link) if !link.url.contains(':') => Some(link.url.clone()),
                _ => None,
            })
            .collect()
    }

    /// The manual note a relative link points to: its file name.
    fn linked_name(link: &str) -> &str {
        link.split('#').next().unwrap_or_default()
    }

    #[test]
    fn every_manual_note_follows_the_rules() {
        let manual = shipped();
        assert!(manual.notes.len() >= 5);
        let names: BTreeSet<&str> = manual.notes.iter().map(|n| n.name.as_str()).collect();
        for note in &manual.notes {
            let name = &note.name;
            for key in ["created", "modified"] {
                assert!(
                    front_matter::get(&note.text, key).is_some_and(|v| v.len() >= 20),
                    "{name}: {key}"
                );
            }
            let tags: Vec<String> = extract_tags(&note.text)
                .into_iter()
                .map(|t| t.key)
                .collect();
            assert!(tags.iter().any(|t| t == TAG), "{name}: #{TAG}");
            for tag in &tags {
                let book = note.id == BOOK_ID && tag == "book";
                assert!(
                    book || tag == TAG || tag.starts_with(&format!("{TAG}/")),
                    "{name}: #{tag} is not under #{TAG}"
                );
            }
            let body = front_matter::body(&note.text).trim_end();
            let last = body.lines().last().unwrap_or_default();
            assert!(
                last.starts_with(&format!("#{TAG}")),
                "{name}: final tag line"
            );
            for link in relative_links(&note.text) {
                assert!(
                    names.contains(linked_name(&link)) || link.starts_with("../../attachments/"),
                    "{name}: link {link} goes nowhere"
                );
            }
        }
    }

    #[test]
    fn the_book_lists_every_note_and_the_readme_follows_it() {
        let manual = shipped();
        let book = manual.notes.iter().find(|n| n.id == BOOK_ID).unwrap();
        assert_eq!(book.name, BOOK_FILE);
        assert_eq!(title_of(&book.text, "x"), "Schnipsle manual");
        let mut order: Vec<String> = Vec::new();
        for link in relative_links(&book.text) {
            let name = linked_name(&link).to_owned();
            if !order.contains(&name) {
                order.push(name);
            }
        }
        let others: BTreeSet<String> = manual
            .notes
            .iter()
            .filter(|n| n.id != BOOK_ID)
            .map(|n| n.name.clone())
            .collect();
        assert_eq!(
            order.iter().cloned().collect::<BTreeSet<_>>(),
            others,
            "the book links to every other note"
        );

        let readme = fs::read_to_string(repository_manual().join("README.md")).unwrap();
        let prefix = format!("{NOTES_DIR}/{FOLDER}/");
        let listed: Vec<String> = relative_links(&readme)
            .iter()
            .filter_map(|l| l.strip_prefix(&prefix).map(str::to_owned))
            .filter(|l| !l.is_empty())
            .collect();
        let mut expected = vec![BOOK_FILE.to_owned()];
        expected.extend(order);
        assert_eq!(listed, expected, "README.md lists the notes in book order");

        // Titles in the README match the notes.
        let titles: BTreeMap<&str, String> = manual
            .notes
            .iter()
            .map(|n| (n.name.as_str(), title_of(&n.text, &n.name)))
            .collect();
        for name in &listed {
            let title = &titles[name.as_str()];
            assert!(
                readme.contains(&format!("[{title}]({prefix}{name})")),
                "{name}"
            );
        }
    }

    #[test]
    fn a_new_vault_gets_the_whole_manual_once() {
        let (_dir, vault) = vault();
        let manual = shipped();
        let done = vault.add_manual(&manual, |_| false).unwrap();
        assert_eq!(done.added.len(), manual.notes.len());
        assert_eq!(done.kept, 0);
        for note in &manual.notes {
            let read = vault
                .read_note_text(&format!("{FOLDER}/{}", note.name))
                .unwrap();
            assert_eq!(read.text, note.text, "written as carried");
        }
        assert_eq!(vault.manual_book(), Some(book_path()));
        let again = vault.add_manual(&manual, |_| false).unwrap();
        assert!(again.added.is_empty());
        assert_eq!(again.kept, manual.notes.len());
    }

    fn small_manual() -> Manual {
        let note = |id: &str, title: &str| {
            format!(
                "---\nid: {id}\ncreated: 2026-11-27T12:00:00+01:00\nmodified: 2026-11-27T12:00:00+01:00\n---\n\n# {title}\n\n![Picture](../../attachments/ab/abcd.png)\n\n#{TAG}\n"
            )
        };
        let book = note(BOOK_ID, "Schnipsle manual");
        let tags = note("01M8EAJZW0YHHR4A1F3AJBD94V", "Tags");
        let edited = note("01M8EAJZW075NMA896ANR8RF8C", "Edited");
        let files: Vec<(String, Vec<u8>)> = vec![
            (format!("notes/{FOLDER}/{BOOK_FILE}"), book.into_bytes()),
            (format!("notes/{FOLDER}/tags.md"), tags.into_bytes()),
            (format!("notes/{FOLDER}/edited.md"), edited.into_bytes()),
            (
                "attachments/ab/abcd.png".into(),
                b"invented picture".to_vec(),
            ),
            ("README.md".into(), b"# Not a note".to_vec()),
            (".notesapp/vault.yaml".into(), b"format: 1".to_vec()),
        ];
        Manual::from_files(files.iter().map(|(p, b)| (p.as_str(), b.as_slice()))).unwrap()
    }

    #[test]
    fn only_notes_and_pictures_are_carried() {
        let manual = small_manual();
        let names: Vec<&str> = manual.notes.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, ["edited.md", "schnipsle-manual.md", "tags.md"]);
        assert_eq!(manual.attachments.len(), 1);
    }

    #[test]
    fn a_note_without_an_id_is_refused() {
        let files = [(
            "notes/schnipsle-manual/a.md",
            b"# No front matter\n".as_slice(),
        )];
        assert!(matches!(
            Manual::from_files(files),
            Err(VaultError::InvalidPath(_))
        ));
    }

    #[test]
    fn moved_changed_and_deleted_notes_are_not_added_again() {
        let (dir, vault) = vault();
        let manual = small_manual();
        vault.add_manual(&manual, |_| false).unwrap();
        // The user moves the book, changes "Edited" and deletes "Tags".
        let book = vault.notes_dir().join(FOLDER).join(BOOK_FILE);
        let moved = vault.notes_dir().join("Start.md");
        fs::rename(&book, &moved).unwrap();
        let edited = format!("{FOLDER}/edited.md");
        let read = vault.read_note_text(&edited).unwrap();
        vault
            .write_note(&edited, &format!("{}More.\n", read.text), &read.hash)
            .unwrap();
        vault.trash_note(&format!("{FOLDER}/tags.md")).unwrap();

        let done = vault.add_manual(&manual, |id| id == BOOK_ID).unwrap();
        assert_eq!(
            done,
            ManualAdded {
                added: vec![],
                kept: 3
            }
        );
        assert!(!book.exists());
        assert!(
            vault
                .read_note_text(&edited)
                .unwrap()
                .text
                .ends_with("More.\n")
        );
        assert!(!vault.notes_dir().join(FOLDER).join("tags.md").exists());
        // The picture is stored once.
        let picture = dir.path().join("attachments/ab/abcd.png");
        assert_eq!(fs::read(&picture).unwrap(), b"invented picture");
    }

    #[test]
    fn a_missing_note_is_added_beside_the_others() {
        let (_dir, vault) = vault();
        let manual = small_manual();
        vault.add_manual(&manual, |_| false).unwrap();
        fs::remove_file(vault.notes_dir().join(FOLDER).join("tags.md")).unwrap();
        let done = vault.add_manual(&manual, |_| false).unwrap();
        assert_eq!(done.added, [format!("{FOLDER}/tags.md")]);
        assert_eq!(done.kept, 2);
    }

    #[test]
    fn the_book_is_found_where_it_was_moved() {
        let (_dir, vault) = vault();
        assert_eq!(vault.manual_book(), None);
        vault.add_manual(&small_manual(), |_| false).unwrap();
        fs::rename(
            vault.notes_dir().join(FOLDER).join(BOOK_FILE),
            vault.notes_dir().join("Start.md"),
        )
        .unwrap();
        // Without the index, a moved book is not found.
        assert_eq!(vault.manual_book(), None);
        // A placeholder at its place counts.
        fs::write(
            vault
                .notes_dir()
                .join(FOLDER)
                .join(".schnipsle-manual.md.icloud"),
            "",
        )
        .unwrap();
        assert_eq!(vault.manual_book(), Some(book_path()));
    }

    #[test]
    fn a_read_only_vault_gets_nothing() {
        let dir = tempfile::tempdir().unwrap();
        Vault::create(dir.path()).unwrap();
        fs::write(
            dir.path().join(".notesapp/vault.yaml"),
            "format: 99\nid: 01M8EAJZW04V621V80ZVE3XFTB\n",
        )
        .unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        assert!(matches!(
            vault.add_manual(&small_manual(), |_| false),
            Err(VaultError::ReadOnly(_))
        ));
    }
}
