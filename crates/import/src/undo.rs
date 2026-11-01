// SPDX-License-Identifier: Apache-2.0
//! The import runs of a vault and undoing one (ADR-0019, format.md §12.4).
//!
//! [`runs`] lists the runs from their logs. [`plan_undo`] is the dry run of
//! an undo: what would happen to each note the run wrote. [`undo`] then goes
//! through the log in reverse order:
//!
//! - a note edited since the import is kept, and its entry gets `kept`;
//! - an added note goes to the trash;
//! - an updated note gets the version back that the run replaced, after its
//!   current version is stored in the history;
//! - a missing note is left as it is; one in the trash (deleted here, or
//!   trashed by the import as it was in the source's trash) stays there.
//!
//! A note that cannot be moved or restored (an I/O error, a version no longer
//! in the history) is kept like an edited one, so a later import still finds
//! it, and the report says why. Then the log gets `undone`. Undo never
//! deletes text: every version it removes is in the trash or the history.

use schnipsle_core::import_log::IMPORT_LOG_FORMAT;
use schnipsle_core::stamp::rfc3339_utc;
use schnipsle_core::vault::title_of;
use schnipsle_core::{ImportLog, ImportLogError, LogAction, LogEntry, Vault, VaultError};
use serde::Serialize;

use crate::pipeline::{Env, ImportError};

/// One import run, from its log. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportRun {
    /// The run's id, the name of its log without `.yaml`.
    pub id: String,
    /// `source` as in format.md §12.1, e.g. `apple-notes`.
    pub source: String,
    /// The name of the file or folder imported.
    pub input: Option<String>,
    /// When it started, RFC 3339.
    pub started: String,
    /// When it was undone, if it was.
    pub undone: Option<String>,
    /// How many notes it added.
    pub added: usize,
    /// How many notes it updated (§12.3).
    pub updated: usize,
    /// Why it cannot be undone, if it cannot: a damaged log or one written by
    /// a newer version. Such a run has only its id.
    pub problem: Option<String>,
}

/// What an undo does with one note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum UndoAction {
    /// Move the added note to the trash.
    Trash,
    /// Give the updated note the version back the run replaced.
    Restore,
    /// Keep the note: edited since the import, or the undo failed for it.
    Keep,
    /// Nothing: the note is no longer in the vault.
    Missing,
    /// Nothing: the note is in the trash already.
    InTrash,
}

/// One note in an undo. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UndoItem {
    /// Its title now, or the name it was written with if it is missing.
    pub title: String,
    /// Where it is, relative to `notes/`; `None` if missing or in the trash.
    pub path: Option<String>,
    /// What happens, or happened, to it.
    pub action: UndoAction,
    /// Why it is kept or what went wrong, for the report.
    pub problem: Option<String>,
    /// Its name in the trash: for [`UndoAction::InTrash`], and for
    /// [`UndoAction::Trash`] after the undo.
    pub trash_key: Option<String>,
}

/// An undo planned or done: the run and its notes, in the order handled
/// (the reverse of the log).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Undo {
    /// The run, with `undone` set once it is undone.
    pub run: ImportRun,
    /// Its notes.
    pub items: Vec<UndoItem>,
}

impl Undo {
    /// How many notes get `action`.
    #[must_use]
    pub fn count(&self, action: UndoAction) -> usize {
        self.items.iter().filter(|i| i.action == action).count()
    }
}

const EDITED: &str = "Changed here since the import, so it stays as it is.";

/// The import runs in `vault`, newest first.
///
/// # Errors
/// [`ImportError::Vault`] if the folder of logs cannot be read.
pub fn runs(vault: &Vault) -> Result<Vec<ImportRun>, ImportError> {
    let mut runs: Vec<ImportRun> = vault
        .import_logs()?
        .into_iter()
        .map(|(name, log)| {
            let id = name.trim_end_matches(".yaml").to_owned();
            match log {
                Ok(log) => run_of(&log, id),
                Err(e) => ImportRun {
                    id,
                    source: String::new(),
                    input: None,
                    started: String::new(),
                    undone: None,
                    added: 0,
                    updated: 0,
                    problem: Some(e.to_string()),
                },
            }
        })
        .collect();
    runs.reverse();
    Ok(runs)
}

fn run_of(log: &ImportLog, id: String) -> ImportRun {
    let entries = log.entries();
    let count = |action| entries.iter().filter(|e| e.action == action).count();
    ImportRun {
        id,
        source: log.source(),
        input: log.input(),
        started: log.started(),
        undone: log.undone(),
        added: count(LogAction::Added),
        updated: count(LogAction::Updated),
        problem: (log.format() > IMPORT_LOG_FORMAT)
            .then(|| ImportLogError::NewerFormat(log.format()).to_string()),
    }
}

/// The log of the run `run_id`, if it may be undone.
fn undoable(vault: &Vault, run_id: &str) -> Result<ImportLog, ImportError> {
    let name = format!("{run_id}.yaml");
    let Some((_, log)) = vault.import_logs()?.into_iter().find(|(n, _)| *n == name) else {
        return Err(ImportError::Source(format!(
            "There is no import {run_id} in this vault."
        )));
    };
    let log = log?;
    if log.format() > IMPORT_LOG_FORMAT {
        return Err(ImportLogError::NewerFormat(log.format()).into());
    }
    if let Some(time) = log.undone() {
        return Err(ImportError::Source(format!(
            "This import was undone already, at {time}."
        )));
    }
    Ok(log)
}

/// A note as the undo finds it.
enum Found {
    Missing,
    /// In the trash, under this key.
    InTrash(String),
    /// Edited since the import, or unreadable (with why).
    Keep(String, Option<String>),
    /// As the run wrote it: its path, text and hash.
    Unchanged(String, String, String),
}

fn find(vault: &Vault, entry: &LogEntry) -> Found {
    let Some(path) = vault.find_note(&entry.id, entry.path.strip_prefix("notes/")) else {
        return vault
            .find_trashed(&entry.id)
            .map_or(Found::Missing, Found::InTrash);
    };
    match vault.read_note_text(&path) {
        Ok(note) if note.hash == entry.hash => Found::Unchanged(path, note.text, note.hash),
        Ok(_) => Found::Keep(path, None),
        Err(e) => Found::Keep(path, Some(format!("It could not be read: {e}"))),
    }
}

fn item(vault: &Vault, entry: &LogEntry, path: Option<String>, action: UndoAction) -> UndoItem {
    let title = path
        .as_deref()
        .and_then(|p| vault.read_note_text(p).ok())
        .map_or_else(
            || stem_of(&entry.path).to_owned(),
            |n| title_of(&n.text, path.as_deref().map_or("", stem_of)),
        );
    UndoItem {
        title,
        path,
        action,
        problem: None,
        trash_key: None,
    }
}

/// The item for a note found in the trash under `key`, titled as it is there.
fn in_trash(vault: &Vault, entry: &LogEntry, key: String) -> UndoItem {
    let stem = stem_of(&entry.path);
    let title = vault
        .read_trashed(&key)
        .map_or_else(|_| stem.to_owned(), |text| title_of(&text, stem));
    UndoItem {
        title,
        path: None,
        action: UndoAction::InTrash,
        problem: None,
        trash_key: Some(key),
    }
}

fn stem_of(path: &str) -> &str {
    let name = path.rsplit('/').next().unwrap_or(path);
    name.strip_suffix(".md").unwrap_or(name)
}

/// The dry run of undoing the run `run_id`: what would happen to each note.
/// Writes nothing.
///
/// # Errors
/// [`ImportError::Source`] if there is no such run or it was undone;
/// [`ImportError::Log`] for a damaged log or one of a newer format.
pub fn plan_undo(vault: &Vault, run_id: &str) -> Result<Undo, ImportError> {
    let log = undoable(vault, run_id)?;
    let items = log
        .entries()
        .iter()
        .rev()
        .map(|entry| match find(vault, entry) {
            Found::Missing => item(vault, entry, None, UndoAction::Missing),
            Found::InTrash(key) => in_trash(vault, entry, key),
            Found::Keep(path, why) => UndoItem {
                problem: Some(why.unwrap_or_else(|| EDITED.to_owned())),
                ..item(vault, entry, Some(path), UndoAction::Keep)
            },
            Found::Unchanged(path, ..) => {
                let action = match entry.action {
                    LogAction::Added => UndoAction::Trash,
                    LogAction::Updated => UndoAction::Restore,
                };
                item(vault, entry, Some(path), action)
            }
        })
        .collect();
    Ok(Undo {
        run: run_of(&log, run_id.to_owned()),
        items,
    })
}

/// Undoes the run `run_id` (format.md §12.4) and returns what was done.
///
/// # Errors
/// As [`plan_undo`]; [`ImportError::Vault`] for a read-only vault;
/// [`ImportError::Log`] if the log cannot be written, in which case the notes
/// are undone but the log is not marked.
pub fn undo(vault: &Vault, run_id: &str, env: &dyn Env) -> Result<Undo, ImportError> {
    if let Some(why) = vault.read_only() {
        return Err(VaultError::ReadOnly(why.to_owned()).into());
    }
    let mut log = undoable(vault, run_id)?;
    let entries = log.entries();
    let mut items = Vec::with_capacity(entries.len());
    for (position, entry) in entries.iter().enumerate().rev() {
        let (item, kept) = undo_one(vault, entry);
        if kept && !entry.kept {
            let mut entry = entry.clone();
            entry.kept = true;
            log.update(position, &entry);
        }
        items.push(item);
    }
    log.set_undone(&rfc3339_utc(env.now_ms()));
    vault.write_import_log(&mut log)?;
    Ok(Undo {
        run: run_of(&log, run_id.to_owned()),
        items,
    })
}

/// Undoes one entry; `true` if the note stays and the entry gets `kept`.
fn undo_one(vault: &Vault, entry: &LogEntry) -> (UndoItem, bool) {
    let failed = |path: String, why: String| {
        let item = UndoItem {
            problem: Some(why),
            ..item(vault, entry, Some(path), UndoAction::Keep)
        };
        (item, true)
    };
    let (path, text, hash) = match find(vault, entry) {
        Found::Missing => return (item(vault, entry, None, UndoAction::Missing), false),
        Found::InTrash(key) => return (in_trash(vault, entry, key), false),
        Found::Keep(path, why) => return failed(path, why.unwrap_or_else(|| EDITED.to_owned())),
        Found::Unchanged(path, text, hash) => (path, text, hash),
    };
    match entry.action {
        LogAction::Added => {
            let title = item(vault, entry, Some(path.clone()), UndoAction::Trash).title;
            match vault.trash_note(&path) {
                Ok(trashed) => (
                    UndoItem {
                        title,
                        path: Some(path),
                        action: UndoAction::Trash,
                        problem: None,
                        trash_key: Some(trashed.key),
                    },
                    false,
                ),
                Err(e) => failed(path, format!("It could not be moved to the trash: {e}")),
            }
        }
        LogAction::Updated => {
            let restored = entry
                .replaced
                .as_deref()
                .ok_or_else(|| "The run did not record the version it replaced.".to_owned())
                .and_then(|name| {
                    vault
                        .read_snapshot(&entry.id, &format!("{name}.md.zst"))
                        .map_err(|e| format!("The version it replaced is not in the history: {e}"))
                })
                .and_then(|previous| {
                    vault
                        .snapshot_if_new(&entry.id, text.as_bytes())
                        .and_then(|_| vault.replace_note_as_is(&path, &previous, &hash))
                        .map_err(|e| format!("Its previous version could not be restored: {e}"))
                });
            match restored {
                Ok(_) => (item(vault, entry, Some(path), UndoAction::Restore), false),
                Err(why) => failed(path, why),
            }
        }
    }
}
