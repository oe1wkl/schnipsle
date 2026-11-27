// SPDX-License-Identifier: Apache-2.0
//! The default vault made or opened at the first start (ADR-0048 rule 3,
//! ADR-0050).
//!
//! In a parent folder (the app's iCloud folder, or `Documents` on the device)
//! the vault is a folder named "Schnipsle", or "Schnipsle 1", "Schnipsle 2",
//! … : the first that is not there, empty, or a vault. A vault there is
//! opened, also when its `vault.yaml` is not downloaded yet; a folder holding
//! anything else is never touched.
//!
//! What a folder holds comes from the disk ([`spot_on_disk`]); on the iPhone
//! the app adds what iCloud's query listed but the disk does not have yet
//! ([`Listed`]).

use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::coordination::Coordinator;
use crate::layout::{Folder, VAULT_FILE};
use crate::vault::{Result, Vault, VaultError};

/// The name of the default vault's folder.
pub const NAME: &str = "Schnipsle";

/// How many numbered names are tried after [`NAME`] itself.
const MAX_NUMBER: u32 = 99;

/// What a candidate folder for the default vault holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Spot {
    /// Not there: a vault is made in a new folder.
    Missing,
    /// There and empty, apart from hidden files: a vault is made in it.
    Empty,
    /// A vault, downloaded or not: it is opened.
    Vault,
    /// Something else, which is left alone.
    Taken,
}

/// The folder chosen for the default vault.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Choice {
    /// The folder's full path.
    pub path: PathBuf,
    /// What it holds: [`Spot::Missing`], [`Spot::Empty`] or [`Spot::Vault`].
    pub spot: Spot,
}

/// Folder names in the order they are tried: "Schnipsle", "Schnipsle 1", …
fn names() -> impl Iterator<Item = String> {
    std::iter::once(NAME.to_owned()).chain((1..=MAX_NUMBER).map(|n| format!("{NAME} {n}")))
}

/// What the folder at `path` holds, from the disk.
///
/// # Errors
/// I/O errors other than the folder not being there.
pub fn spot_on_disk(path: &Path, coordinator: Arc<dyn Coordinator>) -> Result<Spot> {
    match fs::symlink_metadata(path) {
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Spot::Missing),
        Err(e) => return Err(e.into()),
        Ok(meta) if !meta.is_dir() => return Ok(Spot::Taken),
        Ok(_) => {}
    }
    Ok(match crate::layout::inspect(path, coordinator)? {
        Folder::Vault | Folder::VaultNotDownloaded => Spot::Vault,
        Folder::NoVault {
            items,
            not_downloaded,
            ..
        } if items.is_empty() && not_downloaded.is_empty() => Spot::Empty,
        Folder::NoVault { .. } | Folder::InsideVault { .. } => Spot::Taken,
    })
}

/// Chooses the folder for the default vault in `parent`: the first of
/// "Schnipsle", "Schnipsle 1", … whose `spot` is not [`Spot::Taken`].
///
/// # Errors
/// What `spot` returns, or [`VaultError::AlreadyExists`] if every name is
/// taken.
pub fn choose(parent: &Path, mut spot: impl FnMut(&Path) -> Result<Spot>) -> Result<Choice> {
    for name in names() {
        let path = parent.join(&name);
        match spot(&path)? {
            Spot::Taken => {}
            spot => return Ok(Choice { path, spot }),
        }
    }
    Err(VaultError::AlreadyExists(format!("{NAME} {MAX_NUMBER}")))
}

/// What iCloud's query listed in the parent folder, for folders the disk may
/// not have yet: the top-level folders whose name starts with [`NAME`], and
/// those of them holding `.notesapp/vault.yaml`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Listed {
    /// Names of the folders listed.
    pub folders: BTreeSet<String>,
    /// Names of the folders listed with a `vault.yaml`.
    pub vaults: BTreeSet<String>,
}

impl Listed {
    /// Gathers the folders from paths relative to the parent folder, as the
    /// query lists them (`Schnipsle`, `Schnipsle/.notesapp/vault.yaml`, …).
    #[must_use]
    pub fn from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> Self {
        let mut listed = Self::default();
        for path in paths {
            let path = path.trim_matches('/');
            let (top, rest) = path.split_once('/').unwrap_or((path, ""));
            if !top.starts_with(NAME) {
                continue;
            }
            listed.folders.insert(top.to_owned());
            if rest == VAULT_FILE {
                listed.vaults.insert(top.to_owned());
            }
        }
        listed
    }

    /// What the folder at `path` holds, from the query and the disk: a vault
    /// if either has one; a folder the query listed but the disk does not
    /// have yet counts as taken, as its contents are not known.
    ///
    /// # Errors
    /// As for [`spot_on_disk`].
    pub fn spot(&self, path: &Path, coordinator: Arc<dyn Coordinator>) -> Result<Spot> {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        if self.vaults.contains(&name) {
            return Ok(Spot::Vault);
        }
        match spot_on_disk(path, coordinator)? {
            Spot::Missing if self.folders.contains(&name) => Ok(Spot::Taken),
            spot => Ok(spot),
        }
    }
}

/// Makes the vault the choice names, or opens the one there. A vault whose
/// `vault.yaml` is not downloaded yet gives [`VaultError::NotDownloaded`].
///
/// # Errors
/// [`VaultError::NotDownloaded`] as above, [`VaultError::AlreadyExists`] if
/// something arrived in an empty folder meanwhile, and the errors of
/// [`Vault::create_with`] and [`Vault::open_with`].
pub fn make_or_open(choice: &Choice, coordinator: Arc<dyn Coordinator>) -> Result<Vault> {
    match choice.spot {
        Spot::Vault => Vault::open_with(&choice.path, coordinator),
        Spot::Missing | Spot::Empty => {
            fs::create_dir_all(&choice.path)?;
            // Never move what arrived meanwhile into notes/ unasked.
            match spot_on_disk(&choice.path, coordinator.clone())? {
                Spot::Missing | Spot::Empty => Vault::create_with(&choice.path, coordinator),
                Spot::Vault => Vault::open_with(&choice.path, coordinator),
                Spot::Taken => Err(VaultError::AlreadyExists(folder_name(&choice.path))),
            }
        }
        Spot::Taken => Err(VaultError::AlreadyExists(folder_name(&choice.path))),
    }
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Uncoordinated;

    fn coordinator() -> Arc<dyn Coordinator> {
        Arc::new(Uncoordinated)
    }

    fn on_disk(parent: &Path) -> Result<Choice> {
        choose(parent, |path| spot_on_disk(path, coordinator()))
    }

    #[test]
    fn a_new_vault_is_made_in_a_new_folder() {
        let dir = tempfile::tempdir().unwrap();
        let choice = on_disk(dir.path()).unwrap();
        assert_eq!(choice.path, dir.path().join("Schnipsle"));
        assert_eq!(choice.spot, Spot::Missing);
        let vault = make_or_open(&choice, coordinator()).unwrap();
        assert!(vault.root().join(VAULT_FILE).is_file());
        assert_eq!(on_disk(dir.path()).unwrap().spot, Spot::Vault);
    }

    #[test]
    fn an_empty_folder_is_used_and_hidden_files_do_not_count() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Schnipsle");
        fs::create_dir(&folder).unwrap();
        fs::write(folder.join(".DS_Store"), "x").unwrap();
        let choice = on_disk(dir.path()).unwrap();
        assert_eq!((choice.path.clone(), choice.spot), (folder, Spot::Empty));
        make_or_open(&choice, coordinator()).unwrap();
    }

    #[test]
    fn a_vault_there_is_opened_and_keeps_its_notes() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("Schnipsle")).unwrap();
        let first = Vault::create(dir.path().join("Schnipsle")).unwrap();
        first.create_note("Kept.md", "# Kept\n").unwrap();
        let choice = on_disk(dir.path()).unwrap();
        assert_eq!(choice.spot, Spot::Vault);
        let vault = make_or_open(&choice, coordinator()).unwrap();
        assert_eq!(vault.identity().unwrap().id, first.identity().unwrap().id);
        assert_eq!(vault.list_notes().unwrap().len(), 1);
    }

    #[test]
    fn a_folder_holding_something_else_is_skipped_and_left_alone() {
        let dir = tempfile::tempdir().unwrap();
        let other = dir.path().join("Schnipsle");
        fs::create_dir(&other).unwrap();
        fs::write(other.join("letter.txt"), "invented").unwrap();
        fs::write(dir.path().join("Schnipsle 1"), "a file").unwrap();
        let choice = on_disk(dir.path()).unwrap();
        assert_eq!(choice.path, dir.path().join("Schnipsle 2"));
        make_or_open(&choice, coordinator()).unwrap();
        assert_eq!(
            fs::read_to_string(other.join("letter.txt")).unwrap(),
            "invented"
        );
        assert!(!other.join(VAULT_FILE).exists());
    }

    #[test]
    fn something_arriving_in_an_empty_folder_is_not_moved() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Schnipsle");
        fs::create_dir(&folder).unwrap();
        let choice = on_disk(dir.path()).unwrap();
        fs::write(folder.join("arrived.md"), "# Arrived\n").unwrap();
        assert!(matches!(
            make_or_open(&choice, coordinator()),
            Err(VaultError::AlreadyExists(_))
        ));
        assert!(folder.join("arrived.md").is_file());
        assert!(!folder.join(VAULT_FILE).exists());
    }

    #[test]
    fn the_query_adds_folders_the_disk_does_not_have_yet() {
        let listed = Listed::from_paths([
            "Schnipsle",
            "Schnipsle/.notesapp",
            "Schnipsle/notes/Not a vault.md",
            "Schnipsle 1/",
            "Schnipsle 1/.notesapp/vault.yaml",
            "Other/.notesapp/vault.yaml",
        ]);
        assert_eq!(
            listed.folders,
            ["Schnipsle", "Schnipsle 1"].map(String::from).into()
        );
        assert_eq!(listed.vaults, ["Schnipsle 1"].map(String::from).into());

        let dir = tempfile::tempdir().unwrap();
        let choice = choose(dir.path(), |p| listed.spot(p, coordinator())).unwrap();
        assert_eq!(choice.path, dir.path().join("Schnipsle 1"));
        assert_eq!(choice.spot, Spot::Vault);
        // Not on the disk yet: opening waits for iCloud.
        assert!(make_or_open(&choice, coordinator()).is_err());
    }

    #[test]
    fn a_vault_on_disk_counts_when_the_query_has_not_listed_it() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("Schnipsle")).unwrap();
        Vault::create(dir.path().join("Schnipsle")).unwrap();
        let listed = Listed::default();
        let choice = choose(dir.path(), |p| listed.spot(p, coordinator())).unwrap();
        assert_eq!(choice.spot, Spot::Vault);
    }

    #[test]
    fn every_name_taken_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let taken = choose(dir.path(), |_| Ok(Spot::Taken));
        assert!(matches!(taken, Err(VaultError::AlreadyExists(_))));
    }
}
