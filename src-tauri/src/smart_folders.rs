// SPDX-License-Identifier: Apache-2.0
//! Smart folders (F-10, format.md §9, ADR-0018) as commands: each edit is one
//! operation that the core applies to the current file.

use schnipsle_core::{FolderSort, SearchHit, SmartFolder, SmartFolderError, SmartFolders};
use serde::Serialize;
use tauri::State;

use crate::{AppState, CmdResult, CommandError, with_index, with_vault};

impl From<SmartFolderError> for CommandError {
    fn from(e: SmartFolderError) -> Self {
        let message = e.to_string();
        let kind = match e {
            SmartFolderError::Vault(v) => return v.into(),
            SmartFolderError::Damaged(_) => "damaged",
            SmartFolderError::NewerFormat(_) => "newerFormat",
            SmartFolderError::UnknownFolder(_) => "unknownFolder",
            SmartFolderError::InvalidParent(_) => "invalidParent",
            SmartFolderError::Empty(_) => "empty",
            SmartFolderError::KeepsChanging => "keepsChanging",
        };
        Self {
            kind,
            message,
            current_hash: None,
        }
    }
}

/// The smart folders, depth first, and why they cannot be changed, if so.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolderList {
    folders: Vec<SmartFolder>,
    read_only: Option<String>,
}

#[tauri::command]
pub async fn list_smart_folders(state: State<'_, AppState>) -> CmdResult<SmartFolderList> {
    with_vault(&state, |v| {
        let folders = v.smart_folders()?;
        let read_only = v.read_only().map(str::to_owned).or_else(|| {
            (folders.format() > schnipsle_core::smart_folders::SMART_FOLDERS_FORMAT)
                .then(|| SmartFolderError::NewerFormat(folders.format()).to_string())
        });
        Ok(SmartFolderList {
            folders: folders.list(),
            read_only,
        })
    })
}

/// Adds a folder at the end of `parent` (the top level for `None`); returns its id.
#[tauri::command]
pub async fn add_smart_folder(
    state: State<'_, AppState>,
    name: String,
    query: String,
    parent: Option<String>,
    sort: FolderSort,
) -> CmdResult<String> {
    with_vault(&state, |v| {
        Ok(v.edit_smart_folders(|f| f.add(&name, &query, parent.as_deref(), sort))?)
    })
}

/// Saves the edit form: name, query and sort, and moves the folder to the
/// end of `parent` if that is not where it is.
#[tauri::command]
pub async fn update_smart_folder(
    state: State<'_, AppState>,
    id: String,
    name: String,
    query: String,
    parent: Option<String>,
    sort: FolderSort,
) -> CmdResult<()> {
    with_vault(&state, |v| {
        Ok(v.edit_smart_folders(|f| {
            f.update(&id, Some(&name), Some(&query), Some(sort))?;
            if parent_of(f, &id) != parent {
                f.move_to(&id, parent.as_deref(), usize::MAX)?;
            }
            Ok(())
        })?)
    })
}

fn parent_of(folders: &SmartFolders, id: &str) -> Option<String> {
    folders
        .list()
        .into_iter()
        .find(|f| f.id == id)
        .and_then(|f| f.parent)
}

/// Removes a folder; its subfolders move up to its parent.
#[tauri::command]
pub async fn remove_smart_folder(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    with_vault(&state, |v| Ok(v.edit_smart_folders(|f| f.remove(&id))?))
}

/// Drag and drop: moves a folder into `parent` at `index` among its
/// subfolders (not counting the folder itself).
#[tauri::command]
pub async fn move_smart_folder(
    state: State<'_, AppState>,
    id: String,
    parent: Option<String>,
    index: usize,
) -> CmdResult<()> {
    with_vault(&state, |v| {
        Ok(v.edit_smart_folders(|f| f.move_to(&id, parent.as_deref(), index))?)
    })
}

/// The notes of a smart folder: its query, pinned notes first, then in its
/// order. `utc_offset_minutes` as for `search_notes`.
#[tauri::command]
pub async fn smart_folder_notes(
    state: State<'_, AppState>,
    query: String,
    sort: FolderSort,
    utc_offset_minutes: i32,
) -> CmdResult<Vec<SearchHit>> {
    with_index(&state, |i| {
        i.search_sorted(&query, sort, utc_offset_minutes)
    })
}
