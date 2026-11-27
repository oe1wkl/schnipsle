// SPDX-License-Identifier: Apache-2.0
//! The search index of the open vault (ADR-0009): where its file lives, the
//! sync when a vault opens, and updates after changes.
//!
//! The index is a cache in the app's cache folder, one file per vault id. It
//! is synced in a background thread when the vault opens. After that, change
//! events and the app's own writes update single notes. A save never waits
//! for a running sync: paths that arrive while the index is busy are queued
//! and handled when the sync ends.
//!
//! A snapshot of what the index knows per note is kept beside it, taken when
//! it opens and after each sync, so the list and the copy search can skip
//! unchanged files even while a sync holds the index (ADR-0046).

use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Instant;

use schnipsle_core::vault::sha256_hex;
use schnipsle_core::watch::VaultChanges;
use schnipsle_core::{Index, KnownNotes, NotesById, Vault};
use tauri::{AppHandle, Emitter as _, Manager as _};

/// The index and the root of the vault it belongs to.
#[derive(Debug)]
struct Open {
    root: PathBuf,
    index: Index,
}

/// What the index of the vault at a root knew per note (ADR-0046).
type Snapshot = Option<(PathBuf, Arc<KnownNotes>)>;

/// Work that needs the synced index of a vault, see [`SharedIndex::after_sync`].
pub type Job = Box<dyn FnOnce(&Vault, &mut Index) + Send>;

/// Jobs waiting for the sync of the vault at their root.
#[derive(Default)]
struct Jobs(Vec<(PathBuf, Job)>);

impl std::fmt::Debug for Jobs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list()
            .entries(self.0.iter().map(|(root, _)| root))
            .finish()
    }
}

/// The index of the open vault, shared between commands, the watcher and the
/// background sync. Cheap to clone.
#[derive(Debug, Clone, Default)]
pub struct SharedIndex {
    open: Arc<Mutex<Option<Open>>>,
    /// Paths changed while the index was busy, by vault root.
    pending: Arc<Mutex<Vec<(PathBuf, String)>>>,
    /// What the index knew per note when it opened or last synced, by vault
    /// root (ADR-0046).
    known: Arc<Mutex<Snapshot>>,
    /// Run once the next sync of their vault has ended well.
    jobs: Arc<Mutex<Jobs>>,
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    // A panic while indexing leaves a usable cache at worst; keep going.
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

impl SharedIndex {
    /// Opens the index of `vault` and syncs it in the background. Failures
    /// leave the app without an index, which only search needs.
    pub fn open_for(&self, app: &AppHandle, vault: &Vault) {
        *lock(&self.known) = None;
        let file = match index_file(app, vault) {
            Ok(file) => file,
            Err(e) => {
                eprintln!("no search index: {e}");
                self.drop_jobs(vault);
                return;
            }
        };
        let index = match Index::open(&file) {
            Ok(index) => index,
            Err(e) => {
                eprintln!("no search index: {e}");
                self.drop_jobs(vault);
                return;
            }
        };
        self.remember_known(vault, &index);
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
                    let start = Instant::now();
                    let report = vault.sync_index(&mut open.index);
                    if report.is_ok() {
                        for job in shared.take_jobs(&vault) {
                            job(&vault, &mut open.index);
                        }
                        shared.remember_known(&vault, &open.index);
                    } else {
                        shared.drop_jobs(&vault);
                    }
                    if let Ok(r) = &report {
                        let notes = open.index.len().unwrap_or_default();
                        crate::measuring::record(
                            "indexSync",
                            start.elapsed(),
                            crate::measuring::Detail {
                                read: Some(r.indexed),
                                removed: Some(r.removed),
                                ..crate::measuring::notes(notes)
                            },
                        );
                    }
                    report
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

    /// Runs `job` with the index of the vault at `root` once its next sync
    /// has ended well, before the UI hears of it (`index-synced`). Register
    /// it before the vault is opened. A job whose sync fails is dropped.
    pub fn after_sync(&self, root: PathBuf, job: Job) {
        lock(&self.jobs).0.push((root, job));
    }

    /// Removes and returns the jobs waiting for `vault`.
    fn take_jobs(&self, vault: &Vault) -> Vec<Job> {
        let mut jobs = lock(&self.jobs);
        let (mine, others): (Vec<_>, Vec<_>) = std::mem::take(&mut jobs.0)
            .into_iter()
            .partition(|(root, _)| root == vault.root());
        jobs.0 = others;
        mine.into_iter().map(|(_, job)| job).collect()
    }

    fn drop_jobs(&self, vault: &Vault) {
        let dropped = self.take_jobs(vault).len();
        if dropped > 0 {
            eprintln!(
                "{dropped} jobs dropped: the index of {} is not there",
                vault.root().display()
            );
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

    /// Takes the snapshot of what `index` knows about the notes of `vault`.
    fn remember_known(&self, vault: &Vault, index: &Index) {
        let prefix = format!("{}/", schnipsle_core::layout::NOTES_DIR);
        let known = match index.known_notes() {
            Ok(known) => known
                .into_iter()
                .filter_map(|(key, note)| Some((key.strip_prefix(&prefix)?.to_owned(), note)))
                .collect(),
            Err(e) => {
                eprintln!("index snapshot not taken: {e}");
                *lock(&self.known) = None;
                return;
            }
        };
        *lock(&self.known) = Some((vault.root().to_owned(), Arc::new(known)));
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
/// and the copies in the note's own folder are still found. Also hands out
/// the snapshot of what the index knows (ADR-0046), which a sync never blocks.
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

    fn known_notes(&self) -> Option<Arc<KnownNotes>> {
        lock(&self.shared.known)
            .as_ref()
            .filter(|(root, _)| *root == self.root)
            .map(|(_, known)| Arc::clone(known))
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
