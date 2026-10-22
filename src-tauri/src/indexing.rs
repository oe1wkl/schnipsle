// SPDX-License-Identifier: Apache-2.0
//! The search index of the open vault (ADR-0009): where its file lives, the
//! sync when a vault opens, and updates after changes.
//!
//! The index is a cache in the app's cache folder, one file per vault id. It
//! is synced in a background thread when the vault opens. After that, change
//! events and the app's own writes update single notes. A save never waits
//! for a running sync: paths that arrive while the index is busy are queued
//! and handled when the sync ends.

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use schnipsle_core::vault::sha256_hex;
use schnipsle_core::watch::VaultChanges;
use schnipsle_core::{Index, NotesById, Vault};
use tauri::{AppHandle, Emitter as _, Manager as _};

/// The index and the root of the vault it belongs to.
#[derive(Debug)]
struct Open {
    root: PathBuf,
    index: Index,
}

/// The index of the open vault, shared between commands, the watcher and the
/// background sync. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct SharedIndex {
    open: Arc<Mutex<Option<Open>>>,
    /// Paths changed while the index was busy, by vault root.
    pending: Arc<Mutex<Vec<(PathBuf, String)>>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while indexing leaves a usable cache at worst; keep going.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SharedIndex {
    /// Opens the index of `vault` and syncs it in the background. Failures
    /// leave the app without an index, which only search needs.
    pub fn open_for(&self, app: &AppHandle, vault: &Vault) {
        let file = match index_file(app, vault) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("no search index: {e}");
                return;
            }
        };
        let index = match Index::open(&file) {
            Ok(index) => index,
            Err(e) => {
                eprintln!("no search index: {e}");
                return;
            }
        };
        *lock(&self.open) = Some(Open {
            root: vault.root().to_owned(),
            index,
        });
        lock(&self.pending).clear();
        self.sync_in_background(app, vault);
    }

    /// Compares the whole index with the vault, in a background thread.
    pub fn sync_in_background(&self, app: &AppHandle, vault: &Vault) {
        let shared = self.clone();
        let vault = vault.clone();
        let app = app.clone();
        let spawned = std::thread::Builder::new()
            .name("index-sync".into())
            .spawn(move || {
                let report = {
                    let mut open = lock(&shared.open);
                    let Some(open) = open.as_mut().filter(|o| o.root == vault.root()) else {
                        return; // another vault was opened meanwhile
                    };
                    vault.sync_index(&mut open.index)
                };
                match report {
                    Ok(report) => {
                        shared.drain_pending(&vault);
                        let _ = app.emit("index-synced", report);
                    }
                    Err(e) => eprintln!("search index not synced: {e}"),
                }
            });
        if let Err(e) = spawned {
            eprintln!("search index not synced: {e}");
        }
    }

    /// Runs `f` on the index of `vault`, waiting for a running sync; `None` if
    /// there is no index for it.
    pub fn read<T>(&self, vault: &Vault, f: impl FnOnce(&Index) -> T) -> Option<T> {
        let open = lock(&self.open);
        open.as_ref()
            .filter(|o| o.root == vault.root())
            .map(|o| f(&o.index))
    }

    /// Updates the given paths (below the vault root) now, or after the sync
    /// that is running. Paths queued earlier for this vault come along.
    pub fn update(&self, vault: &Vault, keys: &[String]) {
        let Ok(mut open) = self.open.try_lock() else {
            let mut pending = lock(&self.pending);
            pending.extend(keys.iter().map(|k| (vault.root().to_owned(), k.clone())));
            return;
        };
        let Some(open) = open.as_mut().filter(|o| o.root == vault.root()) else {
            return;
        };
        let mut keys = keys.to_vec();
        keys.extend(self.take_pending(vault));
        for key in &keys {
            if let Err(e) = vault.index_file(&mut open.index, key) {
                eprintln!("not indexed: {key}: {e}");
            }
        }
    }

    /// Applies a burst of change events: single notes, or a full sync when
    /// events may have been lost.
    pub fn apply(&self, app: &AppHandle, vault: &Vault, changes: &VaultChanges) {
        if changes.rescan {
            self.sync_in_background(app, vault);
        } else {
            let keys: Vec<String> = changes.paths.iter().map(|p| note_key(p)).collect();
            self.update(vault, &keys);
        }
    }

    fn drain_pending(&self, vault: &Vault) {
        self.update(vault, &[]);
    }

    /// Removes and returns the paths queued for `vault`.
    fn take_pending(&self, vault: &Vault) -> Vec<String> {
        let mut pending = lock(&self.pending);
        let (mine, others): (Vec<_>, Vec<_>) = pending
            .drain(..)
            .partition(|(root, _)| root == vault.root());
        *pending = others;
        mine.into_iter().map(|(_, key)| key).collect()
    }
}

/// Finds copies of a note in other folders (format.md §4) through the index
/// of the vault at `root`. While a sync holds the index it answers nothing,
/// and the copies in the note's own folder are still found.
#[derive(Debug)]
pub struct IndexIds {
    shared: SharedIndex,
    root: PathBuf,
}

impl IndexIds {
    pub fn new(shared: SharedIndex, root: PathBuf) -> Self {
        Self { shared, root }
    }
}

impl NotesById for IndexIds {
    fn paths_with_id(&self, id: &str) -> Vec<String> {
        let Ok(open) = self.shared.open.try_lock() else {
            return Vec::new();
        };
        let prefix = format!("{}/", schnipsle_core::layout::NOTES_DIR);
        open.as_ref()
            .filter(|o| o.root == self.root)
            .and_then(|o| o.index.paths_with_id(id).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(|key| key.strip_prefix(&prefix).map(str::to_owned))
            .collect()
    }
}

/// The index key of a note path (relative to `notes/`).
pub fn note_key(path: &str) -> String {
    format!("{}/{path}", schnipsle_core::layout::NOTES_DIR)
}

/// The index key of a trashed note.
pub fn trash_key(key: &str) -> String {
    format!("{}/{key}.md", schnipsle_core::trash::TRASH_DIR)
}

/// `<app cache>/index/<vault id>.sqlite`; a vault with a damaged
/// `vault.yaml` is keyed by its folder instead.
fn index_file(app: &AppHandle, vault: &Vault) -> Result<PathBuf, String> {
    let dir = app.path().app_cache_dir().map_err(|e| e.to_string())?;
    let name = vault.identity().map_or_else(
        || {
            let hash = sha256_hex(vault.root().to_string_lossy().as_bytes());
            format!("path-{}", &hash[..16])
        },
        |identity| identity.id.clone(),
    );
    Ok(dir.join("index").join(format!("{name}.sqlite")))
}
