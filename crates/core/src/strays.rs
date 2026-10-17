// SPDX-License-Identifier: Apache-2.0
//! Notes outside `notes/` (format.md §1).
//!
//! Only `notes/` holds notes, but a device still running a build from before
//! the vault layout writes new notes to the vault root, and other tools may put
//! some there too. [`Vault::stray_notes`] finds them: `.md` files at the root
//! and folders at the root that contain notes. Other files and folders there
//! are left alone, as are `notes/`, `attachments/` and hidden ones.
//! [`Vault::move_stray_notes`] moves them into `notes/` after the user agreed;
//! it never replaces anything: a note whose name is taken gets `-2`, `-3`, …
//! (format.md §3), a folder whose name is taken stays where it is.

use std::fs;
use std::io;

use serde::Serialize;

use crate::layout::{ATTACHMENTS_DIR, NOTES_DIR};
use crate::names::candidate;
use crate::vault::{
    Access, Result, Vault, VaultError, count_notes, has_md_extension, note_paths,
    placeholder_note_name,
};

const MAX_SUFFIX: u32 = 1000;

/// Notes outside `notes/`, see [`Vault::stray_notes`]. Serialized with
/// camelCase names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StrayNotes {
    /// `.md` files and folders with notes at the vault root, sorted.
    pub items: Vec<String>,
    /// Notes in `items`, at any depth.
    pub notes: usize,
    /// Notes at the vault root that iCloud has not downloaded yet; they cannot
    /// be moved until they are.
    pub not_downloaded: Vec<String>,
}

impl StrayNotes {
    /// `true` if every note is in `notes/`.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty() && self.not_downloaded.is_empty()
    }
}

/// What [`Vault::move_stray_notes`] did. Serialized with camelCase names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MovedNotes {
    /// Where the items went, relative to `notes/`.
    pub moved: Vec<String>,
    /// Items left at the root: folders whose name is taken in `notes/`, and
    /// notes only in iCloud.
    pub skipped: Vec<String>,
    /// Notes whose relative links were rewritten to keep working (ADR-0016),
    /// relative to `notes/`.
    pub relinked: Vec<String>,
}

impl Vault {
    /// Finds notes outside `notes/`; changes nothing.
    ///
    /// # Errors
    /// I/O errors reading the vault root; unreadable folders count as empty.
    pub fn stray_notes(&self) -> Result<StrayNotes> {
        self.coordinated(Access::Read, self.root(), |root| {
            let mut strays = StrayNotes::default();
            for entry in fs::read_dir(root)?.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                let Ok(file_type) = entry.file_type() else {
                    continue;
                };
                if let Some(real) = placeholder_note_name(&name) {
                    strays.not_downloaded.push(real.to_owned());
                } else if name.starts_with('.') || name == NOTES_DIR || name == ATTACHMENTS_DIR {
                    // hidden, or part of the vault layout
                } else if file_type.is_dir() {
                    let notes = count_notes(&entry.path()).unwrap_or(0);
                    if notes > 0 {
                        strays.items.push(name);
                        strays.notes += notes;
                    }
                } else if file_type.is_file() && has_md_extension(&name) {
                    strays.items.push(name);
                    strays.notes += 1;
                }
            }
            strays.items.sort();
            strays.not_downloaded.sort();
            Ok(strays)
        })
    }

    /// Moves the notes [`Vault::stray_notes`] finds into `notes/`, unchanged.
    /// Call it only with the user's consent.
    ///
    /// # Errors
    /// [`VaultError::ReadOnly`], or I/O errors; items moved before the error
    /// stay moved, the others stay where they were.
    pub fn move_stray_notes(&self) -> Result<MovedNotes> {
        self.check_writable()?;
        let strays = self.stray_notes()?;
        let mut result = MovedNotes {
            skipped: strays.not_downloaded,
            ..MovedNotes::default()
        };
        fs::create_dir_all(self.notes_dir())?;
        for item in strays.items {
            let from = self.root().join(&item);
            if from.is_dir() {
                match self.move_new(&from, &self.notes_dir().join(&item)) {
                    Ok(()) => {
                        // Files in the folder moved along; links leaving it need fixing.
                        let map = |p: &[String]| -> Vec<String> {
                            if p.first() == Some(&item) {
                                std::iter::once(NOTES_DIR.to_owned())
                                    .chain(p.iter().cloned())
                                    .collect()
                            } else {
                                p.to_vec()
                            }
                        };
                        for inner in note_paths(&self.notes_dir().join(&item)) {
                            let path = format!("{item}/{inner}");
                            let mut old_dir: Vec<String> =
                                path.split('/').map(str::to_owned).collect();
                            old_dir.pop();
                            self.relink(&path, &old_dir, &map, &mut result.relinked);
                        }
                        result.moved.push(item);
                    }
                    Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => {
                        result.skipped.push(item);
                    }
                    Err(e) => return Err(e),
                }
                continue;
            }
            let stem = &item[..item.len() - 3]; // `.md` in any case
            let mut placed = false;
            for n in 1..=MAX_SUFFIX {
                let target = candidate("", stem, n);
                match self.move_new(&from, &self.resolve(&target)?) {
                    Ok(()) => {
                        self.relink(&target, &[], &|p| p.to_vec(), &mut result.relinked);
                        result.moved.push(target);
                        placed = true;
                        break;
                    }
                    Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => {}
                    Err(e) => return Err(e),
                }
            }
            if !placed {
                result.skipped.push(item);
            }
        }
        Ok(result)
    }

    /// Rewrites the links of a note that moved in (ADR-0016); a failure only
    /// leaves its links as they were.
    fn relink(
        &self,
        path: &str,
        old_dir: &[String],
        map: &dyn Fn(&[String]) -> Vec<String>,
        relinked: &mut Vec<String>,
    ) {
        match self.relink_after_move(path, old_dir, map) {
            Ok(true) => relinked.push(path.to_owned()),
            Ok(false) => {}
            Err(e) => eprintln!("links in {path} not updated: {e}"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::PathKind;
    use crate::layout::VAULT_FILE;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn finds_only_notes_outside_notes() {
        let (dir, vault) = vault();
        let root = dir.path();
        assert!(vault.stray_notes().unwrap().is_empty());

        write(&root.join("notes/listed.md"), "listed");
        write(&root.join("attachments/ab/x.md"), "an attachment");
        write(&root.join(".notesapp/trash/x.md"), "trashed");
        write(&root.join("README.txt"), "not a note");
        write(&root.join("Bilder/a.png"), "folder without notes");
        write(&root.join(".hidden.md"), "hidden");
        write(&root.join("Neu von iPhone.md"), "# Neu\n");
        write(&root.join("Projekte/Alt.MD"), "old");
        write(&root.join("Projekte/tief/drin.md"), "deep");
        write(&root.join(".Wolke.md.icloud"), "plist");

        assert_eq!(
            vault.stray_notes().unwrap(),
            StrayNotes {
                items: vec!["Neu von iPhone.md".into(), "Projekte".into()],
                notes: 3,
                not_downloaded: vec!["Wolke.md".into()],
            }
        );
        assert_eq!(vault.list_notes().unwrap().len(), 1); // not listed until moved
    }

    #[test]
    fn moves_them_into_notes_without_replacing_anything() {
        let (dir, vault) = vault();
        let root = dir.path();
        write(&root.join("notes/Einkauf.md"), "in notes");
        write(&root.join("notes/Projekte/x.md"), "in notes");
        write(&root.join("Einkauf.md"), "from the iPhone");
        write(&root.join("Ideen.md"), "ideas");
        write(&root.join("Projekte/y.md"), "folder name taken");
        write(&root.join("Musik/Kanon.md"), "song");
        write(&root.join("Musik/kanon.abc"), "kept with it");
        write(&root.join(".Wolke.md.icloud"), "plist");
        write(&root.join("README.txt"), "stays");

        let moved = vault.move_stray_notes().unwrap();
        assert_eq!(moved.moved, ["Einkauf-2.md", "Ideen.md", "Musik"]);
        assert_eq!(moved.skipped, ["Wolke.md", "Projekte"]);

        let read = |p: &str| fs::read_to_string(root.join(p)).unwrap();
        assert_eq!(read("notes/Einkauf.md"), "in notes");
        assert_eq!(read("notes/Einkauf-2.md"), "from the iPhone");
        assert_eq!(read("notes/Ideen.md"), "ideas");
        assert_eq!(read("notes/Musik/kanon.abc"), "kept with it");
        assert_eq!(read("notes/Projekte/x.md"), "in notes");
        assert!(!root.join("notes/Projekte/y.md").exists());
        assert_eq!(read("Projekte/y.md"), "folder name taken");
        assert_eq!(read("README.txt"), "stays");
        assert!(root.join(".Wolke.md.icloud").exists());

        assert_eq!(
            vault.stray_notes().unwrap(),
            StrayNotes {
                items: vec!["Projekte".into()],
                notes: 1,
                not_downloaded: vec!["Wolke.md".into()],
            }
        );
        assert_eq!(vault.list_notes().unwrap().len(), 5);
    }

    #[test]
    fn a_read_only_vault_moves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            &root.join(VAULT_FILE),
            "format: 99\nid: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T\n",
        );
        write(&root.join("stray.md"), "x");
        let vault = Vault::open(root).unwrap();
        assert_eq!(vault.stray_notes().unwrap().items, ["stray.md"]);
        assert!(matches!(
            vault.move_stray_notes(),
            Err(VaultError::ReadOnly(_))
        ));
        assert!(root.join("stray.md").exists());
        assert!(!root.join("notes").exists());
    }

    #[test]
    fn changes_outside_notes_ask_for_a_rescan() {
        let (dir, vault) = vault();
        let root = dir.path();
        fs::create_dir(root.join("Neu")).unwrap();
        for rescan in ["a.md", ".a.md.icloud", "Neu", "Projekte/x.md", "gone"] {
            assert_eq!(
                vault.classify(&root.join(rescan)),
                PathKind::Folder,
                "{rescan}"
            );
        }
        for ignored in [
            "README.txt",
            ".DS_Store",
            ".a.md.tmp-1-0",
            "attachments/ab/x.md",
            ".notesapp/trash/x.md",
            "Bilder/a.png",
        ] {
            assert_eq!(
                vault.classify(&root.join(ignored)),
                PathKind::Ignored,
                "{ignored}"
            );
        }
    }

    #[test]
    fn links_keep_working_after_the_move() {
        let (dir, vault) = vault();
        let root = dir.path();
        write(&root.join("attachments/ab/x.png"), "png");
        write(&root.join("Neu.md"), "# Neu\n![x](attachments/ab/x.png)\n");
        write(
            &root.join("Mappe/Notiz.md"),
            "# Notiz\n![in](bild.png) ![out](../attachments/ab/x.png)\n",
        );
        write(&root.join("Mappe/bild.png"), "png");

        let moved = vault.move_stray_notes().unwrap();
        assert_eq!(moved.relinked, ["Mappe/Notiz.md", "Neu.md"]);
        let read = |p: &str| fs::read_to_string(root.join(p)).unwrap();
        assert!(read("notes/Neu.md").contains("![x](../attachments/ab/x.png)"));
        let notiz = read("notes/Mappe/Notiz.md");
        assert!(notiz.contains("![in](bild.png)"), "{notiz}");
        assert!(
            notiz.contains("![out](../../attachments/ab/x.png)"),
            "{notiz}"
        );
        // The notes had no id: they got one, and their old text is in the history.
        let id = crate::front_matter::get(&notiz, "id").unwrap();
        assert_eq!(vault.snapshots(&id).unwrap().len(), 1);
        for (note, link) in [
            ("Neu.md", "../attachments/ab/x.png"),
            ("Mappe/Notiz.md", "bild.png"),
        ] {
            assert!(vault.read_linked_file(note, link).is_ok(), "{note}: {link}");
        }
    }
}
