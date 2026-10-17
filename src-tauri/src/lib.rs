// SPDX-License-Identifier: Apache-2.0
//! Schnipsle app shell: exposes `schnipsle-core` to the UI as Tauri commands.
//!
//! The UI never touches vault files; every file operation goes through a command
//! here (ADR-0003). Commands that may show system UI or wait for iOS are `async`,
//! so they run off the main thread.

mod vault_location;
mod watching;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use schnipsle_core::layout::VAULT_FILE;
use schnipsle_core::{
    Coordinator, Folder, MovedNotes, NewNote, NoteContent, NoteEntry, SavedNote, SortOrder,
    StrayNotes, Tag, TrashedNote, Vault, VaultError,
};
use serde::Serialize;
use tauri::State;

/// The currently opened vault, if any, and the watch reporting its changes.
#[derive(Default)]
struct AppState {
    vault: Mutex<Option<Vault>>,
    watch: Mutex<Option<watching::VaultWatch>>,
    /// A picked folder that is not a vault yet; [`create_vault`] makes it one
    /// once the user agreed.
    pending: Mutex<Option<PathBuf>>,
}

/// Error sent to the UI: a machine-readable `kind` plus a message.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct CommandError {
    kind: &'static str,
    message: String,
    /// For conflicts: the hash now on disk, to overwrite deliberately.
    current_hash: Option<String>,
}

impl CommandError {
    // Takes ownership so it can be used as `.map_err(CommandError::other)`.
    #[allow(clippy::needless_pass_by_value)]
    fn other(message: impl ToString) -> Self {
        Self {
            kind: "other",
            message: message.to_string(),
            current_hash: None,
        }
    }
}

impl From<VaultError> for CommandError {
    fn from(e: VaultError) -> Self {
        let message = e.to_string();
        let (kind, current_hash) = match e {
            VaultError::Conflict { current_hash, .. } => ("conflict", current_hash),
            VaultError::NotDownloaded(_) => ("notDownloaded", None),
            VaultError::AlreadyExists(_) => ("alreadyExists", None),
            VaultError::InvalidPath(_) => ("invalidPath", None),
            VaultError::NotADirectory(_) => ("notADirectory", None),
            VaultError::NotAVault(_) => ("notAVault", None),
            VaultError::InsideVault(_) => ("insideVault", None),
            VaultError::ReadOnly(_) => ("readOnly", None),
            VaultError::InvalidId(_) => ("invalidId", None),
            VaultError::Io(_) => ("io", None),
        };
        Self {
            kind,
            message,
            current_hash,
        }
    }
}

type CmdResult<T> = Result<T, CommandError>;

/// What the UI needs to know about an opened vault.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct VaultInfo {
    name: String,
    path: String,
    /// Why the vault is read-only (format.md §2), if it is.
    read_only: Option<String>,
}

/// The result of picking or restoring a folder.
#[derive(Serialize)]
#[serde(
    tag = "kind",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
enum Opened {
    /// The folder is a vault and is now open.
    Vault(VaultInfo),
    /// The folder is not a vault yet. With the user's consent,
    /// [`create_vault`] moves `items` into `notes/` and opens it.
    Setup {
        name: String,
        path: String,
        items: Vec<String>,
        notes: usize,
        /// Items only in iCloud; their download has been requested.
        not_downloaded: Vec<String>,
    },
}

/// The platform's file coordination (ADR-0015).
fn coordinator() -> Arc<dyn Coordinator> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return Arc::new(schnipsle_apple::FileCoordinator);
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    return Arc::new(schnipsle_core::Uncoordinated);
}

/// Adds the platform's sync conflict versions (ADR-0006).
fn with_sync_conflicts(vault: Vault) -> Vault {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return vault.with_sync_conflicts(Arc::new(schnipsle_apple::ICloudVersions));
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    return vault;
}

fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into_owned(),
    )
}

/// Opens the folder at `path` if it is a vault (format.md §1), else tells the
/// UI what making it one would move. Never changes anything.
fn open_folder(app: &tauri::AppHandle, state: &AppState, path: &Path) -> CmdResult<Opened> {
    match Vault::open_with(path, coordinator()) {
        Ok(vault) => return Ok(Opened::Vault(remember(app, state, vault)?)),
        Err(VaultError::NotAVault(_)) => {}
        Err(e @ VaultError::NotDownloaded(_)) => {
            request_download(app, &path.join(VAULT_FILE));
            return Err(e.into());
        }
        Err(e) => return Err(e.into()),
    }
    match schnipsle_core::inspect(path, coordinator())? {
        Folder::NoVault {
            items,
            notes,
            not_downloaded,
        } => {
            for name in &not_downloaded {
                request_download(app, &path.join(name));
            }
            *state.pending.lock().map_err(CommandError::other)? = Some(path.to_owned());
            Ok(Opened::Setup {
                name: folder_name(path),
                path: path.display().to_string(),
                items,
                notes,
                not_downloaded,
            })
        }
        Folder::InsideVault { root } => Err(VaultError::InsideVault(root).into()),
        // vault.yaml arrived since the attempt above: open it.
        Folder::Vault | Folder::VaultNotDownloaded => Ok(Opened::Vault(remember(
            app,
            state,
            Vault::open_with(path, coordinator())?,
        )?)),
    }
}

/// Asks iCloud for a file; failures only mean the user has to wait longer.
fn request_download(app: &tauri::AppHandle, file: &Path) {
    if let Err(e) = vault_location::download(app, file) {
        eprintln!(
            "download of {} not requested: {}",
            file.display(),
            e.message
        );
    }
}

/// Makes `vault` the open vault and starts watching it.
fn remember(app: &tauri::AppHandle, state: &AppState, vault: Vault) -> CmdResult<VaultInfo> {
    let vault = with_sync_conflicts(vault);
    let path = vault.root();
    let info = VaultInfo {
        name: vault
            .identity()
            .and_then(|i| i.name.clone())
            .unwrap_or_else(|| folder_name(path)),
        path: path.display().to_string(),
        read_only: vault.read_only().map(str::to_owned),
    };
    *state.pending.lock().map_err(CommandError::other)? = None;
    // Stop watching the previous vault before the new one is reported.
    let mut watch = state.watch.lock().map_err(CommandError::other)?;
    *watch = None;
    // Without events the UI still re-checks when it becomes visible, so a
    // watch that fails to start must not keep the vault from opening.
    *watch = watching::start(app, &vault)
        .inspect_err(|e| eprintln!("vault changes will not be reported: {e}"))
        .ok();
    *state.vault.lock().map_err(CommandError::other)? = Some(vault);
    Ok(info)
}

fn with_vault<T>(state: &AppState, f: impl FnOnce(&Vault) -> CmdResult<T>) -> CmdResult<T> {
    // Clone out of the lock: file coordination can wait for iCloud, and that
    // must not hold up every other command.
    let vault = state
        .vault
        .lock()
        .map_err(CommandError::other)?
        .clone()
        .ok_or_else(|| CommandError {
            kind: "noVault",
            message: "No vault folder is open".into(),
            current_hash: None,
        })?;
    f(&vault)
}

/// Lets the user pick a vault folder; `None` if cancelled.
#[tauri::command]
async fn pick_vault(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Option<Opened>> {
    match vault_location::pick(&app)? {
        Some(path) => open_folder(&app, &state, &path).map(Some),
        None => Ok(None),
    }
}

/// Re-opens the vault used last time, if it is still reachable.
#[tauri::command]
async fn restore_vault(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Option<Opened>> {
    match vault_location::restore(&app)? {
        Some(path) => open_folder(&app, &state, &path).map(Some),
        None => Ok(None),
    }
}

/// Makes the folder last reported as [`Opened::Setup`] a vault, moving its
/// contents into `notes/`, and opens it. Call only after the user agreed.
#[tauri::command]
async fn create_vault(app: tauri::AppHandle, state: State<'_, AppState>) -> CmdResult<VaultInfo> {
    let Some(path) = state.pending.lock().map_err(CommandError::other)?.take() else {
        return Err(CommandError::other(
            "No folder is waiting to become a vault",
        ));
    };
    match Vault::create_with(&path, coordinator()) {
        Ok(vault) => remember(&app, &state, vault),
        Err(e) => {
            if let VaultError::NotDownloaded(name) = &e {
                request_download(&app, &path.join(name));
            }
            // The user may fix the cause and try again.
            *state.pending.lock().map_err(CommandError::other)? = Some(path);
            Err(e.into())
        }
    }
}

#[tauri::command]
async fn list_notes(
    state: State<'_, AppState>,
    sort: Option<SortOrder>,
) -> CmdResult<Vec<NoteEntry>> {
    with_vault(&state, |v| Ok(v.list_notes_by(sort.unwrap_or_default())?))
}

/// Notes outside `notes/`, e.g. written by an older build on another device.
#[tauri::command]
async fn stray_notes(state: State<'_, AppState>) -> CmdResult<StrayNotes> {
    with_vault(&state, |v| Ok(v.stray_notes()?))
}

/// Moves the notes outside `notes/` into it; call only after the user agreed.
/// Asks iCloud for the ones it skipped because they are not downloaded.
#[tauri::command]
async fn move_stray_notes(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<MovedNotes> {
    with_vault(&state, |v| {
        let not_downloaded = v.stray_notes()?.not_downloaded;
        for name in &not_downloaded {
            request_download(&app, &v.root().join(name));
        }
        Ok(v.move_stray_notes()?)
    })
}

/// Creates a note named after its title (format.md §3).
#[tauri::command]
async fn new_note(state: State<'_, AppState>, text: String) -> CmdResult<NewNote> {
    with_vault(&state, |v| Ok(v.create_titled("", &text)?))
}

/// After the user left a note: renames it to match its title if that changed
/// since `title_at_open`. Returns its path, new or unchanged.
#[tauri::command]
async fn rename_after_edit(
    state: State<'_, AppState>,
    path: String,
    title_at_open: String,
) -> CmdResult<String> {
    with_vault(&state, |v| Ok(v.rename_after_edit(&path, &title_at_open)?))
}

/// Moves a note to the trash, unchanged (format.md §8).
#[tauri::command]
async fn trash_note(state: State<'_, AppState>, path: String) -> CmdResult<TrashedNote> {
    with_vault(&state, |v| Ok(v.trash_note(&path)?))
}

#[tauri::command]
async fn list_trash(state: State<'_, AppState>) -> CmdResult<Vec<TrashedNote>> {
    with_vault(&state, |v| Ok(v.list_trash()?))
}

/// Moves a note out of the trash; returns where it is now.
#[tauri::command]
async fn restore_note(state: State<'_, AppState>, key: String) -> CmdResult<String> {
    with_vault(&state, |v| Ok(v.restore_note(&key)?))
}

#[tauri::command]
async fn read_note(state: State<'_, AppState>, path: String) -> CmdResult<NoteContent> {
    with_vault(&state, |v| Ok(v.read_note(&path)?))
}

/// Saves a note if it still has the content with `expected_hash`; returns the
/// text now on disk (with the front matter the core maintains) and its hash.
#[tauri::command]
async fn save_note(
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected_hash: String,
) -> CmdResult<SavedNote> {
    with_vault(&state, |v| {
        Ok(v.write_note(&path, &text, &expected_hash)?)
    })
}

#[tauri::command]
async fn create_note(
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> CmdResult<SavedNote> {
    with_vault(&state, |v| Ok(v.create_note(&path, &text)?))
}

/// Settles a sync conflict: `text` becomes the note, every other version goes
/// into its history (ADR-0006, ADR-0011). Returns the note as now on disk.
#[tauri::command]
async fn resolve_conflict(
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected_hash: String,
) -> CmdResult<NoteContent> {
    with_vault(&state, |v| {
        Ok(v.resolve_conflict(&path, &text, &expected_hash)?)
    })
}

/// Keeps a note and its copy with the same `id` as two notes; the one created
/// later gets a new `id` (format.md §4). Returns the note as now on disk.
#[tauri::command]
async fn keep_both(
    state: State<'_, AppState>,
    path: String,
    copy_path: String,
) -> CmdResult<NoteContent> {
    with_vault(&state, |v| Ok(v.keep_both(&path, &copy_path)?))
}

/// Asks iCloud to download a note that is only a placeholder (iOS); no-op elsewhere.
#[tauri::command]
async fn download_note(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<()> {
    let full = with_vault(&state, |v| Ok(v.note_file(&path)?))?;
    vault_location::download(&app, &full)
}

/// Returns the hashtags found in a Markdown text (format.md §5.3).
#[tauri::command]
fn extract_tags(markdown: &str) -> Vec<Tag> {
    schnipsle_core::extract_tags(markdown)
}

/// Puts "New Note" (⌘N) and "Open Folder…" (⌘O) at the top of the File menu.
/// Tauri's default menu has a File menu (with Close Window) on macOS but none on
/// Linux, where one is created. A click is sent to the UI as a `menu` event
/// carrying the item id.
#[cfg(desktop)]
fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::Emitter as _;
    use tauri::menu::{Menu, MenuItem, PredefinedMenuItem, Submenu};

    let menu = Menu::default(app.handle())?;
    let new_note = MenuItem::with_id(app, "new-note", "New Note", true, Some("CmdOrCtrl+N"))?;
    let open = MenuItem::with_id(app, "open-vault", "Open Folder…", true, Some("CmdOrCtrl+O"))?;
    let separator = PredefinedMenuItem::separator(app)?;

    let mut file_menu = None;
    for item in menu.items()? {
        if let Some(submenu) = item.as_submenu()
            && submenu.text()? == "File"
        {
            file_menu = Some(submenu.clone());
        }
    }
    if let Some(file) = file_menu {
        file.insert_items(&[&new_note, &open, &separator], 0)?;
    } else {
        let file = Submenu::with_items(
            app,
            "File",
            true,
            &[
                &new_note,
                &open,
                &separator,
                &PredefinedMenuItem::close_window(app, None)?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?;
        // After the application menu on macOS, first elsewhere.
        let position = usize::from(cfg!(target_os = "macos"));
        menu.insert(&file, position)?;
    }
    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        let id = event.id().as_ref();
        if matches!(id, "new-note" | "open-vault") {
            let _ = app.emit("menu", id);
        }
    });
    Ok(())
}

/// Builds and runs the application. Also the iOS/Android entry point.
///
/// # Panics
/// Panics if the Tauri runtime cannot start (e.g. no `WebView` available).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = tauri::Builder::default()
        .manage(AppState::default())
        .plugin(tauri_plugin_vault_access::init());
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_dialog::init())
        .setup(|app| Ok(install_menu(app)?));
    builder
        .invoke_handler(tauri::generate_handler![
            pick_vault,
            restore_vault,
            create_vault,
            list_notes,
            stray_notes,
            move_stray_notes,
            new_note,
            rename_after_edit,
            trash_note,
            list_trash,
            restore_note,
            read_note,
            save_note,
            create_note,
            resolve_conflict,
            keep_both,
            download_note,
            extract_tags
        ])
        .run(tauri::generate_context!())
        .expect("error while running Schnipsle");
}
