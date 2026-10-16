// SPDX-License-Identifier: Apache-2.0
//! Access to a vault folder: listing, reading and safely writing notes.
//!
//! Implements `docs/format.md` §3 (note files), §5.2 (title) and §10 (writing rules):
//! writes are atomic (temporary file + rename) and refuse to overwrite a note that
//! changed since it was read. Paths given to this module are relative to the vault
//! root and use `/` as separator.
//!
//! iCloud Drive keeps notes that are not downloaded yet as hidden placeholder files
//! (`.Name.md.icloud`). They are listed with `available: false` so the app can ask
//! the system to download them.
//!
//! Every file access runs inside the vault's [`Coordinator`] (ADR-0015).

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Write as _};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::conflicts::{ConflictVersion, NoSyncConflicts, SyncConflicts};
use crate::coordination::{Coordinator, Uncoordinated};
use crate::front_matter;
use crate::stamp::{new_ulid, now_ms, rfc3339_utc};

/// Errors from vault operations.
#[derive(Debug)]
pub enum VaultError {
    /// The vault root is missing or not a directory.
    NotADirectory(PathBuf),
    /// A note path is absolute, leaves the vault, or does not end in `.md`.
    InvalidPath(String),
    /// The note exists only as a cloud placeholder and must be downloaded first.
    NotDownloaded(String),
    /// The note changed on disk since it was read; nothing was written.
    Conflict {
        /// Path of the note, relative to the vault.
        path: String,
        /// Hash of the content now on disk, or `None` if the note was deleted.
        current_hash: Option<String>,
    },
    /// A note to be created already exists.
    AlreadyExists(String),
    /// A note's front matter `id` is not a ULID, so it cannot name its history folder.
    InvalidId(String),
    /// Any other file-system error.
    Io(io::Error),
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotADirectory(p) => write!(f, "not a folder: {}", p.display()),
            Self::InvalidPath(p) => write!(f, "invalid note path: {p}"),
            Self::NotDownloaded(p) => write!(f, "not downloaded yet: {p}"),
            Self::Conflict { path, .. } => {
                write!(f, "changed elsewhere since it was opened: {path}")
            }
            Self::AlreadyExists(p) => write!(f, "a note with this name already exists: {p}"),
            Self::InvalidId(id) => write!(f, "the note's id is not valid: {id}"),
            Self::Io(e) => write!(f, "{e}"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<io::Error> for VaultError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Result type of this module.
pub type Result<T> = std::result::Result<T, VaultError>;

/// A note as shown in a list. Serialized with camelCase field names for the UI.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NoteEntry {
    /// Path relative to the vault root, `/`-separated, e.g. `projects/tuner.md`.
    pub path: String,
    /// Title per format.md §5.2; the file name for placeholders.
    pub title: String,
    /// Last modification, milliseconds since the Unix epoch.
    pub modified_ms: u64,
    /// `false` if only a cloud placeholder is on this device.
    pub available: bool,
}

/// A note's content together with the hash used for conflict detection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct NoteContent {
    /// Path relative to the vault root.
    pub path: String,
    /// The complete file content.
    pub text: String,
    /// SHA-256 of the content as hex; pass it back to [`Vault::write_note`].
    pub hash: String,
    /// Versions a sync service kept because they conflict with this one
    /// (ADR-0006); resolve them with [`Vault::resolve_conflict`].
    pub conflicts: Vec<ConflictVersion>,
    /// Why `conflicts` could not be listed, if it could not; the note itself
    /// was read normally.
    #[serde(rename = "conflictsError")]
    pub conflicts_error: Option<String>,
}

/// A note as it was written: the text actually on disk, which carries the
/// front matter the app maintains (format.md §4), and its hash.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SavedNote {
    /// The complete file content as written.
    pub text: String,
    /// SHA-256 of `text`; pass it to the next [`Vault::write_note`].
    pub hash: String,
}

/// What a changed path means for the vault, see [`Vault::classify`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathKind {
    /// A note (or its iCloud placeholder); vault-relative path.
    Note(String),
    /// The root or a folder: any notes below it may have changed.
    Folder,
    /// Outside the vault, hidden, or not a note.
    Ignored,
}

/// An opened vault folder. Cheap to clone; clones share the coordinator.
#[derive(Debug, Clone)]
pub struct Vault {
    root: PathBuf,
    coordinator: Arc<dyn Coordinator>,
    sync: Arc<dyn SyncConflicts>,
}

impl Vault {
    /// Opens the vault at `root` without file coordination.
    ///
    /// # Errors
    /// [`VaultError::NotADirectory`] if `root` is not an existing folder.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        Self::open_with(root, Arc::new(Uncoordinated))
    }

    /// Opens the vault at `root`; all file access goes through `coordinator`.
    ///
    /// # Errors
    /// [`VaultError::NotADirectory`] if `root` is not an existing folder.
    pub fn open_with(root: impl Into<PathBuf>, coordinator: Arc<dyn Coordinator>) -> Result<Self> {
        let root = root.into();
        if root.is_dir() {
            Ok(Self {
                root,
                coordinator,
                sync: Arc::new(NoSyncConflicts),
            })
        } else {
            Err(VaultError::NotADirectory(root))
        }
    }

    /// Uses `sync` to find conflict versions kept by a sync service.
    #[must_use]
    pub fn with_sync_conflicts(mut self, sync: Arc<dyn SyncConflicts>) -> Self {
        self.sync = sync;
        self
    }

    pub(crate) fn sync(&self) -> &dyn SyncConflicts {
        self.sync.as_ref()
    }

    /// The vault's root folder.
    #[must_use]
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Lists all notes (`*.md`) below the root, newest first. Hidden files and
    /// folders (names starting with `.`, including `.notesapp/`) are skipped,
    /// except iCloud placeholders of notes.
    ///
    /// # Errors
    /// I/O errors while reading the root folder; unreadable subfolders are skipped.
    pub fn list_notes(&self) -> Result<Vec<NoteEntry>> {
        // One coordinated read of the folder covers the files inside it.
        let mut notes = self.coordinated(Access::Read, &self.root, |root| {
            let mut notes = Vec::new();
            collect(root, "", &mut notes, true)?;
            Ok(notes)
        })?;
        notes.sort_by(|a, b| {
            b.modified_ms
                .cmp(&a.modified_ms)
                .then_with(|| a.path.cmp(&b.path))
        });
        Ok(notes)
    }

    /// Reads a note.
    ///
    /// # Errors
    /// [`VaultError::NotDownloaded`] for iCloud placeholders, [`VaultError::InvalidPath`],
    /// or I/O errors (e.g. the note does not exist).
    pub fn read_note(&self, path: &str) -> Result<NoteContent> {
        let full = self.resolve(path)?;
        // Checked before coordinating: a coordinated read of a placeholder waits
        // for the download, which can take forever when offline.
        if !full.exists() && placeholder_of(&full).exists() {
            return Err(VaultError::NotDownloaded(path.to_owned()));
        }
        let mut note = self.coordinated(Access::Read, &full, |file| match fs::read(file) {
            Ok(bytes) => {
                let text = String::from_utf8_lossy(&bytes).into_owned();
                Ok(NoteContent {
                    path: path.to_owned(),
                    hash: sha256_hex(&bytes),
                    text,
                    conflicts: Vec::new(),
                    conflicts_error: None,
                })
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound && placeholder_of(file).exists() => {
                Err(VaultError::NotDownloaded(path.to_owned()))
            }
            Err(e) => Err(e.into()),
        })?;
        // Conflict versions stay with the sync service until resolved, so a
        // failure to list them loses nothing; it must not make the note
        // unreadable, but it is reported rather than hidden.
        match self.conflicts_at(path, &full, &note.text) {
            Ok(conflicts) => note.conflicts = conflicts,
            Err(e) => {
                eprintln!("conflict versions of {path} unavailable: {e}");
                note.conflicts_error = Some(e.to_string());
            }
        }
        Ok(note)
    }

    /// Writes a note atomically, but only if it still has the content that was read.
    ///
    /// `expected_hash` is the hash from [`Vault::read_note`] (or a previous write).
    /// The note gets `id` and `created` if missing and an updated `modified`
    /// (format.md §4); `id` and `created` already on disk are kept even if `text`
    /// lacks them, and so is the whole block if `text` has none. Text equal to
    /// what is on disk is not written again. Returns what is now on disk.
    ///
    /// # Errors
    /// [`VaultError::Conflict`] if the note changed or disappeared since it was read;
    /// nothing is written in that case.
    pub fn write_note(&self, path: &str, text: &str, expected_hash: &str) -> Result<SavedNote> {
        let full = self.resolve(path)?;
        let current = self.coordinated(Access::Read, &full, |file| match fs::read(file) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        })?;
        let current_hash = current.as_deref().map(sha256_hex);
        let Some(current) = current.filter(|_| current_hash.as_deref() == Some(expected_hash))
        else {
            return Err(VaultError::Conflict {
                path: path.to_owned(),
                current_hash,
            });
        };
        if current == text.as_bytes() {
            return Ok(SavedNote {
                text: text.to_owned(),
                hash: expected_hash.to_owned(),
            });
        }
        let stamped = stamp_for_save(
            text,
            Some(&String::from_utf8_lossy(&current)),
            created_ms(&full),
        )?;

        // Write and flush first, then check and replace while holding exclusive
        // access: with a real coordinator nothing can change the note in between;
        // without one the window shrinks to one read plus the rename.
        let mut tmp = Some(TempFile::write(&full, stamped.as_bytes())?);
        self.coordinated(Access::Write, &full, |file| {
            let current_hash = match fs::read(file) {
                Ok(bytes) => Some(sha256_hex(&bytes)),
                Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                Err(e) => return Err(e.into()),
            };
            if current_hash.as_deref() != Some(expected_hash) {
                return Err(VaultError::Conflict {
                    path: path.to_owned(),
                    current_hash,
                });
            }
            let tmp = tmp.take().ok_or_else(accessor_ran_twice)?;
            tmp.replace(file)?;
            Ok(())
        })?;
        Ok(SavedNote {
            hash: sha256_hex(stamped.as_bytes()),
            text: stamped,
        })
    }

    /// Creates a new note with `id`, `created` and `modified` in its front
    /// matter (format.md §4); fails if the file already exists. Returns what is
    /// now on disk.
    ///
    /// # Errors
    /// [`VaultError::AlreadyExists`], [`VaultError::InvalidPath`] or I/O errors.
    pub fn create_note(&self, path: &str, text: &str) -> Result<SavedNote> {
        let full = self.resolve(path)?;
        if let Some(parent) = full.parent() {
            fs::create_dir_all(parent)?;
        }
        let stamped = stamp_for_save(text, None, now_ms())?;
        let mut tmp = Some(TempFile::write(&full, stamped.as_bytes())?);
        self.coordinated(Access::Write, &full, |file| {
            let exists = || VaultError::AlreadyExists(path.to_owned());
            if placeholder_of(file).exists() {
                return Err(exists());
            }
            let tmp = tmp.take().ok_or_else(accessor_ran_twice)?;
            match tmp.place_new(file) {
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(exists()),
                result => Ok(result?),
            }
        })?;
        Ok(SavedNote {
            hash: sha256_hex(stamped.as_bytes()),
            text: stamped,
        })
    }

    /// Runs `f` inside the coordinator and passes its result through. `f` gets
    /// the path to use, which the coordinator may have updated.
    pub(crate) fn coordinated<T>(
        &self,
        access: Access,
        path: &Path,
        mut f: impl FnMut(&Path) -> Result<T>,
    ) -> Result<T> {
        let mut outcome = None;
        {
            let mut accessor = |file: &Path| {
                outcome = Some(f(file));
                Ok(())
            };
            match access {
                Access::Read => self.coordinator.read(path, &mut accessor)?,
                Access::Write => self.coordinator.write(path, &mut accessor)?,
                Access::Delete => self.coordinator.delete(path, &mut accessor)?,
            }
        }
        outcome.unwrap_or_else(|| {
            Err(io::Error::other("file coordination did not grant access").into())
        })
    }

    /// Classifies an absolute path reported by a file watcher (ADR-0015).
    ///
    /// The root and the reported path may each be in canonical form or go
    /// through a symlink: iOS opens the vault as `/private/var/mobile/…` but its
    /// presenter reports `/var/mobile/…`. Every combination is tried.
    #[must_use]
    pub fn classify(&self, file: &Path) -> PathKind {
        let roots = [Some(self.root.clone()), canonical_lenient(&self.root)];
        let files = [Some(file.to_owned()), canonical_lenient(file)];
        let Some(rel) = files.iter().flatten().find_map(|f| {
            roots
                .iter()
                .flatten()
                .find_map(|root| f.strip_prefix(root).ok().map(Path::to_path_buf))
        }) else {
            return PathKind::Ignored;
        };
        let mut parts: Vec<String> = Vec::new();
        for component in rel.components() {
            match component {
                Component::Normal(part) => parts.push(part.to_string_lossy().into_owned()),
                _ => return PathKind::Ignored,
            }
        }
        let Some(last) = parts.pop() else {
            return PathKind::Folder; // the vault root itself
        };
        if parts.iter().any(|p| p.starts_with('.')) {
            return PathKind::Ignored; // inside .notesapp/ or another hidden folder
        }
        let prefix = if parts.is_empty() {
            String::new()
        } else {
            format!("{}/", parts.join("/"))
        };
        if let Some(real) = placeholder_note_name(&last) {
            return PathKind::Note(format!("{prefix}{real}"));
        }
        if last.starts_with('.') {
            return PathKind::Ignored; // temporary files, .DS_Store, …
        }
        if has_md_extension(&last) {
            return PathKind::Note(format!("{prefix}{last}"));
        }
        // Gone or a folder: either way we cannot tell which notes it held.
        if file.is_dir() || Path::new(&last).extension().is_none() {
            PathKind::Folder
        } else {
            PathKind::Ignored // attachments and other files
        }
    }

    /// Maps a vault-relative note path to a file path, rejecting anything that
    /// could leave the vault or is not a Markdown note.
    pub(crate) fn resolve(&self, path: &str) -> Result<PathBuf> {
        let invalid = || VaultError::InvalidPath(path.to_owned());
        let rel = Path::new(path);
        if path.is_empty() || !has_md_extension(path) || path.contains('\\') {
            return Err(invalid());
        }
        let mut full = self.root.clone();
        for component in rel.components() {
            match component {
                Component::Normal(part) if !part.to_string_lossy().starts_with('.') => {
                    full.push(part);
                }
                _ => return Err(invalid()),
            }
        }
        Ok(full)
    }
}

/// The text to write for `text` (format.md §4): `id` and `created` from `text`,
/// else from `previous` (what is on disk), else new; `modified` now. If `text`
/// has no front matter but `previous` does, that block is kept, so keys other
/// tools added survive an editor that lost them.
fn stamp_for_save(text: &str, previous: Option<&str>, created_fallback_ms: u64) -> Result<String> {
    let from = |key: &str| {
        front_matter::get(text, key).or_else(|| previous.and_then(|p| front_matter::get(p, key)))
    };
    let id = from("id").map_or_else(new_ulid, Ok)?;
    let created = from("created").unwrap_or_else(|| rfc3339_utc(created_fallback_ms));
    let kept_block = previous
        .filter(|_| front_matter::block(text).is_none())
        .and_then(front_matter::block);
    let text = kept_block.map_or_else(|| text.to_owned(), |block| format!("{block}{text}"));
    Ok(front_matter::stamp(
        &text,
        &id,
        &created,
        &rfc3339_utc(now_ms()),
    ))
}

/// Creation time of a file for `created`: its birth time where the file system
/// records one, else its modification time, else now.
pub(crate) fn created_ms(file: &Path) -> u64 {
    fs::metadata(file)
        .and_then(|m| m.created().or_else(|_| m.modified()))
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or_else(now_ms, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// Last modification of a file, else now; shown for copies.
pub(crate) fn modified_ms_of(file: &Path) -> u64 {
    fs::metadata(file).map_or_else(|_| now_ms(), |m| modified_ms(&m))
}

/// Canonical form of `path`, also for paths that no longer exist (a deleted
/// note): the nearest existing ancestor is canonicalized, the rest re-attached.
fn canonical_lenient(path: &Path) -> Option<PathBuf> {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        if let Ok(mut canonical) = fs::canonicalize(current) {
            canonical.extend(missing.iter().rev());
            return Some(canonical);
        }
        missing.push(current.file_name()?);
        current = current.parent()?;
    }
}

/// Recursively collects notes below `dir`; `prefix` is `dir` relative to the vault.
fn collect(dir: &Path, prefix: &str, out: &mut Vec<NoteEntry>, is_root: bool) -> Result<()> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if !is_root => {
            // A subfolder we cannot read must not hide the rest of the vault.
            let _ = e;
            return Ok(());
        }
        Err(e) => return Err(e.into()),
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let modified_ms = entry.metadata().map_or(0, |m| modified_ms(&m));

        if let Some(real) = placeholder_note_name(&name) {
            out.push(NoteEntry {
                path: format!("{prefix}{real}"),
                title: real.trim_end_matches(".md").to_owned(),
                modified_ms,
                available: false,
            });
        } else if name.starts_with('.') {
            // hidden file or folder, including .notesapp/
        } else if file_type.is_dir() {
            collect(&entry.path(), &format!("{prefix}{name}/"), out, false)?;
        } else if file_type.is_file() && has_md_extension(&name) {
            let stem = name[..name.len() - 3].to_owned();
            let title = read_head(&entry.path())
                .map_or_else(|_| stem.clone(), |head| title_of(&head, &stem));
            out.push(NoteEntry {
                path: format!("{prefix}{name}"),
                title,
                modified_ms,
                available: true,
            });
        }
    }
    Ok(())
}

/// Title per format.md §5.2: the first level-1 ATX heading if it is the first
/// non-empty block, else the first non-empty line (max. 120 characters), else
/// the file name without extension. Front matter is skipped.
#[must_use]
pub fn title_of(text: &str, file_stem: &str) -> String {
    let body = skip_front_matter(text);
    let Some(first) = body.lines().map(str::trim).find(|l| !l.is_empty()) else {
        return file_stem.to_owned();
    };
    let line = first
        .strip_prefix("# ")
        .or_else(|| (first == "#").then_some(""))
        .map_or(first, |h| h.trim().trim_end_matches('#').trim_end());
    if line.is_empty() {
        file_stem.to_owned()
    } else {
        line.chars().take(120).collect()
    }
}

fn skip_front_matter(text: &str) -> &str {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let Some(rest) = text
        .strip_prefix("---\n")
        .or_else(|| text.strip_prefix("---\r\n"))
    else {
        return text;
    };
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        offset += line.len();
        if line.trim_end() == "---" {
            return &rest[offset..];
        }
    }
    text // unterminated front matter: treat as body
}

fn has_md_extension(name: &str) -> bool {
    name.len() > 3 && name.to_ascii_lowercase().ends_with(".md")
}

/// `.Name.md.icloud` → `Name.md`
fn placeholder_note_name(name: &str) -> Option<&str> {
    let inner = name.strip_prefix('.')?.strip_suffix(".icloud")?;
    has_md_extension(inner).then_some(inner)
}

fn placeholder_of(full: &Path) -> PathBuf {
    let name = full
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    full.with_file_name(format!(".{name}.icloud"))
}

fn read_head(path: &Path) -> io::Result<String> {
    use std::io::Read as _;
    let mut buf = vec![0; 4096];
    let n = fs::File::open(path)?.read(&mut buf)?;
    buf.truncate(n);
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn modified_ms(meta: &fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// SHA-256 of `bytes` as lowercase hex.
#[must_use]
pub fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut hex = String::with_capacity(64);
    for b in digest {
        let _ = write!(hex, "{b:02x}");
    }
    hex
}

#[derive(Debug, Clone, Copy)]
pub(crate) enum Access {
    Read,
    Write,
    Delete,
}

/// A coordinator broke its contract and called an accessor a second time.
pub(crate) fn accessor_ran_twice() -> VaultError {
    io::Error::other("file coordination ran the same write twice").into()
}

/// A fully written and flushed temporary file next to its target (format.md §10).
/// Dropping it without placing it removes the file.
pub(crate) struct TempFile {
    path: PathBuf,
}

impl TempFile {
    pub(crate) fn write(target: &Path, bytes: &[u8]) -> io::Result<Self> {
        static COUNTER: AtomicU64 = AtomicU64::new(0);
        let dir = target.parent().unwrap_or_else(|| Path::new("."));
        let name = target
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let tmp = Self {
            path: dir.join(format!(
                ".{name}.tmp-{}-{}",
                std::process::id(),
                COUNTER.fetch_add(1, Ordering::Relaxed)
            )),
        };
        let mut file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&tmp.path)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        Ok(tmp)
    }

    /// Renames the file over `target`, replacing what is there.
    pub(crate) fn replace(self, target: &Path) -> io::Result<()> {
        fs::rename(&self.path, target)
        // on success Drop finds nothing left to remove
    }

    /// Puts the file at `target` only if nothing is there yet. A hard link fails
    /// atomically with `AlreadyExists`, so a note created elsewhere in the meantime
    /// is never replaced. File systems without hard links fall back to an
    /// exclusive create, which is equally safe but not atomic for the content.
    pub(crate) fn place_new(self, target: &Path) -> io::Result<()> {
        match fs::hard_link(&self.path, target) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => Err(e),
            Err(_) => {
                let bytes = fs::read(&self.path)?;
                let mut file = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(target)?;
                let result = file.write_all(&bytes).and_then(|()| file.sync_all());
                if result.is_err() {
                    // We created this file a moment ago; never leave half of it behind.
                    let _ = fs::remove_file(target);
                }
                result
            }
        }
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn titles() {
        assert_eq!(
            title_of("# Antenna test #radio\n\ntext", "x"),
            "Antenna test #radio"
        );
        assert_eq!(title_of("\n\nShopping list\n- milk", "x"), "Shopping list");
        assert_eq!(title_of("", "fallback"), "fallback");
        assert_eq!(
            title_of("---\nid: 1\n---\n# Real title\n", "x"),
            "Real title"
        );
        assert_eq!(title_of("## Second level\n", "x"), "## Second level");
        assert_eq!(title_of("# Closed heading ##\n", "x"), "Closed heading");
        assert_eq!(title_of(&"a".repeat(200), "x").chars().count(), 120);
    }

    #[test]
    fn lists_notes_and_skips_hidden() {
        let (dir, vault) = vault();
        let root = dir.path();
        fs::write(root.join("a.md"), "# Alpha\n").unwrap();
        fs::create_dir_all(root.join("sub")).unwrap();
        fs::write(root.join("sub/b.MD"), "Beta line\n").unwrap();
        fs::write(root.join("image.png"), [0u8; 4]).unwrap();
        fs::create_dir_all(root.join(".notesapp/history")).unwrap();
        fs::write(root.join(".notesapp/history/x.md"), "hidden").unwrap();
        fs::write(root.join(".hidden.md"), "hidden").unwrap();
        fs::write(root.join(".Cloud note.md.icloud"), "plist").unwrap();

        let mut notes = vault.list_notes().unwrap();
        notes.sort_by(|a, b| a.path.cmp(&b.path));
        let summary: Vec<_> = notes
            .iter()
            .map(|n| (n.path.as_str(), n.title.as_str(), n.available))
            .collect();
        assert_eq!(
            summary,
            [
                ("Cloud note.md", "Cloud note", false),
                ("a.md", "Alpha", true),
                ("sub/b.MD", "Beta line", true),
            ]
        );
    }

    #[test]
    fn read_write_roundtrip_and_conflict() {
        let (dir, vault) = vault();
        fs::write(dir.path().join("n.md"), "one").unwrap();

        let note = vault.read_note("n.md").unwrap();
        assert_eq!(note.text, "one");
        let h2 = vault.write_note("n.md", "two", &note.hash).unwrap().hash;
        let on_disk = fs::read_to_string(dir.path().join("n.md")).unwrap();
        assert_eq!(front_matter::body(&on_disk), "two");

        // Someone else changes the file: our next save must not overwrite it.
        fs::write(dir.path().join("n.md"), "changed on the Mac").unwrap();
        let err = vault.write_note("n.md", "three", &h2).unwrap_err();
        assert!(matches!(
            err,
            VaultError::Conflict {
                current_hash: Some(_),
                ..
            }
        ));
        assert_eq!(
            fs::read_to_string(dir.path().join("n.md")).unwrap(),
            "changed on the Mac"
        );

        // Deleted elsewhere is a conflict too.
        fs::remove_file(dir.path().join("n.md")).unwrap();
        let err = vault.write_note("n.md", "four", &h2).unwrap_err();
        assert!(matches!(
            err,
            VaultError::Conflict {
                current_hash: None,
                ..
            }
        ));
        assert!(!dir.path().join("n.md").exists());
    }

    #[test]
    fn create_adds_front_matter() {
        let (dir, vault) = vault();
        let saved = vault.create_note("n.md", "# Title\n").unwrap();
        let on_disk = fs::read_to_string(dir.path().join("n.md")).unwrap();
        assert_eq!(saved.text, on_disk);
        assert_eq!(saved.hash, sha256_hex(on_disk.as_bytes()));
        assert_eq!(front_matter::body(&on_disk), "# Title\n");
        assert!(crate::stamp::is_ulid(
            &front_matter::get(&on_disk, "id").unwrap()
        ));
        assert_eq!(
            front_matter::get(&on_disk, "created"),
            front_matter::get(&on_disk, "modified")
        );
    }

    #[test]
    fn first_write_of_a_plain_note_adds_front_matter() {
        let (dir, vault) = vault();
        fs::write(dir.path().join("n.md"), "plain").unwrap();
        let note = vault.read_note("n.md").unwrap();
        // Reading never adds front matter (format.md §3).
        assert_eq!(
            fs::read_to_string(dir.path().join("n.md")).unwrap(),
            "plain"
        );
        let saved = vault
            .write_note("n.md", "plain, edited", &note.hash)
            .unwrap();
        assert_eq!(front_matter::body(&saved.text), "plain, edited");
        for key in ["id", "created", "modified"] {
            assert!(front_matter::get(&saved.text, key).is_some(), "{key}");
        }
    }

    #[test]
    fn identity_and_other_keys_survive_an_editor_without_them() {
        let (dir, vault) = vault();
        let id = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
        let disk = format!(
            "---\nid: {id}\ncreated: 2026-01-01T00:00:00Z\npinned: true\nmodified: 2026-01-01T00:00:00Z\n---\nold\n"
        );
        fs::write(dir.path().join("n.md"), &disk).unwrap();
        let hash = sha256_hex(disk.as_bytes());

        // The editor lost the whole block.
        let saved = vault.write_note("n.md", "new\n", &hash).unwrap();
        assert_eq!(front_matter::get(&saved.text, "id").as_deref(), Some(id));
        assert_eq!(
            front_matter::get(&saved.text, "created").as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            front_matter::get(&saved.text, "pinned").as_deref(),
            Some("true")
        );
        assert_ne!(
            front_matter::get(&saved.text, "modified").as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(front_matter::body(&saved.text), "new\n");

        // The editor has a block, but without `id` (e.g. typed before the first save returned).
        let saved = vault
            .write_note("n.md", "---\npinned: true\n---\nnewer\n", &saved.hash)
            .unwrap();
        assert_eq!(front_matter::get(&saved.text, "id").as_deref(), Some(id));
    }

    #[test]
    fn unchanged_text_is_not_written_again() {
        let (dir, vault) = vault();
        let saved = vault.create_note("n.md", "x").unwrap();
        let before = fs::metadata(dir.path().join("n.md"))
            .unwrap()
            .modified()
            .unwrap();
        std::thread::sleep(std::time::Duration::from_millis(20));
        let again = vault.write_note("n.md", &saved.text, &saved.hash).unwrap();
        assert_eq!(again, saved);
        let after = fs::metadata(dir.path().join("n.md"))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(before, after);
    }

    #[test]
    fn a_hand_edited_invalid_id_does_not_block_saving() {
        let (dir, vault) = vault();
        fs::write(dir.path().join("n.md"), "---\nid: mine\n---\nx").unwrap();
        let note = vault.read_note("n.md").unwrap();
        let saved = vault
            .write_note("n.md", "---\nid: mine\n---\ny", &note.hash)
            .unwrap();
        assert_eq!(
            front_matter::get(&saved.text, "id").as_deref(),
            Some("mine")
        );
    }

    #[test]
    fn no_temp_files_left_behind() {
        let (dir, vault) = vault();
        let h = vault.create_note("deep/new.md", "x").unwrap().hash;
        vault.write_note("deep/new.md", "y", &h).unwrap();
        let names: Vec<_> = fs::read_dir(dir.path().join("deep"))
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["new.md"]);
    }

    #[test]
    fn conflict_leaves_no_temp_file() {
        let (dir, vault) = vault();
        let h = vault.create_note("n.md", "one").unwrap().hash;
        fs::write(dir.path().join("n.md"), "changed elsewhere").unwrap();
        assert!(vault.write_note("n.md", "mine", &h).is_err());
        let names: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["n.md"]);
    }

    #[test]
    fn concurrent_creates_never_replace_each_other() {
        let (dir, vault) = vault();
        let winners: Vec<_> = std::thread::scope(|s| {
            let handles: Vec<_> = (0..16)
                .map(|i| {
                    let vault = &vault;
                    s.spawn(move || vault.create_note("race.md", &format!("writer {i}")))
                })
                .collect();
            handles.into_iter().map(|h| h.join().unwrap()).collect()
        });
        let ok: Vec<_> = winners.iter().filter_map(|r| r.as_ref().ok()).collect();
        assert_eq!(ok.len(), 1, "exactly one create may succeed");
        assert!(
            winners
                .iter()
                .filter(|r| r.is_err())
                .all(|r| matches!(r, Err(VaultError::AlreadyExists(_))))
        );
        // The file on disk is the winner's, untouched by the others.
        let on_disk = fs::read(dir.path().join("race.md")).unwrap();
        assert_eq!(sha256_hex(&on_disk), ok[0].hash);
        assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
    }

    /// A coordinator for tests: logs every call, and can act like another
    /// device (change the file before granting access), refuse access, or
    /// report that the file moved while waiting.
    #[derive(Debug, Default)]
    struct Scripted {
        log: std::sync::Mutex<Vec<(&'static str, PathBuf)>>,
        before_write: Option<fn(&Path) -> PathBuf>,
        refuse: bool,
    }

    impl Scripted {
        fn run(
            &self,
            kind: &'static str,
            path: &Path,
            f: &mut dyn FnMut(&Path) -> io::Result<()>,
        ) -> io::Result<()> {
            self.log.lock().unwrap().push((kind, path.to_owned()));
            if self.refuse {
                return Err(io::Error::other("refused"));
            }
            let path = match (kind, self.before_write) {
                ("write", Some(hook)) => hook(path),
                _ => path.to_owned(),
            };
            f(&path)
        }

        fn calls(&self) -> Vec<(&'static str, PathBuf)> {
            self.log.lock().unwrap().clone()
        }
    }

    impl Coordinator for Scripted {
        fn read(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
            self.run("read", path, f)
        }
        fn write(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
            self.run("write", path, f)
        }
    }

    fn scripted_vault(coordinator: Scripted) -> (tempfile::TempDir, Vault, Arc<Scripted>) {
        let dir = tempfile::tempdir().unwrap();
        let coordinator = Arc::new(coordinator);
        let vault = Vault::open_with(dir.path(), coordinator.clone()).unwrap();
        (dir, vault, coordinator)
    }

    fn names_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        names.sort();
        names
    }

    #[test]
    fn every_access_is_coordinated() {
        let (dir, vault, c) = scripted_vault(Scripted::default());
        let root = dir.path();
        let h = vault.create_note("n.md", "one").unwrap().hash;
        let note = vault.read_note("n.md").unwrap();
        vault.write_note("n.md", "two", &note.hash).unwrap();
        vault.list_notes().unwrap();
        assert_eq!(h, note.hash);
        assert_eq!(
            c.calls(),
            [
                ("write", root.join("n.md")), // create
                ("read", root.join("n.md")),  // read_note
                ("read", root.to_owned()),    // read_note: copies with the same id
                ("read", root.join("n.md")),  // write_note: id and created on disk
                ("write", root.join("n.md")), // write_note: check and replace
                ("read", root.to_owned()),    // list_notes
            ]
        );
    }

    #[test]
    fn change_made_while_waiting_for_access_is_a_conflict() {
        let (dir, vault, _) = scripted_vault(Scripted {
            before_write: Some(|p| {
                fs::write(p, "saved by iCloud while we waited").unwrap();
                p.to_owned()
            }),
            ..Scripted::default()
        });
        fs::write(dir.path().join("n.md"), "one").unwrap();
        let note = vault.read_note("n.md").unwrap();
        let err = vault.write_note("n.md", "mine", &note.hash).unwrap_err();
        assert!(matches!(err, VaultError::Conflict { .. }));
        assert_eq!(
            fs::read_to_string(dir.path().join("n.md")).unwrap(),
            "saved by iCloud while we waited"
        );
        assert_eq!(names_in(dir.path()), ["n.md"]);
    }

    #[test]
    fn refused_access_writes_nothing() {
        let (dir, vault, _) = scripted_vault(Scripted {
            refuse: true,
            ..Scripted::default()
        });
        fs::write(dir.path().join("n.md"), "one").unwrap();
        let hash = sha256_hex(b"one");
        assert!(matches!(
            vault.write_note("n.md", "two", &hash),
            Err(VaultError::Io(_))
        ));
        assert!(matches!(
            vault.create_note("new.md", "x"),
            Err(VaultError::Io(_))
        ));
        assert_eq!(fs::read_to_string(dir.path().join("n.md")).unwrap(), "one");
        assert_eq!(names_in(dir.path()), ["n.md"]);
    }

    #[test]
    fn write_follows_a_note_moved_while_waiting() {
        let (dir, vault, _) = scripted_vault(Scripted {
            before_write: Some(|p| {
                let moved = p.with_file_name("moved.md");
                fs::rename(p, &moved).unwrap();
                moved
            }),
            ..Scripted::default()
        });
        fs::write(dir.path().join("n.md"), "one").unwrap();
        vault
            .write_note("n.md", "two", &sha256_hex(b"one"))
            .unwrap();
        assert_eq!(names_in(dir.path()), ["moved.md"]);
        let on_disk = fs::read_to_string(dir.path().join("moved.md")).unwrap();
        assert_eq!(front_matter::body(&on_disk), "two");
    }

    #[test]
    fn create_refuses_existing_and_placeholders() {
        let (dir, vault) = vault();
        vault.create_note("a.md", "x").unwrap();
        assert!(matches!(
            vault.create_note("a.md", "y"),
            Err(VaultError::AlreadyExists(_))
        ));
        fs::write(dir.path().join(".b.md.icloud"), "").unwrap();
        assert!(matches!(
            vault.create_note("b.md", "y"),
            Err(VaultError::AlreadyExists(_))
        ));
        assert!(matches!(
            vault.read_note("b.md"),
            Err(VaultError::NotDownloaded(_))
        ));
    }

    #[test]
    fn rejects_paths_outside_the_vault() {
        let (_dir, vault) = vault();
        for bad in [
            "",
            "../x.md",
            "/etc/x.md",
            "a/../../x.md",
            ".notesapp/x.md",
            "x.txt",
            "a\\b.md",
            "./x.md",
        ] {
            assert!(
                matches!(vault.read_note(bad), Err(VaultError::InvalidPath(_))),
                "{bad} must be rejected"
            );
        }
    }

    #[test]
    fn entry_serializes_camel_case() {
        let entry = NoteEntry {
            path: "a.md".into(),
            title: "A".into(),
            modified_ms: 1,
            available: true,
        };
        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("\"modifiedMs\":1"), "{json}");
    }

    #[test]
    fn open_requires_a_folder() {
        assert!(matches!(
            Vault::open("/definitely/not/here"),
            Err(VaultError::NotADirectory(_))
        ));
    }
}
