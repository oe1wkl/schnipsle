// SPDX-License-Identifier: Apache-2.0
//! The dry run ([`plan`]) and the import itself ([`apply`]), see the crate
//! documentation.

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;

use schnipsle_core::attachments::{attachment_path, link_from};
use schnipsle_core::dates::parse_rfc3339;
use schnipsle_core::stamp::{new_ulid, now_ms, rfc3339_utc};
use schnipsle_core::{
    ImportLog, ImportLogError, LogAction, LogEntry, Vault, VaultError, front_matter,
};
use serde::Serialize;

use crate::note::{AttachmentData, Batch, ImportedNote, Notice, Source};
use crate::prepare::{self, Keys};

/// How many log entries are written ahead of their notes at a time.
const LOG_BATCH: usize = 25;

/// The problem for an update of a note the source has in its trash: an
/// import never deletes a note it wrote before (§12.1 rule 6).
const TRASHED_IN_SOURCE: &str = "It is in the trash in the source; here it stays where it is.";

/// The clock and the id generator, replaceable for tests.
pub trait Env {
    /// Milliseconds since the Unix epoch, now.
    fn now_ms(&self) -> u64;
    /// A new ULID.
    ///
    /// # Errors
    /// If no random numbers are available.
    fn new_id(&self) -> std::io::Result<String>;
}

/// The real clock and random ids.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemEnv;

impl Env for SystemEnv {
    fn now_ms(&self) -> u64 {
        now_ms()
    }

    fn new_id(&self) -> std::io::Result<String> {
        new_ulid()
    }
}

/// Errors that stop an import as a whole. Problems with single notes are in
/// the [`Plan`] and the [`Outcome`] instead.
#[derive(Debug)]
pub enum ImportError {
    /// Reading the vault or writing the import log failed.
    Vault(VaultError),
    /// The import log could not be written; nothing more is written.
    Log(ImportLogError),
    /// Reading the source failed (adapters).
    Source(String),
}

impl fmt::Display for ImportError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vault(e) => write!(f, "{e}"),
            Self::Log(e) => write!(f, "{e}"),
            Self::Source(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for ImportError {}

impl From<VaultError> for ImportError {
    fn from(e: VaultError) -> Self {
        Self::Vault(e)
    }
}

impl From<ImportLogError> for ImportError {
    fn from(e: ImportLogError) -> Self {
        Self::Log(e)
    }
}

impl From<std::io::Error> for ImportError {
    fn from(e: std::io::Error) -> Self {
        Self::Vault(e.into())
    }
}

/// What the import will do with a note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum PlannedAction {
    /// Add it as a new note.
    Add,
    /// Replace the version an earlier import wrote (§12.3).
    Update,
    /// Leave it out, for the reason given.
    Skip,
}

/// Why a note is left out (format.md §12.3 rule 3). Serialized in camelCase.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SkipReason {
    /// The batch has this `source_id` more than once; only the first counts.
    Duplicate,
    /// Imported before and changed here since.
    ChangedHere,
    /// Imported before and not newer in the source.
    NotNewer,
    /// Imported before, but the source or the log has no modification time.
    NoModifiedTime,
    /// Imported before, but the note is no longer in the vault.
    Missing,
    /// Changed here between the dry run and the import.
    ChangedMeanwhile,
    /// Writing it failed; the problem says why.
    Failed,
}

/// One note in the report. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PlanItem {
    /// The note's identifier in the source.
    pub source_id: String,
    /// Its title.
    pub title: String,
    /// What happens to it.
    pub action: PlannedAction,
    /// Why it is skipped.
    pub reason: Option<SkipReason>,
    /// The note's path relative to `notes/`: where it will be added (the
    /// name may still get a number), or the note it updates or that blocks
    /// the update.
    pub path: Option<String>,
    /// Things the user should know, e.g. a date that was not understood.
    pub problems: Vec<String>,
    /// A new note that goes to the trash, as it is in the source's trash
    /// (§12.1 rule 6). Serialized only when true.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub trashed: bool,
}

/// A note ready to be written.
#[derive(Debug, Clone)]
struct Prepared {
    /// Position in [`Plan::items`].
    item: usize,
    /// For a new note its folder in `notes/`, for an update the note's path.
    folder: String,
    title: String,
    text: String,
    attachments: Vec<(String, Vec<u8>)>,
    entry: LogEntry,
    /// For an update: the note's hash when planned.
    expected_hash: Option<String>,
    /// Move the new note to the trash once written.
    trash: bool,
}

/// The dry-run report, and what [`apply`] will write. Serialized with
/// camelCase names; what is to be written is not serialized.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Plan {
    /// The source.
    pub source: Source,
    /// The chosen file or folder.
    pub input: String,
    /// Every note of the batch, in its order.
    pub items: Vec<PlanItem>,
    /// Notes about the import as a whole, from the adapter and the pipeline.
    pub notices: Vec<Notice>,
    #[serde(skip)]
    prepared: Vec<Prepared>,
}

impl Plan {
    /// How many notes have `action`.
    #[must_use]
    pub fn count(&self, action: PlannedAction) -> usize {
        self.items.iter().filter(|i| i.action == action).count()
    }
}

/// The latest log entry per `source_id` of the batch's source (§12.3 rule
/// 1), and notices about logs that cannot be used.
fn imported_before(
    vault: &Vault,
    source: Source,
) -> Result<(HashMap<String, LogEntry>, Vec<Notice>), ImportError> {
    let mut latest = HashMap::new();
    let mut notices = Vec::new();
    for (name, log) in vault.import_logs()? {
        let log = match log {
            Ok(log) if log.format() > schnipsle_core::import_log::IMPORT_LOG_FORMAT => {
                notices.push(Notice::general(format!(
                    "The import log {name} is from a newer version; notes it lists count as \
                     not imported."
                )));
                continue;
            }
            Ok(log) => log,
            Err(e) => {
                notices.push(Notice::general(format!(
                    "The import log {name} cannot be read ({e}); notes it lists count as not \
                     imported."
                )));
                continue;
            }
        };
        if log.source() != source.id() {
            continue;
        }
        let undone = log.undone().is_some();
        for entry in log.entries() {
            if !undone || entry.kept {
                latest.insert(entry.source_id.clone(), entry);
            }
        }
    }
    Ok((latest, notices))
}

/// A valid RFC 3339 time from the source, or the import time with a problem.
fn time_or_now(value: Option<&str>, what: &str, now: &str, problems: &mut Vec<String>) -> String {
    match value.map(str::trim).filter(|v| !v.is_empty()) {
        Some(v) if parse_rfc3339(v).is_some() => v.to_owned(),
        Some(v) => {
            problems.push(format!(
                "The {what} time {v:?} was not understood; the import time is used."
            ));
            now.to_owned()
        }
        None => now.to_owned(),
    }
}

/// The note's text with attachment links pointing to where the attachments
/// will be stored, relative to a note in `folder`; and the attachments.
fn with_attachments(
    note: &ImportedNote,
    body: String,
    folder: &str,
    problems: &mut Vec<String>,
) -> (String, Vec<(String, Vec<u8>)>) {
    let mut body = body;
    let mut files = Vec::new();
    // Any name in the folder: only the folder's depth matters for the link.
    let note_path = format!("{folder}/note.md");
    for attachment in &note.attachments {
        let bytes = match &attachment.data {
            AttachmentData::Bytes(bytes) => bytes.clone(),
            AttachmentData::File(file) => {
                let Ok(bytes) = fs::read(file) else {
                    problems.push(format!(
                        "The attachment {} cannot be read; its link is left as it is.",
                        attachment.file_name
                    ));
                    continue;
                };
                bytes
            }
        };
        let Some(stored) = attachment_path(&attachment.file_name, &bytes) else {
            problems.push(format!(
                "The attachment {} has no usable extension; its link is left as it is.",
                attachment.file_name
            ));
            continue;
        };
        body = prepare::rewrite_link(&body, &attachment.link, &link_from(&note_path, &stored));
        files.push((attachment.file_name.clone(), bytes));
    }
    (body, files)
}

/// The file a note becomes (§12.1): links to where its attachments will be
/// stored relative to a note in `folder`, the final tag line and the front
/// matter of `keys`; and the attachments to store.
fn file_text(
    note: &ImportedNote,
    body: &str,
    folder: &str,
    keys: &Keys<'_>,
    problems: &mut Vec<String>,
) -> (String, Vec<(String, Vec<u8>)>) {
    let (body, attachments) = with_attachments(note, body.to_owned(), folder, problems);
    let tags = prepare::final_tags(&body, &note.folders, &note.tags);
    let body = prepare::append_tags(&body, &tags);
    if let Some(id) = front_matter::get(&body, "id").filter(|id| id != keys.id) {
        problems.push(format!(
            "Its front matter has the id {id}; it gets {} instead, like every imported note.",
            keys.id
        ));
    }
    (prepare::with_front_matter(&body, keys), attachments)
}

/// The note an earlier import wrote, as it may be updated (§12.3).
struct Target {
    /// Its path relative to `notes/`.
    path: String,
    /// Its `created`, which an update keeps.
    created: Option<String>,
    /// Its hash now.
    hash: String,
}

/// Whether the note `entry` logged may be replaced by a version from the
/// source modified at `source_modified` (§12.3 rules 2 and 3); if not, why,
/// with the note's path if it still exists.
fn update_target(
    vault: &Vault,
    entry: &LogEntry,
    source_modified: Option<&str>,
) -> Result<Result<Target, (SkipReason, Option<String>)>, ImportError> {
    let Some(path) = vault.find_note(&entry.id, entry.path.strip_prefix("notes/")) else {
        return Ok(Err((SkipReason::Missing, None)));
    };
    let current = vault.read_note_text(&path)?;
    if current.hash != entry.hash {
        return Ok(Err((SkipReason::ChangedHere, Some(path))));
    }
    let newer = source_modified.and_then(parse_rfc3339);
    let older = entry.source_modified.as_deref().and_then(parse_rfc3339);
    Ok(match (newer, older) {
        (Some(new), Some(old)) if new > old => Ok(Target {
            created: front_matter::get(&current.text, "created"),
            hash: current.hash,
            path,
        }),
        (Some(_), Some(_)) => Err((SkipReason::NotNewer, Some(path))),
        _ => Err((SkipReason::NoModifiedTime, Some(path))),
    })
}

/// The folder of the note at `path` (relative to `notes/`).
fn folder_of(path: &str) -> &str {
    path.rsplit_once('/').map_or("", |(folder, _)| folder)
}

/// Checks that `input`, the file or folder chosen for an import, is neither
/// in `vault` nor contains it: importing a vault's own notes would add them
/// a second time.
///
/// # Errors
/// [`ImportError::Source`] if it is.
pub fn check_input(vault: &Vault, input: &std::path::Path) -> Result<(), ImportError> {
    let canonical = |p: &std::path::Path| p.canonicalize().unwrap_or_else(|_| p.to_path_buf());
    let root = canonical(vault.root());
    let input = canonical(input);
    if input.starts_with(&root) {
        return Err(ImportError::Source(
            "This is in the open vault. Choose files from outside it; the vault's notes are \
             there already."
                .to_owned(),
        ));
    }
    if root.starts_with(&input) {
        return Err(ImportError::Source(
            "This folder contains the open vault. Choose a folder outside it.".to_owned(),
        ));
    }
    Ok(())
}

/// The dry run: what importing `batch` into `vault` would do. Nothing is
/// written. `env` supplies the import time for notes without dates and
/// their new ids.
///
/// # Errors
/// [`ImportError::Vault`] if the import logs cannot be listed, or no id can
/// be generated.
pub fn plan(vault: &Vault, batch: &Batch, env: &dyn Env) -> Result<Plan, ImportError> {
    let (before, mut notices) = imported_before(vault, batch.source)?;
    notices.splice(0..0, batch.notices.iter().cloned());
    let mut planner = Planner {
        vault,
        env,
        source: batch.source,
        now: rfc3339_utc(env.now_ms()),
        before,
        seen: HashSet::new(),
        reserved: HashSet::new(),
    };
    let mut items = Vec::new();
    let mut prepared = Vec::new();
    for note in &batch.notes {
        let (item, ready) = planner.note(note)?;
        if let Some(mut ready) = ready {
            ready.item = items.len();
            prepared.push(ready);
        }
        items.push(item);
    }
    Ok(Plan {
        source: batch.source,
        input: batch.input.clone(),
        items,
        notices,
        prepared,
    })
}

/// What [`plan`] keeps track of across the notes of a batch.
struct Planner<'a> {
    vault: &'a Vault,
    env: &'a dyn Env,
    source: Source,
    /// The import time, for notes without dates.
    now: String,
    /// The latest log entry per `source_id`.
    before: HashMap<String, LogEntry>,
    /// The `source_id`s planned so far.
    seen: HashSet<String>,
    /// The paths (lowercase) planned for new notes so far.
    reserved: HashSet<String>,
}

impl Planner<'_> {
    /// A free path for a new note titled `title` in `folder`, not taken by
    /// a note planned before.
    fn reserve(&mut self, folder: &str, title: &str) -> Result<String, ImportError> {
        let reserved = &self.reserved;
        let path = self
            .vault
            .free_note_path(folder, title, |p| reserved.contains(p))?;
        self.reserved.insert(path.to_lowercase());
        Ok(path)
    }

    /// The report line for `note`, and what to write unless it is skipped.
    fn note(&mut self, note: &ImportedNote) -> Result<(PlanItem, Option<Prepared>), ImportError> {
        let body = prepare::clean(&note.body);
        let title = if note.title.trim().is_empty() {
            schnipsle_core::vault::title_of(&body, "note")
        } else {
            note.title.trim().to_owned()
        };
        let mut item = PlanItem {
            source_id: note.source_id.clone(),
            title: title.clone(),
            action: PlannedAction::Skip,
            reason: None,
            path: None,
            problems: Vec::new(),
            trashed: false,
        };
        if !self.seen.insert(note.source_id.clone()) {
            item.reason = Some(SkipReason::Duplicate);
            return Ok((item, None));
        }
        let problems = &mut item.problems;
        let created = time_or_now(note.created.as_deref(), "creation", &self.now, problems);
        let modified = time_or_now(
            note.modified.as_deref(),
            "modification",
            &self.now,
            problems,
        );
        problems.extend(prepare::code_like_tags_problem(&body));
        let source_modified = note
            .modified
            .as_deref()
            .filter(|m| parse_rfc3339(m).is_some())
            .map(str::to_owned);

        // An update (§12.3) or a new note: (where, id, created, hash now).
        let (folder, id, created, expected_hash) =
            if let Some(entry) = self.before.get(&note.source_id) {
                match update_target(self.vault, entry, source_modified.as_deref())? {
                    Ok(target) => {
                        item.action = PlannedAction::Update;
                        item.path = Some(target.path.clone());
                        if note.trashed {
                            item.problems.push(TRASHED_IN_SOURCE.to_owned());
                        }
                        let created = target.created.unwrap_or(created);
                        (target.path, entry.id.clone(), created, Some(target.hash))
                    }
                    Err((reason, path)) => {
                        item.reason = Some(reason);
                        item.path = path;
                        return Ok((item, None));
                    }
                }
            } else {
                let folder = prepare::placement(self.source, &note.folders);
                item.action = PlannedAction::Add;
                item.path = Some(self.reserve(&folder, &title)?);
                item.trashed = note.trashed;
                (folder, self.env.new_id()?, created, None)
            };

        // Links relative to the note's folder: for an update, its own.
        let link_folder = if expected_hash.is_some() {
            folder_of(&folder)
        } else {
            &folder
        };
        let keys = Keys {
            id: &id,
            created: &created,
            modified: &modified,
            pinned: note.pinned,
            source: self.source,
            source_id: &note.source_id,
        };
        let (text, attachments) = file_text(note, &body, link_folder, &keys, &mut item.problems);
        let entry = LogEntry {
            source_id: note.source_id.clone(),
            id,
            path: format!("notes/{}", item.path.as_deref().unwrap_or_default()),
            action: if expected_hash.is_some() {
                LogAction::Updated
            } else {
                LogAction::Added
            },
            source_modified,
            hash: schnipsle_core::vault::sha256_hex(text.as_bytes()),
            replaced: None,
            kept: false,
        };
        let ready = Prepared {
            item: 0,
            folder,
            title,
            text,
            attachments,
            entry,
            trash: item.trashed,
            expected_hash,
        };
        Ok((item, Some(ready)))
    }
}

/// A note the import wrote. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Written {
    /// Its path relative to `notes/`.
    pub path: String,
    /// Added or updated.
    pub action: LogAction,
}

/// What [`apply`] did. Serialized with camelCase names.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    /// The run's id, which names its log; `None` if nothing was written.
    pub run_id: Option<String>,
    /// The report, updated: final paths, and notes that could not be
    /// written after all (skipped with [`SkipReason::ChangedMeanwhile`] or
    /// [`SkipReason::Failed`]).
    pub plan: Plan,
    /// The notes written into `notes/`, for the index update; not those
    /// moved to the trash.
    pub written: Vec<Written>,
    /// The notes moved to the trash (§12.1 rule 6), by their names in it
    /// (format.md §8), for the index update.
    pub trashed: Vec<String>,
}

/// Writes what `plan` says (see the crate documentation). A note that fails
/// is reported in the outcome and the others are still written; a failure
/// to write the import log stops the import, so no note is left out of it.
///
/// # Errors
/// [`ImportError::Log`] if the import log cannot be written; the notes
/// written up to then are in the log.
pub fn apply(vault: &Vault, plan: Plan, env: &dyn Env) -> Result<Outcome, ImportError> {
    apply_with_progress(vault, plan, env, &mut |_, _| ())
}

/// [`apply`], calling `progress(done, total)` after each note it wrote or
/// skipped, for a progress bar.
///
/// # Errors
/// As [`apply`].
pub fn apply_with_progress(
    vault: &Vault,
    mut plan: Plan,
    env: &dyn Env,
    progress: &mut dyn FnMut(usize, usize),
) -> Result<Outcome, ImportError> {
    let prepared = std::mem::take(&mut plan.prepared);
    if prepared.is_empty() {
        return Ok(Outcome {
            run_id: None,
            plan,
            written: Vec::new(),
            trashed: Vec::new(),
        });
    }
    let run_id = env.new_id()?;
    let mut log = ImportLog::new(
        &run_id,
        plan.source.id(),
        &plan.input,
        &rfc3339_utc(env.now_ms()),
    );
    vault.write_import_log(&mut log)?;
    let mut written = Vec::new();
    let mut trashed = Vec::new();
    let mut done = 0;
    let mut position = 0; // of the next entry in the log
    for chunk in prepared.chunks(LOG_BATCH) {
        for p in chunk {
            log.push(&p.entry);
        }
        vault.write_import_log(&mut log)?;
        for p in chunk {
            let result = write_one(vault, p);
            let item = &mut plan.items[p.item];
            match result {
                Ok(entry) => {
                    item.path = entry.path.strip_prefix("notes/").map(str::to_owned);
                    let path = item.path.clone().unwrap_or_default();
                    // Logged as added either way: undo finds it in the trash
                    // already, and a re-import counts it as deleted here.
                    let in_trash = if p.trash {
                        match vault.trash_note(&path) {
                            Ok(t) => Some(t.key),
                            Err(e) => {
                                item.trashed = false;
                                item.problems.push(format!(
                                    "It could not be moved to the trash ({e}), so it is with the \
                                     other notes."
                                ));
                                None
                            }
                        }
                    } else {
                        None
                    };
                    match in_trash {
                        Some(key) => trashed.push(key),
                        None => written.push(Written {
                            path,
                            action: entry.action,
                        }),
                    }
                    log.update(position, &entry);
                    position += 1;
                }
                Err(reason) => {
                    item.action = PlannedAction::Skip;
                    item.reason = Some(reason.0);
                    item.problems.extend(reason.1);
                    log.remove(position);
                }
            }
            done += 1;
            progress(done, prepared.len());
        }
        vault.write_import_log(&mut log)?;
    }
    log.finish(&rfc3339_utc(env.now_ms()));
    vault.write_import_log(&mut log)?;
    Ok(Outcome {
        run_id: Some(run_id),
        plan,
        written,
        trashed,
    })
}

/// Writes one note with its attachments; returns its log entry as written.
fn write_one(vault: &Vault, p: &Prepared) -> Result<LogEntry, (SkipReason, Option<String>)> {
    let failed = |e: &dyn fmt::Display| (SkipReason::Failed, Some(e.to_string()));
    let note_folder = match &p.expected_hash {
        Some(_) => folder_of(&p.folder),
        None => &p.folder,
    };
    let note_path = format!("{note_folder}/note.md");
    for (name, bytes) in &p.attachments {
        vault
            .add_attachment(note_path.trim_start_matches('/'), name, bytes)
            .map_err(|e| failed(&e))?;
    }
    let mut entry = p.entry.clone();
    if let Some(expected) = &p.expected_hash {
        let current = vault.read_note_text(&p.folder).map_err(|e| failed(&e))?;
        if &current.hash != expected {
            return Err((SkipReason::ChangedMeanwhile, None));
        }
        let snapshot = vault
            .snapshot(&entry.id, current.text.as_bytes())
            .map_err(|e| failed(&e))?;
        let name = snapshot.rsplit('/').next().unwrap_or_default();
        entry.replaced = Some(name.trim_end_matches(".md.zst").to_owned());
        match vault.replace_note_as_is(&p.folder, &p.text, expected) {
            Ok(_) => {}
            Err(VaultError::Conflict { .. }) => return Err((SkipReason::ChangedMeanwhile, None)),
            Err(e) => return Err(failed(&e)),
        }
        entry.path = format!("notes/{}", p.folder);
    } else {
        let note = vault
            .create_titled_as_is(&p.folder, &p.title, &p.text)
            .map_err(|e| failed(&e))?;
        entry.path = format!("notes/{}", note.path);
    }
    Ok(entry)
}
