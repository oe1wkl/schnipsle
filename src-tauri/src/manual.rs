// SPDX-License-Identifier: Apache-2.0
//! The manual the app carries and adds to a vault (ADR-0048 rules 6 and 7,
//! ADR-0051): at the first start, and from Help › Manual.

use std::collections::HashSet;

use schnipsle_core::manual::{Manual, ManualAdded};
use schnipsle_core::{Index, Vault};
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, State};

use crate::indexing::note_key;
use crate::{AppState, CmdResult, reindex, with_vault};

/// The files of `docs/manual/`, built into the app by `build.rs`.
mod files {
    include!(concat!(env!("OUT_DIR"), "/manual.rs"));
}

/// The manual as the app carries it.
pub fn shipped() -> schnipsle_core::vault::Result<Manual> {
    Manual::from_files(files::FILES.iter().copied())
}

/// What adding the manual did, and where its book note is now.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Added {
    #[serde(flatten)]
    pub done: ManualAdded,
    /// The book note, relative to `notes/`, if the vault has one now.
    pub book: Option<String>,
}

/// The ids of the manual notes that `index` knows anywhere in the vault.
fn known_ids(manual: &Manual, index: &Index) -> HashSet<String> {
    manual
        .notes
        .iter()
        .filter(|n| index.paths_with_id(&n.id).is_ok_and(|p| !p.is_empty()))
        .map(|n| n.id.clone())
        .collect()
}

/// Adds the missing manual notes once the index of `vault` has synced, so
/// that notes the user moved are found by their id; then tells the UI
/// (`manual-added`). For a vault that was there already at the first start.
pub fn add_after_sync(app: &AppHandle, state: &AppState, vault: &Vault) {
    let app = app.clone();
    state.index.after_sync(
        vault.root().to_owned(),
        Box::new(move |vault, index| {
            let manual = match shipped() {
                Ok(manual) => manual,
                Err(e) => {
                    eprintln!("manual not added: {e}");
                    return;
                }
            };
            let known = known_ids(&manual, index);
            match vault.add_manual(&manual, |id| known.contains(id)) {
                Ok(done) => {
                    for path in &done.added {
                        if let Err(e) = vault.index_file(index, &note_key(path)) {
                            eprintln!("not indexed: {path}: {e}");
                        }
                    }
                    let added = Added {
                        done,
                        book: vault.manual_book(),
                    };
                    let _ = app.emit("manual-added", added);
                }
                Err(e) => eprintln!("manual not added: {e}"),
            }
        }),
    );
}

/// Adds the whole manual to a vault that was just made: nothing in it can
/// have a manual note's id. Failures are only logged; the vault opens anyway.
pub fn add_to_new(vault: &Vault) {
    match shipped().and_then(|manual| vault.add_manual(&manual, |_| false)) {
        Ok(done) => eprintln!("manual added: {} notes", done.added.len()),
        Err(e) => eprintln!("manual not added: {e}"),
    }
}

/// Help › Manual: the path (relative to `notes/`) of the open vault's book
/// note, or `None` if it has none, and the UI offers [`add_manual`].
#[tauri::command]
pub async fn open_manual(state: State<'_, AppState>) -> CmdResult<Option<String>> {
    with_vault(&state, |v| Ok(v.manual_book()))
}

/// Adds the missing manual notes to the open vault, after the user agreed.
#[tauri::command]
pub async fn add_manual(state: State<'_, AppState>) -> CmdResult<Added> {
    with_vault(&state, |v| {
        let manual = shipped()?;
        let known = state
            .index
            .read(v, |index| known_ids(&manual, index))
            .unwrap_or_default();
        let done = v.add_manual(&manual, |id| known.contains(id))?;
        let keys: Vec<String> = done.added.iter().map(|p| note_key(p)).collect();
        reindex(&state, v, &keys);
        Ok(Added {
            done,
            book: v.manual_book(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_built_in_manual_is_the_repository_s() {
        let manual = shipped().unwrap();
        assert!(
            manual
                .notes
                .iter()
                .any(|n| n.id == schnipsle_core::manual::BOOK_ID)
        );
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../docs/manual");
        assert_eq!(manual, Manual::load(&root).unwrap());
    }
}
