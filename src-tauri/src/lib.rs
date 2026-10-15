// SPDX-License-Identifier: Apache-2.0
//! Schnipsle app shell: exposes `schnipsle-core` to the UI as Tauri commands.
//!
//! The UI never touches vault files; every file operation goes through a command
//! here (ADR-0003). Commands that may show system UI or wait for iOS are `async`,
//! so they run off the main thread.

mod vault_location;
mod watching;

use std::sync::Mutex;

use schnipsle_core::{NoteContent, NoteEntry, Tag, Vault, VaultError};
use serde::Serialize;
use tauri::State;

/// The currently opened vault, if any, and the watch reporting its changes.
#[derive(Default)]
struct AppState {
    vault: Mutex<Option<Vault>>,
    watch: Mutex<Option<watching::VaultWatch>>,
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
struct VaultInfo {
    name: String,
    path: String,
}

/// Opens a vault with the platform's file coordination (ADR-0015) and sync
/// conflict versions (ADR-0006).
fn open_vault(path: &std::path::Path) -> Result<Vault, VaultError> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return Ok(
        Vault::open_with(path, std::sync::Arc::new(schnipsle_apple::FileCoordinator))?
            .with_sync_conflicts(std::sync::Arc::new(schnipsle_apple::ICloudVersions)),
    );
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    return Vault::open(path);
}

fn open_and_remember(
    app: &tauri::AppHandle,
    state: &AppState,
    path: &std::path::Path,
) -> CmdResult<VaultInfo> {
    let vault = open_vault(path)?;
    let info = VaultInfo {
        name: path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        ),
        path: path.display().to_string(),
    };
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
) -> CmdResult<Option<VaultInfo>> {
    match vault_location::pick(&app)? {
        Some(path) => open_and_remember(&app, &state, &path).map(Some),
        None => Ok(None),
    }
}

/// Re-opens the vault used last time, if it is still reachable.
#[tauri::command]
async fn restore_vault(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
) -> CmdResult<Option<VaultInfo>> {
    match vault_location::restore(&app)? {
        Some(path) => open_and_remember(&app, &state, &path).map(Some),
        None => Ok(None),
    }
}

#[tauri::command]
async fn list_notes(state: State<'_, AppState>) -> CmdResult<Vec<NoteEntry>> {
    with_vault(&state, |v| Ok(v.list_notes()?))
}

#[tauri::command]
async fn read_note(state: State<'_, AppState>, path: String) -> CmdResult<NoteContent> {
    with_vault(&state, |v| Ok(v.read_note(&path)?))
}

/// Saves a note if it still has the content with `expected_hash`; returns the new hash.
#[tauri::command]
async fn save_note(
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected_hash: String,
) -> CmdResult<String> {
    with_vault(&state, |v| {
        Ok(v.write_note(&path, &text, &expected_hash)?)
    })
}

#[tauri::command]
async fn create_note(state: State<'_, AppState>, path: String, text: String) -> CmdResult<String> {
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

/// Asks iCloud to download a note that is only a placeholder (iOS); no-op elsewhere.
#[tauri::command]
async fn download_note(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
) -> CmdResult<()> {
    let full = with_vault(&state, |v| Ok(v.root().join(&path)))?;
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
            list_notes,
            read_note,
            save_note,
            create_note,
            resolve_conflict,
            download_note,
            extract_tags
        ])
        .run(tauri::generate_context!())
        .expect("error while running Schnipsle");
}
