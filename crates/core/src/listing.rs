// SPDX-License-Identifier: Apache-2.0
//! Listing folders with each entry's kind, modification time and size (ADR-0047).
//!
//! Listing all notes, the index sync and the search for copies of a note
//! (ADR-0046) need, for every file in a folder, its modification time and
//! size. They ask a [`FolderLister`] for them. [`PlainLister`] reads the
//! folder and asks each entry with `lstat`, as `std::fs` does; on Apple
//! platforms the app shell passes in one from `schnipsle-apple` that gets
//! them for many entries in one system call. Both must give the same answers.
//!
//! On macOS and iOS an entry also says whether it is *dataless*: iCloud
//! keeps its name and size on the device but not its content, which is
//! downloaded when it is read (ADR-0050). Such a file counts as not
//! downloaded, like a `.Name.md.icloud` placeholder.

use std::ffi::OsString;
use std::fmt;
use std::fs;
use std::io;
use std::path::Path;

use crate::vault::modified_ms;

/// What an entry of a folder is, as `lstat` tells it: a symbolic link is
/// neither a file nor a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EntryKind {
    /// A regular file.
    File,
    /// A folder.
    Folder,
    /// Anything else, such as a symbolic link.
    Other,
}

/// An entry's modification time and size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FileStat {
    /// Milliseconds since the Unix epoch; 0 for a time before it.
    pub modified_ms: u64,
    /// The size in bytes of a file; 0 for anything else.
    pub size: u64,
}

impl FileStat {
    /// The time and size `meta` gives.
    #[must_use]
    pub fn of(meta: &fs::Metadata) -> Self {
        Self {
            modified_ms: modified_ms(meta),
            size: if meta.is_file() { meta.len() } else { 0 },
        }
    }
}

/// One entry of a folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    /// Its name in the folder.
    pub name: OsString,
    /// What it is.
    pub kind: EntryKind,
    /// Its time and size; `None` if the system could not tell them.
    pub stat: Option<FileStat>,
    /// Its content is not on this device (`SF_DATALESS`, ADR-0050); always
    /// `false` on systems without the flag. Only files are read, so only a
    /// dataless file counts as not downloaded.
    pub dataless: bool,
}

/// `SF_DATALESS` of `<sys/stat.h>`: the file's content is not on this
/// device; reading it makes the system download it first.
pub const SF_DATALESS: u32 = 0x4000_0000;

/// Whether `meta`, from `lstat`, is of an entry whose content is not on
/// this device. `false` on systems without the flag.
#[must_use]
pub fn is_dataless(meta: &fs::Metadata) -> bool {
    flags_of(meta) & SF_DATALESS != 0
}

#[cfg(target_os = "macos")]
fn flags_of(meta: &fs::Metadata) -> u32 {
    std::os::macos::fs::MetadataExt::st_flags(meta)
}

#[cfg(target_os = "ios")]
fn flags_of(meta: &fs::Metadata) -> u32 {
    std::os::ios::fs::MetadataExt::st_flags(meta)
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn flags_of(_meta: &fs::Metadata) -> u32 {
    0
}

/// Lists the entries of a folder, in no particular order.
pub trait FolderLister: Send + Sync + fmt::Debug {
    /// The entries of `dir`, without `.` and `..`. An entry whose kind
    /// cannot be told is left out.
    ///
    /// # Errors
    /// If `dir` cannot be read, the same error `std::fs::read_dir` gives.
    fn list(&self, dir: &Path) -> io::Result<Vec<FolderEntry>>;
}

/// Lists with `std::fs::read_dir` and one `lstat` per entry.
#[derive(Debug, Default, Clone, Copy)]
pub struct PlainLister;

impl FolderLister for PlainLister {
    fn list(&self, dir: &Path) -> io::Result<Vec<FolderEntry>> {
        let mut out = Vec::new();
        for entry in fs::read_dir(dir)?.flatten() {
            let Ok(file_type) = entry.file_type() else {
                continue;
            };
            // Like `lstat`: `DirEntry::metadata` does not follow links.
            let meta = entry.metadata().ok();
            out.push(FolderEntry {
                name: entry.file_name(),
                kind: kind_of(file_type),
                stat: meta.as_ref().map(FileStat::of),
                dataless: meta.as_ref().is_some_and(is_dataless),
            });
        }
        Ok(out)
    }
}

/// The kind of an entry with `file_type`, which does not follow links.
#[must_use]
pub fn kind_of(file_type: fs::FileType) -> EntryKind {
    if file_type.is_file() {
        EntryKind::File
    } else if file_type.is_dir() {
        EntryKind::Folder
    } else {
        EntryKind::Other
    }
}

/// For tests: lists as [`PlainLister`] and marks the files with these names
/// as dataless, which a test cannot make without iCloud.
#[cfg(test)]
#[derive(Debug, Default)]
pub(crate) struct MarkDataless(pub(crate) Vec<&'static str>);

#[cfg(test)]
impl FolderLister for MarkDataless {
    fn list(&self, dir: &Path) -> io::Result<Vec<FolderEntry>> {
        let mut entries = PlainLister.list(dir)?;
        for entry in &mut entries {
            entry.dataless = self.0.iter().any(|name| entry.name == *name);
        }
        Ok(entries)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn sorted(mut entries: Vec<FolderEntry>) -> Vec<FolderEntry> {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    #[test]
    fn lists_files_folders_and_links_with_time_and_size() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "hello").unwrap();
        let when =
            UNIX_EPOCH + Duration::from_millis(1_700_000_000_123) + Duration::from_nanos(456);
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(when)
            .unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(&file, dir.path().join("link.md")).unwrap();

        let entries = sorted(PlainLister.list(dir.path()).unwrap());
        let names: Vec<_> = entries.iter().map(|e| e.name.to_string_lossy()).collect();
        #[cfg(unix)]
        assert_eq!(names, ["a.md", "link.md", "sub"]);
        assert_eq!(entries[0].kind, EntryKind::File);
        assert_eq!(
            entries[0].stat,
            Some(FileStat {
                modified_ms: 1_700_000_000_123,
                size: 5
            })
        );
        #[cfg(unix)]
        assert_eq!(entries[1].kind, EntryKind::Other);
        let folder = entries.last().unwrap();
        assert_eq!(folder.kind, EntryKind::Folder);
        assert_eq!(folder.stat.unwrap().size, 0);
        // Files written here have their content on this device.
        assert!(entries.iter().all(|e| !e.dataless));
    }

    #[test]
    fn a_file_written_here_is_not_dataless() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "x").unwrap();
        assert!(!is_dataless(&fs::symlink_metadata(&file).unwrap()));
        assert!(!is_dataless(&fs::symlink_metadata(dir.path()).unwrap()));
    }

    #[test]
    fn a_time_before_1970_counts_as_0() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("old.md");
        fs::write(&file, "x").unwrap();
        let before = UNIX_EPOCH - Duration::from_millis(1500);
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(before)
            .unwrap();
        let entries = PlainLister.list(dir.path()).unwrap();
        assert_eq!(entries[0].stat.unwrap().modified_ms, 0);
    }

    #[test]
    fn a_missing_folder_gives_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let err = PlainLister.list(&dir.path().join("none")).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::NotFound);
    }

    /// Lists as [`PlainLister`] and records the folders it was asked for.
    #[derive(Debug, Default)]
    struct Recording(std::sync::Mutex<Vec<String>>);

    impl FolderLister for Recording {
        fn list(&self, dir: &Path) -> io::Result<Vec<FolderEntry>> {
            let name = dir.file_name().unwrap().to_string_lossy().into_owned();
            self.0.lock().unwrap().push(name);
            PlainLister.list(dir)
        }
    }

    #[test]
    fn the_vault_lists_its_folders_with_the_lister_it_was_given() {
        let dir = tempfile::tempdir().unwrap();
        let recording = std::sync::Arc::new(Recording::default());
        let vault = crate::Vault::create(dir.path())
            .unwrap()
            .with_folder_lister(recording.clone());
        let notes = vault.notes_dir().to_owned();
        fs::create_dir(notes.join("sub")).unwrap();
        fs::write(notes.join("sub").join("b.md"), "# B").unwrap();
        fs::write(
            notes.join("a.md"),
            "---\nid: 01J0000000000000000000000A\n---\n# A",
        )
        .unwrap();
        let taken = || std::mem::take(&mut *recording.0.lock().unwrap());

        let listed = vault.list_notes_by(crate::SortOrder::Title).unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(taken(), ["notes", "sub"]);

        let mut index = crate::Index::in_memory().unwrap();
        assert_eq!(vault.sync_index(&mut index).unwrap().indexed, 2);
        assert!(taken().starts_with(&["notes".to_owned(), "sub".to_owned()]));

        vault.read_note("a.md").unwrap();
        assert_eq!(taken(), ["notes"]);
    }
}
