// SPDX-License-Identifier: Apache-2.0
//! Renaming and merging tags (F-11, ADR-0040) as commands: a plan for the
//! user to confirm, the rename, and its undo while the app runs.

use std::collections::HashSet;
use std::path::PathBuf;
use std::sync::Mutex;

use schnipsle_core::{
    InvalidRename, RetagOutcome, RetagPlan, RetagUndo, SortOrder, TagRename, Vault,
};
use tauri::State;

use crate::{AppState, CmdResult, CommandError, indexing, reindex, with_vault};

/// The last rename, which Undo puts back, with the vault it was made in.
#[derive(Default)]
pub struct LastRename(Mutex<Option<(PathBuf, RetagUndo)>>);

impl From<InvalidRename> for CommandError {
    fn from(e: InvalidRename) -> Self {
        let message = e.to_string();
        let (kind, value) = match e {
            InvalidRename::NotATag(name) if name.is_empty() => ("emptyTag", None),
            InvalidRename::NotATag(name) => ("notATag", Some(name)),
            InvalidRename::IntoItself => ("tagIntoItself", None),
        };
        Self {
            kind,
            message,
            current_hash: None,
            value,
        }
    }
}

/// The notes under `key` and its subtags, from the index; every note if the
/// index cannot say, so a rename never misses one.
fn notes_under(state: &AppState, vault: &Vault, key: &str) -> CmdResult<Vec<String>> {
    let indexed = state.index.read(vault, |i| {
        i.notes_in(&[key.to_owned()], SortOrder::default())
    });
    let notes = match indexed {
        Some(Ok(notes)) => notes,
        _ => vault.list_notes()?,
    };
    Ok(notes.into_iter().map(|n| n.path).collect())
}

/// What renaming the tag `old` to `new` changes (ADR-0040 rule 5).
#[tauri::command]
pub async fn plan_tag_rename(
    state: State<'_, AppState>,
    old: String,
    new: String,
) -> CmdResult<RetagPlan> {
    let rename = TagRename::new(&old, &new)?;
    with_vault(&state, |v| {
        let paths = notes_under(&state, v, rename.old_key())?;
        let merge = rename.new_key() != rename.old_key() && {
            let own: HashSet<&String> = paths.iter().collect();
            notes_under(&state, v, rename.new_key())?
                .iter()
                .any(|path| !own.contains(path))
        };
        Ok(v.plan_retag(&rename, &paths, merge)?)
    })
}

/// Renames as `plan` says; Undo can put it back until the vault closes.
#[tauri::command]
pub async fn rename_tag(state: State<'_, AppState>, plan: RetagPlan) -> CmdResult<RetagOutcome> {
    with_vault(&state, |v| {
        let (outcome, undo) = v.apply_retag(&plan)?;
        let keys: Vec<String> = outcome
            .changed
            .iter()
            .map(|p| indexing::note_key(p))
            .collect();
        reindex(&state, v, &keys);
        *state.last_rename.0.lock().map_err(CommandError::other)? =
            (!undo.is_empty()).then(|| (v.root().to_owned(), undo));
        Ok(outcome)
    })
}

/// Undoes the last rename in the open vault (ADR-0040 rule 7).
#[tauri::command]
pub async fn undo_tag_rename(state: State<'_, AppState>) -> CmdResult<RetagOutcome> {
    with_vault(&state, |v| {
        let last = state
            .last_rename
            .0
            .lock()
            .map_err(CommandError::other)?
            .take();
        let Some((root, undo)) = last.filter(|(root, _)| root == v.root()) else {
            return Err(CommandError::other("There is no rename to undo"));
        };
        let outcome = v.undo_retag(&undo);
        if outcome.is_err() {
            // Nothing was written; the rename can still be undone.
            *state.last_rename.0.lock().map_err(CommandError::other)? = Some((root, undo));
        }
        let outcome = outcome?;
        let keys: Vec<String> = outcome
            .changed
            .iter()
            .map(|p| indexing::note_key(p))
            .collect();
        reindex(&state, v, &keys);
        Ok(outcome)
    })
}
