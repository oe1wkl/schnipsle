// SPDX-License-Identifier: Apache-2.0
//! The vault layout (format.md §1) and the vault's identity, `vault.yaml` (§2).
//!
//! A vault is a folder with `.notesapp/vault.yaml`; its notes live in `notes/`.
//! [`inspect`] tells what a folder the user picked is. [`Vault::create`] makes a
//! vault of a folder that has none; the app asks the user first, since this
//! moves everything visible in the folder into `notes/` (notes, subfolders and
//! other files alike, so relative links between them keep working). The moves
//! come first and `vault.yaml` last: an interrupted run leaves no vault behind,
//! so the folder is simply offered again and the rest is moved then. Nothing is
//! ever replaced, and hidden files such as `.notesapp/` stay where they are.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::Serialize;

use crate::coordination::{Coordinator, Uncoordinated};
use crate::flat_yaml;
use crate::stamp::{is_ulid, new_ulid, now_ms, rfc3339_utc};
use crate::vault::{
    Access, Result, TempFile, Vault, VaultError, accessor_ran_twice, count_notes, placeholder_of,
};

/// The vault's identity file, relative to the vault root (format.md §2).
pub const VAULT_FILE: &str = ".notesapp/vault.yaml";

/// The folder holding the notes, relative to the vault root (format.md §1).
pub const NOTES_DIR: &str = "notes";

/// The highest `format` in `vault.yaml` this version reads and writes.
pub const FORMAT: u32 = 1;

/// The content of `vault.yaml` (format.md §2).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct VaultIdentity {
    /// Format version; a vault with a higher one is opened read-only.
    pub format: u32,
    /// ULID, never changes.
    pub id: String,
    /// Display name, if set.
    pub name: Option<String>,
    /// When the vault was created, RFC 3339, if recorded.
    pub created: Option<String>,
}

impl VaultIdentity {
    /// Reads `vault.yaml`; the error says what is wrong with it.
    ///
    /// # Errors
    /// A message if `format` is not a positive integer or `id` is not a ULID.
    pub fn parse(text: &str) -> std::result::Result<Self, String> {
        let format = flat_yaml::value(text, "format")
            .ok_or("it has no format")?
            .parse::<u32>()
            .ok()
            .filter(|&f| f >= 1)
            .ok_or("its format is not a positive whole number")?;
        let id = flat_yaml::value(text, "id")
            .filter(|id| is_ulid(id))
            .ok_or("it has no valid id")?;
        let optional = |key| flat_yaml::value(text, key).filter(|v| !v.is_empty());
        Ok(Self {
            format,
            id,
            name: optional("name"),
            created: optional("created"),
        })
    }

    /// A new identity: current format, new id, created now.
    fn new() -> Result<Self> {
        Ok(Self {
            format: FORMAT,
            id: new_ulid()?,
            name: None,
            created: Some(rfc3339_utc(now_ms())),
        })
    }

    fn to_yaml(&self) -> String {
        let mut yaml = format!("format: {}\nid: {}\n", self.format, self.id);
        if let Some(name) = &self.name {
            let _ = writeln!(yaml, "name: {}", flat_yaml::quote(name));
        }
        if let Some(created) = &self.created {
            let _ = writeln!(yaml, "created: {created}");
        }
        yaml
    }
}

/// What a folder is, see [`inspect`]. Serialized with a camelCase `kind` tag.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum Folder {
    /// A vault: open it with [`Vault::open`].
    Vault,
    /// A vault whose `vault.yaml` iCloud has not downloaded yet.
    VaultNotDownloaded,
    /// A folder inside the vault at `root`, which should be opened instead.
    InsideVault {
        /// The vault's root folder.
        root: PathBuf,
    },
    /// No vault yet. [`Vault::create`] would move `items` into `notes/`.
    NoVault {
        /// Names of the visible files and folders at the top level that would
        /// move, sorted; empty for an empty folder.
        items: Vec<String>,
        /// Notes (`.md` files) in the folder, at any depth.
        notes: usize,
        /// Items at the top level iCloud has not downloaded yet; they block
        /// [`Vault::create`] until they are downloaded.
        not_downloaded: Vec<String>,
    },
}

/// Tells what `root` is without changing anything.
///
/// # Errors
/// [`VaultError::NotADirectory`], or I/O errors reading the folder.
pub fn inspect(root: &Path, coordinator: Arc<dyn Coordinator>) -> Result<Folder> {
    Vault::unopened(root.to_owned(), coordinator)?.inspect()
}

impl Vault {
    /// Makes `root` a vault without file coordination, see [`Vault::create_with`].
    ///
    /// # Errors
    /// As for [`Vault::create_with`].
    pub fn create(root: impl Into<PathBuf>) -> Result<Self> {
        Self::create_with(root, Arc::new(Uncoordinated))
    }

    /// Makes `root` a vault (format.md §1): moves every visible item at its top
    /// level into `notes/`, then writes `.notesapp/vault.yaml`. Opens the vault
    /// that is already there if `root` is one. Call it only with the user's
    /// consent if [`inspect`] listed items to move.
    ///
    /// # Errors
    /// [`VaultError::InsideVault`] for a folder inside a vault;
    /// [`VaultError::NotDownloaded`] if an item is only in iCloud (nothing is
    /// moved then); [`VaultError::AlreadyExists`] if `notes/` already holds an
    /// item of the same name (nothing is moved then); I/O errors, after which
    /// the items not yet moved are where they were.
    pub fn create_with(
        root: impl Into<PathBuf>,
        coordinator: Arc<dyn Coordinator>,
    ) -> Result<Self> {
        let root = root.into();
        let bare = Self::unopened(root.clone(), coordinator.clone())?;
        let items = match bare.inspect()? {
            Folder::Vault => return Self::open_with(root, coordinator),
            Folder::VaultNotDownloaded => return Err(VaultError::NotDownloaded(VAULT_FILE.into())),
            Folder::InsideVault { root } => return Err(VaultError::InsideVault(root)),
            Folder::NoVault {
                items,
                not_downloaded,
                ..
            } => match not_downloaded.into_iter().next() {
                Some(name) => return Err(VaultError::NotDownloaded(name)),
                None => items,
            },
        };
        let notes = bare.notes_dir().to_owned();
        if fs::symlink_metadata(&notes).is_ok_and(|m| !m.is_dir()) {
            return Err(VaultError::AlreadyExists(NOTES_DIR.into()));
        }
        // Check every target first, so a clash moves nothing.
        for item in &items {
            let target = notes.join(item);
            if fs::symlink_metadata(&target).is_ok() || placeholder_of(&target).exists() {
                return Err(VaultError::AlreadyExists(format!("{NOTES_DIR}/{item}")));
            }
        }
        fs::create_dir_all(&notes)?;
        for item in &items {
            bare.move_new(&root.join(item), &notes.join(item))?;
        }

        let file = root.join(VAULT_FILE);
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut tmp = Some(TempFile::write(
            &file,
            VaultIdentity::new()?.to_yaml().as_bytes(),
        )?);
        let placed = bare.coordinated(Access::Write, &file, |file| {
            Ok(tmp.take().ok_or_else(accessor_ran_twice)?.place_new(file)?)
        });
        match placed {
            // Made elsewhere meanwhile, e.g. on another device: that one counts.
            Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => {}
            other => other?,
        }
        Self::open_with(root, coordinator)
    }

    fn inspect(&self) -> Result<Folder> {
        let file = self.root().join(VAULT_FILE);
        if fs::symlink_metadata(&file).is_ok() {
            return Ok(Folder::Vault);
        }
        if placeholder_of(&file).exists() {
            return Ok(Folder::VaultNotDownloaded);
        }
        if let Some(root) = self
            .root()
            .ancestors()
            .skip(1)
            .find(|dir| dir.join(VAULT_FILE).is_file())
        {
            return Ok(Folder::InsideVault {
                root: root.to_owned(),
            });
        }
        let notes_dir = self.notes_dir().is_dir();
        self.coordinated(Access::Read, self.root(), |root| {
            let mut items = Vec::new();
            let mut not_downloaded = Vec::new();
            for entry in fs::read_dir(root)?.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if let Some(real) = name
                    .strip_prefix('.')
                    .and_then(|n| n.strip_suffix(".icloud"))
                {
                    not_downloaded.push(real.to_owned());
                } else if !(name.starts_with('.') || notes_dir && name == NOTES_DIR) {
                    items.push(name);
                }
            }
            items.sort();
            not_downloaded.sort();
            Ok(Folder::NoVault {
                items,
                notes: count_notes(root)?,
                not_downloaded,
            })
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{PathKind, Uncoordinated};

    const ID: &str = "01JA7Q5Y0Z8H3K2M6N4P9R1S2T";

    fn uncoordinated() -> Arc<dyn Coordinator> {
        Arc::new(Uncoordinated)
    }

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    #[test]
    fn parses_vault_yaml() {
        let identity = VaultIdentity::parse(&format!(
            "format: 1                     # integer, REQUIRED\nid: {ID}  # ULID\n\
             name: My Notes                # display name\ncreated: 2026-10-13T21:10:00+02:00\n\
             future_key: kept\n"
        ))
        .unwrap();
        assert_eq!(
            identity,
            VaultIdentity {
                format: 1,
                id: ID.into(),
                name: Some("My Notes".into()),
                created: Some("2026-10-13T21:10:00+02:00".into()),
            }
        );
        let minimal = VaultIdentity::parse(&format!("format: 7\nid: {ID}\n")).unwrap();
        assert_eq!(
            (minimal.format, minimal.name, minimal.created),
            (7, None, None)
        );

        for damaged in [
            String::new(),
            format!("id: {ID}\n"),
            format!("format: one\nid: {ID}\n"),
            format!("format: 0\nid: {ID}\n"),
            "format: 1\n".to_owned(),
            "format: 1\nid: not-a-ulid\n".to_owned(),
        ] {
            assert!(VaultIdentity::parse(&damaged).is_err(), "{damaged:?}");
        }
    }

    #[test]
    fn written_identity_reads_back() {
        let mut identity = VaultIdentity::new().unwrap();
        identity.name = Some("Willi's \"notes\"".into());
        assert_eq!(VaultIdentity::parse(&identity.to_yaml()).unwrap(), identity);
    }

    #[test]
    fn an_empty_folder_becomes_a_vault() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join(".DS_Store"), "x").unwrap();
        assert_eq!(
            inspect(dir.path(), uncoordinated()).unwrap(),
            Folder::NoVault {
                items: vec![],
                notes: 0,
                not_downloaded: vec![]
            }
        );

        let vault = Vault::create(dir.path()).unwrap();
        assert!(dir.path().join("notes").is_dir());
        assert!(dir.path().join(".DS_Store").exists());
        let identity = vault.identity().unwrap();
        assert_eq!(identity.format, FORMAT);
        assert!(is_ulid(&identity.id));
        assert!(identity.created.is_some());
        assert_eq!(vault.read_only(), None);
        assert_eq!(inspect(dir.path(), uncoordinated()).unwrap(), Folder::Vault);

        // Creating again opens the same vault and changes nothing.
        let again = Vault::create(dir.path()).unwrap();
        assert_eq!(again.identity(), vault.identity());
    }

    #[test]
    fn a_folder_of_notes_moves_into_notes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("Einkaufsliste.md"), "# Einkauf\n");
        write(
            &root.join("Projekte/Antenne.md"),
            "# Antenne\n![](bild.png)\n",
        );
        write(&root.join("Projekte/bild.png"), "png");
        write(&root.join("README.txt"), "other file");
        write(&root.join(".notesapp/history/x/1.md.zst"), "kept");
        write(&root.join(".DS_Store"), "kept");

        assert_eq!(
            inspect(root, uncoordinated()).unwrap(),
            Folder::NoVault {
                items: vec![
                    "Einkaufsliste.md".into(),
                    "Projekte".into(),
                    "README.txt".into()
                ],
                notes: 2,
                not_downloaded: vec![]
            }
        );
        // Nothing is touched by looking.
        assert!(matches!(Vault::open(root), Err(VaultError::NotAVault(_))));
        assert!(!root.join("notes").exists());

        let vault = Vault::create(root).unwrap();
        for moved in [
            "notes/Einkaufsliste.md",
            "notes/Projekte/Antenne.md",
            "notes/Projekte/bild.png",
            "notes/README.txt",
            ".notesapp/history/x/1.md.zst",
            ".notesapp/vault.yaml",
            ".DS_Store",
        ] {
            assert!(root.join(moved).exists(), "{moved}");
        }
        for gone in ["Einkaufsliste.md", "Projekte", "README.txt"] {
            assert!(!root.join(gone).exists(), "{gone}");
        }
        assert_eq!(
            fs::read_to_string(root.join("notes/Projekte/Antenne.md")).unwrap(),
            "# Antenne\n![](bild.png)\n"
        );
        let paths: Vec<_> = vault
            .list_notes()
            .unwrap()
            .into_iter()
            .map(|n| n.path)
            .collect();
        assert_eq!(paths.len(), 2);
        assert!(paths.contains(&"Projekte/Antenne.md".to_owned()));
    }

    #[test]
    fn an_interrupted_move_is_finished_later() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("notes/a.md"), "moved before the interruption");
        write(&root.join("b.md"), "not moved yet");
        assert_eq!(
            inspect(root, uncoordinated()).unwrap(),
            Folder::NoVault {
                items: vec!["b.md".into()],
                notes: 2,
                not_downloaded: vec![]
            }
        );
        let vault = Vault::create(root).unwrap();
        assert_eq!(vault.list_notes().unwrap().len(), 2);
        assert!(root.join("notes/b.md").exists());
    }

    #[test]
    fn a_name_taken_in_notes_moves_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join("notes/a.md"), "inside");
        write(&root.join("a.md"), "outside");
        write(&root.join("b.md"), "outside too");
        match Vault::create(root) {
            Err(VaultError::AlreadyExists(name)) => assert_eq!(name, "notes/a.md"),
            other => panic!("{other:?}"),
        }
        assert_eq!(
            fs::read_to_string(root.join("notes/a.md")).unwrap(),
            "inside"
        );
        assert!(root.join("a.md").exists() && root.join("b.md").exists());
        assert!(!root.join(VAULT_FILE).exists());
    }

    #[test]
    fn a_file_named_notes_blocks_creation() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join("notes"), "a file");
        assert!(matches!(
            Vault::create(dir.path()),
            Err(VaultError::AlreadyExists(_))
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join("notes")).unwrap(),
            "a file"
        );
    }

    #[test]
    fn items_only_in_icloud_block_creation() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join(".Fern.md.icloud"), "plist");
        write(&root.join("Nah.md"), "here");
        assert_eq!(
            inspect(root, uncoordinated()).unwrap(),
            Folder::NoVault {
                items: vec!["Nah.md".into()],
                notes: 2,
                not_downloaded: vec!["Fern.md".into()]
            }
        );
        match Vault::create(root) {
            Err(VaultError::NotDownloaded(name)) => assert_eq!(name, "Fern.md"),
            other => panic!("{other:?}"),
        }
        assert!(root.join("Nah.md").exists());
        assert!(!root.join("notes").exists());
    }

    #[test]
    fn vault_yaml_only_in_icloud_is_not_a_missing_vault() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join(".notesapp/.vault.yaml.icloud"), "plist");
        write(&root.join("notes/a.md"), "a");
        assert_eq!(
            inspect(root, uncoordinated()).unwrap(),
            Folder::VaultNotDownloaded
        );
        assert!(matches!(
            Vault::open(root),
            Err(VaultError::NotDownloaded(p)) if p == VAULT_FILE
        ));
        // Never a second identity next to the one still in iCloud.
        assert!(matches!(
            Vault::create(root),
            Err(VaultError::NotDownloaded(_))
        ));
        assert!(!root.join(VAULT_FILE).exists());
    }

    #[test]
    fn a_folder_inside_a_vault_is_not_made_a_vault() {
        let dir = tempfile::tempdir().unwrap();
        Vault::create(dir.path()).unwrap();
        let inner = dir.path().join("notes/Projekte");
        write(&inner.join("a.md"), "a");
        assert_eq!(
            inspect(&inner, uncoordinated()).unwrap(),
            Folder::InsideVault {
                root: dir.path().to_owned()
            }
        );
        assert!(matches!(
            Vault::create(&inner),
            Err(VaultError::InsideVault(_))
        ));
        assert!(!inner.join("notes").exists());
    }

    #[test]
    fn a_newer_format_opens_read_only() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(&root.join(VAULT_FILE), &format!("format: 2\nid: {ID}\n"));
        write(&root.join("notes/old.md"), "# A\n");
        let vault = Vault::open(root).unwrap();
        assert_eq!(vault.identity().unwrap().format, 2);
        assert!(vault.read_only().unwrap().contains("format 2"));

        // Reading works …
        assert_eq!(vault.list_notes().unwrap().len(), 1);
        let note = vault.read_note("old.md").unwrap();
        // … writing does not, and leaves nothing behind.
        let refused = |r: Result<()>| assert!(matches!(r, Err(VaultError::ReadOnly(_))), "{r:?}");
        refused(vault.write_note("old.md", "# B\n", &note.hash).map(drop));
        refused(vault.create_note("b.md", "# B\n").map(drop));
        refused(vault.create_titled("", "# C\n").map(drop));
        refused(vault.rename_after_edit("old.md", "Old title").map(drop));
        refused(vault.trash_note("old.md").map(drop));
        refused(vault.restore_note(ID).map(drop));
        refused(vault.keep_both("old.md", "copy.md").map(drop));
        refused(vault.snapshot(ID, "x").map(drop));
        refused(
            vault
                .resolve_conflict("old.md", "# B\n", &note.hash)
                .map(drop),
        );
        let mut names: Vec<_> = walk(root);
        names.sort();
        assert_eq!(
            names,
            [".notesapp", ".notesapp/vault.yaml", "notes", "notes/old.md"]
        );
        assert_eq!(
            fs::read_to_string(root.join("notes/old.md")).unwrap(),
            "# A\n"
        );
    }

    #[test]
    fn a_damaged_vault_yaml_opens_read_only() {
        let dir = tempfile::tempdir().unwrap();
        write(&dir.path().join(VAULT_FILE), "format: 1\nid: oops\n");
        let vault = Vault::open(dir.path()).unwrap();
        assert_eq!(vault.identity(), None);
        assert!(vault.read_only().unwrap().contains("damaged"));
        assert!(matches!(
            vault.create_note("a.md", "x"),
            Err(VaultError::ReadOnly(_))
        ));
    }

    #[test]
    fn only_notes_inside_notes_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        let root = dir.path();
        assert_eq!(
            vault.classify(&root.join("notes/Projekte/a.md")),
            PathKind::Note("Projekte/a.md".into())
        );
        assert_eq!(
            vault.classify(&root.join("notes/.b.md.icloud")),
            PathKind::Note("b.md".into())
        );
        assert_eq!(vault.classify(&root.join("notes")), PathKind::Folder);
        assert_eq!(vault.classify(root), PathKind::Folder);
        for ignored in [
            "a.md",
            "attachments/x.md",
            ".notesapp/trash/x.md",
            "notesx/a.md",
        ] {
            assert_eq!(
                vault.classify(&root.join(ignored)),
                PathKind::Ignored,
                "{ignored}"
            );
        }
    }

    fn walk(root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        let mut stack = vec![root.to_owned()];
        while let Some(dir) = stack.pop() {
            for entry in fs::read_dir(dir).unwrap().flatten() {
                let path = entry.path();
                out.push(
                    path.strip_prefix(root)
                        .unwrap()
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
                if path.is_dir() {
                    stack.push(path);
                }
            }
        }
        out
    }
}
