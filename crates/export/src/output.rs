// SPDX-License-Identifier: Apache-2.0
//! Files of an export: names from titles (ADR-0021 rule 3) and writing them.
//!
//! A file goes into a folder under a free name, or is left out if the same
//! name holds other content, so an export never replaces something already
//! in the folder. Only [`crate::Layout::SingleFile`] replaces, as the save dialog
//! has asked.

use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::Path;

use schnipsle_core::vault::{Result, TempFile, Vault, VaultError};
use unicode_normalization::UnicodeNormalization;

use crate::Format;
use crate::document::DEFAULT_TITLE;

/// Longest file name stem in bytes; file systems allow 255, and `-n.html` must fit.
const MAX_STEM: usize = 200;

/// The file name stem for `title`: characters a file name cannot hold become
/// `-`, no dots or spaces at its ends; `note` if nothing is left.
pub(crate) fn file_stem(title: &str) -> String {
    let replaced: String = title
        .nfc()
        .map(|c| {
            if c.is_control() || matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') {
                '-'
            } else {
                c
            }
        })
        .collect();
    let mut end = replaced.len().min(MAX_STEM);
    while !replaced.is_char_boundary(end) {
        end -= 1;
    }
    let stem = replaced[..end].trim_matches(['.', ' ']);
    if stem.is_empty() {
        "note".to_owned()
    } else {
        stem.to_owned()
    }
}

/// The file name a save dialog suggests for one document called `title`.
#[must_use]
pub fn suggested_file_name(title: Option<&str>, format: Format) -> String {
    let stem = file_stem(title.unwrap_or(DEFAULT_TITLE));
    format!("{stem}.{}", format.extension())
}

/// Refuses an export into the vault itself: its copies would sync to every
/// device, next to the notes they were made from. `to` is the file or folder
/// chosen, which need not exist yet.
///
/// # Errors
/// [`VaultError::Io`] if `to` is inside the vault.
pub fn check_target(vault: &Vault, to: &Path) -> Result<()> {
    // A place that cannot be resolved is for the export itself to fail on.
    let resolved = fs::canonicalize(to).or_else(|_| {
        let parent = to.parent().ok_or(io::ErrorKind::NotFound)?;
        fs::canonicalize(parent)
    });
    let (Ok(target), Ok(root)) = (resolved, fs::canonicalize(vault.root())) else {
        return Ok(());
    };
    if target.starts_with(root) {
        return Err(VaultError::Io(io::Error::other(
            "An export cannot go inside the vault; choose a place outside it",
        )));
    }
    Ok(())
}

/// Hands out names in a folder that nothing has yet. File systems often ignore
/// case, so names differing only in case count as the same.
pub(crate) struct Names<'f> {
    folder: &'f Path,
    used: HashSet<String>,
    /// The names handed out with a number.
    pub numbered: Vec<String>,
}

impl<'f> Names<'f> {
    pub fn new(folder: &'f Path) -> Self {
        Self {
            folder,
            used: HashSet::new(),
            numbered: Vec::new(),
        }
    }

    /// A free file name from `title`, numbered `-2`, `-3`, … if taken.
    pub fn free(&mut self, title: &str, extension: &str) -> String {
        let stem = file_stem(title);
        let mut name = format!("{stem}.{extension}");
        let mut n = 1;
        while self.used.contains(&name.to_lowercase()) || self.folder.join(&name).exists() {
            n += 1;
            name = format!("{stem}-{n}.{extension}");
        }
        self.used.insert(name.to_lowercase());
        if n > 1 {
            self.numbered.push(name.clone());
        }
        name
    }
}

/// A unique anchor per note: `note-` and the slug of its title, numbered if taken.
pub(crate) fn free_anchor(used: &mut HashSet<String>, slug: &str) -> String {
    let mut anchor = format!("note-{slug}");
    let mut n = 1;
    while !used.insert(anchor.clone()) {
        n += 1;
        anchor = format!("note-{slug}-{n}");
    }
    anchor
}

/// Writes the file `target`, replacing what is there.
pub(crate) fn replace_file(target: &Path, bytes: &[u8]) -> io::Result<()> {
    TempFile::write(target, bytes)?.replace(target)
}

/// Writes the new file `target`, with the folders it needs.
pub(crate) fn write_new(target: &Path, bytes: &[u8]) -> io::Result<()> {
    if let Some(parent) = target.parent() {
        fs::create_dir_all(parent)?;
    }
    TempFile::write(target, bytes)?.place_new(target)
}

/// Copies `bytes` to `target` unless it is there already; `false` if something
/// else is there, which stays.
pub(crate) fn copy_new(target: &Path, bytes: &[u8]) -> io::Result<bool> {
    match write_new(target, bytes) {
        Ok(()) => Ok(true),
        Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Ok(fs::read(target)? == bytes),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn suggested_names_come_from_the_title() {
        assert_eq!(
            suggested_file_name(Some("a/b: c"), Format::Pdf),
            "a-b- c.pdf"
        );
        assert_eq!(
            suggested_file_name(Some("#tuner"), Format::Odt),
            "#tuner.odt"
        );
        assert_eq!(suggested_file_name(None, Format::Html), "Notes.html");
    }

    #[test]
    fn an_export_stays_out_of_the_vault() {
        let place = tempfile::tempdir().unwrap();
        let root = place.path().join("vault");
        fs::create_dir(&root).unwrap();
        let vault = Vault::create(&root).unwrap();
        let outside = place.path().join("elsewhere");
        fs::create_dir(&outside).unwrap();

        for inside in [
            root.clone(),
            root.join("export.pdf"),
            root.join("notes"),
            root.join("notes/export.pdf"),
        ] {
            let refused = check_target(&vault, &inside).unwrap_err().to_string();
            assert!(refused.contains("inside the vault"), "{refused}");
        }
        check_target(&vault, &outside).unwrap();
        check_target(&vault, &outside.join("export.pdf")).unwrap();
        // Next to the vault, with a name that starts like it.
        check_target(&vault, &place.path().join("vault.pdf")).unwrap();
        // A place that does not exist is for the export to report.
        check_target(&vault, &outside.join("missing/export.pdf")).unwrap();
    }

    #[test]
    fn names_from_titles() {
        assert_eq!(file_stem("Packing list"), "Packing list");
        assert_eq!(file_stem("a/b: c?"), "a-b- c-");
        assert_eq!(file_stem("  ..hidden.. "), "hidden");
        assert_eq!(file_stem("..."), "note");
        assert_eq!(file_stem(""), "note");
        assert_eq!(file_stem("Gru\u{308}n"), "Grün");
        assert_eq!(file_stem(&"ä".repeat(300)).len(), 200);
    }

    #[test]
    fn taken_names_are_numbered() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("There.html"), "mine").unwrap();
        let mut names = Names::new(dir.path());
        assert_eq!(names.free("There", "html"), "There-2.html");
        assert_eq!(names.free("New", "html"), "New.html");
        assert_eq!(names.free("new", "html"), "new-2.html");
        assert_eq!(names.free("New", "html"), "New-3.html");
        assert_eq!(names.numbered, ["There-2.html", "new-2.html", "New-3.html"]);
    }

    #[test]
    fn copies_never_replace_other_content() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a/b.png");
        assert!(copy_new(&file, b"one").unwrap());
        assert!(copy_new(&file, b"one").unwrap());
        assert!(!copy_new(&file, b"two").unwrap());
        assert_eq!(fs::read(&file).unwrap(), b"one");
    }
}
