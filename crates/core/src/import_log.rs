// SPDX-License-Identifier: Apache-2.0
//! Import logs: one file per import run in `.notesapp/import-log/<run-id>.yaml`
//! (format.md §12.2, ADR-0019).
//!
//! A log lists every note a run wrote, so a later run can update those notes
//! (§12.3) and the run can be undone (§12.4). [`ImportLog`] holds the file as
//! YAML, so keys this version does not know stay as they are when the log is
//! written again, as do entries it cannot use.
//!
//! [`Vault::write_import_log`] creates a log the first time and afterwards
//! replaces it only if the file is still what was last written or read
//! (format.md §10).

use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::Serialize;
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlEmitter, YamlLoader};

use crate::layout::IMPORT_LOG_DIR;
use crate::stamp::is_ulid;
use crate::vault::{Access, TempFile, Vault, VaultError, accessor_ran_twice, sha256_hex};

/// The `format` this version reads and writes.
pub const IMPORT_LOG_FORMAT: i64 = 1;

/// Errors from reading or writing an import log.
#[derive(Debug)]
pub enum ImportLogError {
    /// Reading or writing the file failed, or the vault is read-only. A
    /// [`VaultError::Conflict`] means the log changed since it was read.
    Vault(VaultError),
    /// The file is not YAML or lacks a required key; it is left alone.
    Damaged(String),
    /// The log has a newer `format`; it must not be changed (§12.2 rule 5).
    NewerFormat(i64),
}

impl fmt::Display for ImportLogError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vault(e) => write!(f, "{e}"),
            Self::Damaged(why) => write!(f, "the import log is damaged: {why}"),
            Self::NewerFormat(n) => write!(
                f,
                "the import log was written by a newer version (format {n}); it is left as it is"
            ),
        }
    }
}

impl std::error::Error for ImportLogError {}

impl From<VaultError> for ImportLogError {
    fn from(e: VaultError) -> Self {
        Self::Vault(e)
    }
}

impl From<io::Error> for ImportLogError {
    fn from(e: io::Error) -> Self {
        Self::Vault(e.into())
    }
}

type Result<T> = std::result::Result<T, ImportLogError>;

/// What a run did to a note. Serialized as in the file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum LogAction {
    /// The run created the note.
    Added,
    /// The run replaced an earlier import of the note (§12.3).
    Updated,
}

impl LogAction {
    fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Updated => "updated",
        }
    }
}

/// One note a run wrote (§12.2). Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LogEntry {
    /// The note's identifier in the source (§12.1).
    pub source_id: String,
    /// The note's `id` (format.md §4).
    pub id: String,
    /// Where it was written, relative to the vault root; only a hint.
    pub path: String,
    /// Added or updated.
    pub action: LogAction,
    /// The source's modification time of the note, if it has one.
    pub source_modified: Option<String>,
    /// SHA-256 (lowercase hex) of the file as written.
    pub hash: String,
    /// The snapshot (§7) of the version this run replaced, without extension.
    pub replaced: Option<String>,
    /// Kept by an undo because it was edited after the import (§12.4).
    pub kept: bool,
}

impl LogEntry {
    fn of(entry: &Yaml) -> Option<Self> {
        let Yaml::Hash(map) = entry else {
            return None;
        };
        let field = |name: &str| map.get(&key(name)).and_then(scalar);
        let action = match field("action")?.as_str() {
            "added" => LogAction::Added,
            "updated" => LogAction::Updated,
            _ => return None,
        };
        Some(Self {
            source_id: field("source_id")?,
            id: field("id")?,
            path: field("path")?,
            action,
            source_modified: field("source_modified"),
            hash: field("hash")?,
            replaced: field("replaced"),
            kept: map.get(&key("kept")).and_then(Yaml::as_bool) == Some(true),
        })
    }

    /// Writes the fields into `map`, keeping keys it does not know.
    fn write_into(&self, map: &mut Hash) {
        let mut set = |name: &str, value: Option<&str>| match value {
            Some(v) => {
                map.replace(key(name), Yaml::String(v.to_owned()));
            }
            None => {
                map.remove(&key(name));
            }
        };
        set("source_id", Some(&self.source_id));
        set("id", Some(&self.id));
        set("path", Some(&self.path));
        set("action", Some(self.action.as_str()));
        set("source_modified", self.source_modified.as_deref());
        set("hash", Some(&self.hash));
        set("replaced", self.replaced.as_deref());
        if self.kept {
            map.replace(key("kept"), Yaml::Boolean(true));
        } else {
            map.remove(&key("kept"));
        }
    }
}

/// An import log, kept as the YAML it was read from (see the module
/// documentation).
#[derive(Debug, Clone, PartialEq)]
pub struct ImportLog {
    /// The top-level map; `notes` holds a null that marks its place, the
    /// entries are in `entries`.
    doc: Hash,
    /// The entries of `notes`, as written.
    entries: Vec<Yaml>,
    /// Hash of the file as last read or written, `None` if not on disk yet.
    on_disk: Option<String>,
}

impl ImportLog {
    /// A new log for a run that starts now: `id` is the run's ULID, `source`
    /// as in §12.1, `input` the name of the chosen file or folder, `started`
    /// an RFC 3339 time.
    #[must_use]
    pub fn new(id: &str, source: &str, input: &str, started: &str) -> Self {
        let mut doc = Hash::new();
        doc.insert(key("format"), Yaml::Integer(IMPORT_LOG_FORMAT));
        doc.insert(key("id"), Yaml::String(id.to_owned()));
        doc.insert(key("source"), Yaml::String(source.to_owned()));
        doc.insert(key("input"), Yaml::String(input.to_owned()));
        doc.insert(key("started"), Yaml::String(started.to_owned()));
        doc.insert(key("undone"), Yaml::Null);
        doc.insert(key("notes"), Yaml::Null);
        Self {
            doc,
            entries: Vec::new(),
            on_disk: None,
        }
    }

    /// Reads the content of a log file.
    ///
    /// # Errors
    /// [`ImportLogError::Damaged`] if `text` is not YAML, not a map, or lacks
    /// one of `format`, `id`, `source`, `started` and a `notes` list.
    pub fn parse(text: &str) -> Result<Self> {
        let damaged = |why: &str| ImportLogError::Damaged(why.to_owned());
        let docs = YamlLoader::load_from_str(text).map_err(|e| damaged(&e.to_string()))?;
        let Some(Yaml::Hash(mut doc)) = docs.into_iter().next() else {
            return Err(damaged("not a map"));
        };
        let entries = match doc.get(&key("notes")) {
            Some(Yaml::Array(entries)) => entries.clone(),
            _ => return Err(damaged("`notes` is not a list")),
        };
        doc.replace(key("notes"), Yaml::Null);
        if doc.get(&key("format")).and_then(Yaml::as_i64).is_none() {
            return Err(damaged("no `format`"));
        }
        for name in ["id", "source", "started"] {
            if doc.get(&key(name)).and_then(scalar).is_none() {
                return Err(damaged(&format!("no `{name}`")));
            }
        }
        Ok(Self {
            doc,
            entries,
            on_disk: None,
        })
    }

    /// The file content: YAML with the keys in the order they were read, new
    /// keys at the end. Comments in the file read are not kept.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        let mut doc = self.doc.clone();
        doc.replace(key("notes"), Yaml::Array(self.entries.clone()));
        let mut out = String::new();
        // Writing to a String cannot fail.
        let _ = YamlEmitter::new(&mut out).dump(&Yaml::Hash(doc));
        // The emitter writes null as `~`; written as in format.md §12.2.
        let mut text = String::with_capacity(out.len());
        for line in out.strip_prefix("---\n").unwrap_or(&out).lines() {
            match line.strip_suffix('~') {
                Some(head) if head.ends_with(": ") => {
                    text.push_str(head);
                    text.push_str("null");
                }
                _ => text.push_str(line),
            }
            text.push('\n');
        }
        text
    }

    fn text(&self, name: &str) -> Option<String> {
        self.doc.get(&key(name)).and_then(scalar)
    }

    /// The `format` of the log.
    #[must_use]
    pub fn format(&self) -> i64 {
        self.doc
            .get(&key("format"))
            .and_then(Yaml::as_i64)
            .unwrap_or(IMPORT_LOG_FORMAT)
    }

    /// The run's id, which names the file.
    #[must_use]
    pub fn id(&self) -> String {
        self.text("id").unwrap_or_default()
    }

    /// The source, e.g. `apple-notes`.
    #[must_use]
    pub fn source(&self) -> String {
        self.text("source").unwrap_or_default()
    }

    /// The name of the file or folder imported, for display.
    #[must_use]
    pub fn input(&self) -> Option<String> {
        self.text("input")
    }

    /// When the run started.
    #[must_use]
    pub fn started(&self) -> String {
        self.text("started").unwrap_or_default()
    }

    /// When the run ended; `None` while it writes or if it was interrupted.
    #[must_use]
    pub fn finished(&self) -> Option<String> {
        self.text("finished")
    }

    /// When the run was undone, if it was.
    #[must_use]
    pub fn undone(&self) -> Option<String> {
        self.text("undone")
    }

    /// The entries it can use, in the order written; others are skipped.
    #[must_use]
    pub fn entries(&self) -> Vec<LogEntry> {
        self.entries.iter().filter_map(LogEntry::of).collect()
    }

    /// Adds an entry at the end; returns its position for [`Self::update`].
    pub fn push(&mut self, entry: &LogEntry) -> usize {
        let mut map = Hash::new();
        entry.write_into(&mut map);
        self.entries.push(Yaml::Hash(map));
        self.entries.len() - 1
    }

    /// Changes the entry at `position` (as returned by [`Self::push`]),
    /// keeping keys it does not know. Does nothing for a position that is
    /// not an entry map.
    pub fn update(&mut self, position: usize, entry: &LogEntry) {
        if let Some(Yaml::Hash(map)) = self.entries.get_mut(position) {
            entry.write_into(map);
        }
    }

    /// Removes the entry at `position`, e.g. a note that could not be written.
    pub fn remove(&mut self, position: usize) {
        if position < self.entries.len() {
            self.entries.remove(position);
        }
    }

    /// Sets `finished`, the end of the run.
    pub fn finish(&mut self, time: &str) {
        let value = Yaml::String(time.to_owned());
        if self.doc.contains_key(&key("finished")) {
            self.doc.replace(key("finished"), value);
            return;
        }
        // A new `finished` goes after `started`, as in format.md §12.2.
        let mut doc = Hash::new();
        for (k, v) in std::mem::take(&mut self.doc) {
            let after = k == key("started");
            doc.insert(k, v);
            if after {
                doc.insert(key("finished"), value.clone());
            }
        }
        if !doc.contains_key(&key("finished")) {
            doc.insert(key("finished"), value); // no `started`
        }
        self.doc = doc;
    }

    /// Sets `undone` (§12.4).
    pub fn set_undone(&mut self, time: &str) {
        // `replace` keeps the key's place, or adds it at the end.
        self.doc
            .replace(key("undone"), Yaml::String(time.to_owned()));
    }
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_owned())
}

/// A scalar as text: strings as they are, numbers as written (a hash or id
/// made of digits only reads as a number).
fn scalar(value: &Yaml) -> Option<String> {
    match value {
        Yaml::String(s) | Yaml::Real(s) => Some(s.clone()),
        Yaml::Integer(n) => Some(n.to_string()),
        _ => None,
    }
}

impl Vault {
    fn import_log_dir(&self) -> PathBuf {
        self.root().join(IMPORT_LOG_DIR)
    }

    /// All import logs, oldest run first (their ULID names sort by time), each
    /// with its file name; a log that cannot be read comes with its error.
    ///
    /// # Errors
    /// I/O errors listing the folder; none if it does not exist.
    pub fn import_logs(&self) -> std::result::Result<Vec<(String, Result<ImportLog>)>, VaultError> {
        let dir = self.import_log_dir();
        let names = match fs::read_dir(&dir) {
            Ok(entries) => {
                let mut names: Vec<String> = entries
                    .filter_map(std::result::Result::ok)
                    .filter_map(|e| e.file_name().into_string().ok())
                    .filter(|n| n.strip_suffix(".yaml").is_some_and(is_ulid))
                    .collect();
                names.sort();
                names
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(e) => return Err(e.into()),
        };
        Ok(names
            .into_iter()
            .map(|name| {
                let log = self.read_import_log(&dir.join(&name));
                (name, log)
            })
            .collect())
    }

    fn read_import_log(&self, file: &std::path::Path) -> Result<ImportLog> {
        let bytes = self.coordinated(Access::Read, file, |f| Ok(fs::read(f)?))?;
        let text = String::from_utf8(bytes.clone())
            .map_err(|_| ImportLogError::Damaged("not UTF-8".into()))?;
        let mut log = ImportLog::parse(text.strip_prefix('\u{feff}').unwrap_or(&text))?;
        log.on_disk = Some(sha256_hex(&bytes));
        Ok(log)
    }

    /// Writes `log` to `.notesapp/import-log/<id>.yaml` atomically and
    /// coordinated: a new log only if no file has that name, an existing one
    /// only if the file is still what was last read or written.
    ///
    /// # Errors
    /// [`ImportLogError::NewerFormat`]; [`VaultError::InvalidId`] if the id is
    /// not a ULID; [`VaultError::AlreadyExists`] or [`VaultError::Conflict`]
    /// if the file is not what `log` expects; a read-only vault; I/O errors.
    pub fn write_import_log(&self, log: &mut ImportLog) -> Result<()> {
        self.check_writable()?;
        if log.format() > IMPORT_LOG_FORMAT {
            return Err(ImportLogError::NewerFormat(log.format()));
        }
        let id = log.id();
        if !is_ulid(&id) {
            return Err(VaultError::InvalidId(id).into());
        }
        let name = format!("{id}.yaml");
        let dir = self.import_log_dir();
        fs::create_dir_all(&dir)?;
        let target = dir.join(&name);
        let text = log.to_yaml();
        let mut tmp = Some(TempFile::write(&target, text.as_bytes())?);
        let expected = log.on_disk.clone();
        let shown = format!("{IMPORT_LOG_DIR}/{name}");
        self.coordinated(Access::Write, &target, |file| {
            let tmp = tmp.take().ok_or_else(accessor_ran_twice)?;
            match &expected {
                None => match tmp.place_new(file) {
                    Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                        Err(VaultError::AlreadyExists(shown.clone()))
                    }
                    result => Ok(result?),
                },
                Some(hash) => {
                    let current_hash = match fs::read(file) {
                        Ok(bytes) => Some(sha256_hex(&bytes)),
                        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
                        Err(e) => return Err(e.into()),
                    };
                    if current_hash.as_ref() != Some(hash) {
                        return Err(VaultError::Conflict {
                            path: shown.clone(),
                            current_hash,
                        });
                    }
                    Ok(tmp.replace(file)?)
                }
            }
        })?;
        log.on_disk = Some(sha256_hex(text.as_bytes()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/format/12-import-log")
    }

    fn read(name: &str) -> ImportLog {
        ImportLog::parse(&fs::read_to_string(fixtures().join(name)).unwrap()).unwrap()
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir_all(dir.path().join(".notesapp")).unwrap();
        fs::write(
            dir.path().join(".notesapp/vault.yaml"),
            "format: 1\nid: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T\n",
        )
        .unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn reads_the_fixtures() {
        let log = read("finished.yaml");
        assert_eq!(log.format(), 1);
        assert_eq!(log.source(), "apple-notes");
        assert_eq!(log.input().as_deref(), Some("Schnipsle-Import"));
        assert_eq!(log.finished().as_deref(), Some("2026-10-29T10:15:42+01:00"));
        assert_eq!(log.undone(), None);
        let entries = log.entries();
        assert_eq!(entries.len(), 2);
        assert_eq!(
            entries[0].path,
            "notes/Apple Notes/Amateurfunk & Radio/QRP Kits/antenna-tuner.md"
        );
        assert_eq!(entries[0].action, LogAction::Added);
        assert_eq!(
            entries[0].source_modified.as_deref(),
            Some("2025-11-20T18:40:12Z")
        );
        assert_eq!(entries[1].source_modified, None);

        let update = &read("reimport.yaml").entries()[0];
        assert_eq!(update.source_id, "0a1b2c3d4e5f40718293a4b5c6d7e8f9");
        assert_eq!(update.action, LogAction::Updated);
        assert_eq!(update.replaced.as_deref(), Some("20261102T070001Z"));

        let undone = read("undone.yaml");
        assert_eq!(
            undone.undone().as_deref(),
            Some("2026-10-30T21:30:00+01:00")
        );
        assert_eq!(
            undone.entries().iter().map(|e| e.kept).collect::<Vec<_>>(),
            [true, false]
        );

        // Interrupted: no `finished`, but read like a finished run (rule 3).
        let interrupted = read("interrupted.yaml");
        assert_eq!(interrupted.finished(), None);
        assert_eq!(interrupted.entries().len(), 2);

        assert_eq!(read("newer-format.yaml").format(), 2);
    }

    #[test]
    fn keeps_unknown_keys_and_entries() {
        let mut log = read("finished.yaml");
        log.entries.push(Yaml::String("not an entry".into()));
        let mut entry = log.entries()[0].clone();
        entry.kept = true;
        log.update(0, &entry);
        let text = log.to_yaml();
        assert!(text.contains("written_by: another-tool 1.0"), "{text}");
        assert!(text.contains("- not an entry"), "{text}");
        assert!(text.contains("undone: null"), "{text}");
        let again = ImportLog::parse(&text).unwrap();
        assert_eq!(again.entries().len(), 2);
        assert!(again.entries()[0].kept);
    }

    #[test]
    fn rejects_damaged_logs() {
        for text in [
            "",
            "- a list",
            "format: 1\nid: x\nsource: y\nstarted: z\n",
            "notes: []\n",
        ] {
            assert!(
                matches!(ImportLog::parse(text), Err(ImportLogError::Damaged(_))),
                "{text:?}"
            );
        }
    }

    #[test]
    fn writes_new_logs_and_replaces_only_what_it_wrote() {
        let (dir, vault) = vault();
        let id = "01JB2C3D4E5F6G7H8J9K0M1N2P";
        let mut log = ImportLog::new(id, "simplenote", "export.zip", "2026-10-29T10:00:00Z");
        log.push(&LogEntry {
            source_id: "0001".into(),
            id: "01JB2C3E0F1G2H3J4K5M6N7P8Q".into(),
            path: "notes/Simplenote/a.md".into(),
            action: LogAction::Added,
            source_modified: None,
            hash: "00".repeat(32),
            replaced: None,
            kept: false,
        });
        vault.write_import_log(&mut log).unwrap();
        log.finish("2026-10-29T10:00:01Z");
        vault.write_import_log(&mut log).unwrap();

        let text =
            fs::read_to_string(dir.path().join(IMPORT_LOG_DIR).join(format!("{id}.yaml"))).unwrap();
        assert!(
            text.starts_with(
                "format: 1\nid: 01JB2C3D4E5F6G7H8J9K0M1N2P\nsource: simplenote\n\
                 input: export.zip\nstarted: \"2026-10-29T10:00:00Z\"\n\
                 finished: \"2026-10-29T10:00:01Z\"\nundone: null\nnotes:\n"
            ),
            "{text}"
        );
        let logs = vault.import_logs().unwrap();
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].0, format!("{id}.yaml"));
        let read = logs[0].1.as_ref().unwrap();
        assert_eq!(read.finished().as_deref(), Some("2026-10-29T10:00:01Z"));
        // A source id of digits only stays a string.
        assert_eq!(read.entries()[0].source_id, "0001");

        // Another log with the same id is refused, and so is replacing a
        // log changed since it was written.
        let mut other = ImportLog::new(id, "simplenote", "x", "2026-10-29T11:00:00Z");
        assert!(matches!(
            vault.write_import_log(&mut other),
            Err(ImportLogError::Vault(VaultError::AlreadyExists(_)))
        ));
        let file = dir.path().join(IMPORT_LOG_DIR).join(format!("{id}.yaml"));
        fs::write(&file, fs::read_to_string(&file).unwrap() + "# edited\n").unwrap();
        log.set_undone("2026-10-29T12:00:00Z");
        assert!(matches!(
            vault.write_import_log(&mut log),
            Err(ImportLogError::Vault(VaultError::Conflict { .. }))
        ));
    }

    #[test]
    fn leaves_newer_logs_alone() {
        let (_dir, vault) = vault();
        let mut log = read("newer-format.yaml");
        assert!(matches!(
            vault.write_import_log(&mut log),
            Err(ImportLogError::NewerFormat(2))
        ));
    }
}
