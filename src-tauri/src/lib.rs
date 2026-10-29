// SPDX-License-Identifier: Apache-2.0
//! Schnipsle app shell: exposes `schnipsle-core` to the UI as Tauri commands.
//!
//! The UI never touches vault files; every file operation goes through a command
//! here (ADR-0003). Commands that may show system UI or wait for iOS are `async`,
//! so they run off the main thread.

mod importing;
mod indexing;
mod smart_folders;
mod vault_location;
mod watching;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use schnipsle_core::layout::VAULT_FILE;
use schnipsle_core::{
    AddedAttachment, Coordinator, Folder, MovedNotes, NewNote, NoteContent, NoteEntry, SavedNote,
    SearchHit, SortOrder, StrayNotes, Tag, TagFolder, TrashedNote, Vault, VaultError, Version,
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
    /// The search index of the open vault (ADR-0009).
    index: indexing::SharedIndex,
    /// The import dry run the UI shows.
    import: importing::PendingImport,
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
    let ids = indexing::IndexIds::new(state.index.clone(), vault.root().to_owned());
    let vault = with_sync_conflicts(vault).with_notes_by_id(Arc::new(ids));
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
    *watch = watching::start(app, &vault, state.index.clone())
        .inspect_err(|e| eprintln!("vault changes will not be reported: {e}"))
        .ok();
    state.index.open_for(app, &vault);
    thin_history_in_background(&vault);
    *state.vault.lock().map_err(CommandError::other)? = Some(vault);
    Ok(info)
}

/// Applies the history retention (ADR-0011) without holding up the UI.
fn thin_history_in_background(vault: &Vault) {
    let vault = vault.clone();
    let spawned = std::thread::Builder::new()
        .name("history-retention".into())
        .spawn(move || {
            let now = schnipsle_core::stamp::now_ms();
            match vault.thin_history(vault.retention(), now) {
                Ok(0) | Err(VaultError::ReadOnly(_)) => {}
                Ok(n) => eprintln!("history: {n} old snapshots removed"),
                Err(e) => eprintln!("history not thinned: {e}"),
            }
        });
    if let Err(e) = spawned {
        eprintln!("history not thinned: {e}");
    }
}

/// Updates the index for notes the app itself wrote: on iOS no event reports
/// them (the presenter ignores its own coordinated writes).
fn reindex(state: &AppState, vault: &Vault, keys: &[String]) {
    state.index.update(vault, keys);
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
        let moved = v.move_stray_notes()?;
        state.index.sync_in_background(&app, v); // whole folders may have moved in
        Ok(moved)
    })
}

/// A header of an IPC request, percent-encoded by the UI (headers are ASCII).
fn header(request: &tauri::ipc::Request<'_>, name: &str) -> CmdResult<String> {
    request
        .headers()
        .get(name)
        .and_then(|v| v.to_str().ok())
        .and_then(schnipsle_core::attachments::percent_decode)
        .ok_or_else(|| CommandError::other(format!("missing header {name}")))
}

/// Stores a file as an attachment (format.md §6) for a note and returns the
/// Markdown linking to it. The body is the file's bytes; the headers
/// `x-note` (note path) and `x-name` (original file name) are percent-encoded.
#[tauri::command]
async fn add_attachment(
    state: State<'_, AppState>,
    request: tauri::ipc::Request<'_>,
) -> CmdResult<AddedAttachment> {
    let note = header(&request, "x-note")?;
    let name = header(&request, "x-name")?;
    let json_bytes;
    let bytes = match request.body() {
        tauri::ipc::InvokeBody::Raw(bytes) => bytes.as_slice(),
        // Android sends JSON only: an array of numbers.
        tauri::ipc::InvokeBody::Json(value) => {
            json_bytes =
                serde_json::from_value::<Vec<u8>>(value.clone()).map_err(CommandError::other)?;
            json_bytes.as_slice()
        }
    };
    with_vault(&state, |v| Ok(v.add_attachment(&note, &name, bytes)?))
}

/// The bytes of the file a link in a note points to (format.md §5.5), e.g. an
/// image to show in the editor.
#[tauri::command]
async fn read_linked_file(
    state: State<'_, AppState>,
    note_path: String,
    link: String,
) -> CmdResult<tauri::ipc::Response> {
    with_vault(&state, |v| {
        Ok(tauri::ipc::Response::new(
            v.read_linked_file(&note_path, &link)?,
        ))
    })
}

/// Stores a version of a note in its history (ADR-0011): `text` if given,
/// else the file on disk; skipped if the history already ends with it.
#[tauri::command]
async fn snapshot_note(
    state: State<'_, AppState>,
    path: String,
    text: Option<String>,
) -> CmdResult<Option<String>> {
    with_vault(&state, |v| Ok(v.snapshot_note(&path, text.as_deref())?))
}

/// The versions in a note's history, newest first (ADR-0011).
#[tauri::command]
async fn list_versions(state: State<'_, AppState>, path: String) -> CmdResult<Vec<Version>> {
    with_vault(&state, |v| Ok(v.versions(&path)?))
}

#[tauri::command]
async fn read_version(state: State<'_, AppState>, path: String, name: String) -> CmdResult<String> {
    with_vault(&state, |v| Ok(v.read_version(&path, &name)?))
}

/// Makes a version the note's text again; the current text goes into the
/// history first. Returns what is now on disk.
#[tauri::command]
async fn restore_version(
    state: State<'_, AppState>,
    path: String,
    name: String,
    expected_hash: String,
) -> CmdResult<SavedNote> {
    with_vault(&state, |v| {
        let saved = v.restore_version(&path, &name, &expected_hash)?;
        reindex(&state, v, &[indexing::note_key(&path)]);
        Ok(saved)
    })
}

/// Creates a new note from a version, next to the note.
#[tauri::command]
async fn copy_version(
    state: State<'_, AppState>,
    path: String,
    name: String,
) -> CmdResult<NewNote> {
    with_vault(&state, |v| {
        let created = v.copy_version(&path, &name)?;
        reindex(&state, v, &[indexing::note_key(&created.path)]);
        Ok(created)
    })
}

/// Runs a query on the index of the open vault.
fn with_index<T>(
    state: &AppState,
    f: impl FnOnce(&schnipsle_core::Index) -> Result<T, schnipsle_core::IndexError>,
) -> CmdResult<T> {
    with_vault(state, |v| {
        state
            .index
            .read(v, f)
            .ok_or_else(|| CommandError {
                kind: "noIndex",
                message: "The search index is not available".into(),
                current_hash: None,
            })?
            .map_err(CommandError::other)
    })
}

/// The subfolders of the tag folder with `keys`, or the top level for none
/// (ADR-0008).
#[tauri::command]
async fn tag_folders(state: State<'_, AppState>, keys: Vec<String>) -> CmdResult<Vec<TagFolder>> {
    with_index(&state, |i| i.tag_folders(&keys))
}

/// Full-text search (F-09) in the query language of
/// `docs/manual/query-language.md`; best matches first, with snippets.
/// `utc_offset_minutes` (local time minus UTC) makes `created:2026-10-23`
/// and `today` mean the user's local days.
#[tauri::command]
async fn search_notes(
    state: State<'_, AppState>,
    query: String,
    utc_offset_minutes: i32,
) -> CmdResult<Vec<SearchHit>> {
    with_index(&state, |i| i.search(&query, utc_offset_minutes))
}

/// Number of notes without any tag.
#[tauri::command]
async fn untagged_count(state: State<'_, AppState>) -> CmdResult<usize> {
    with_index(&state, schnipsle_core::Index::untagged_count)
}

/// The notes of the tag folder with `keys`; the untagged notes for none.
#[tauri::command]
async fn notes_in_tags(
    state: State<'_, AppState>,
    keys: Vec<String>,
    sort: Option<SortOrder>,
) -> CmdResult<Vec<NoteEntry>> {
    with_index(&state, |i| i.notes_in(&keys, sort.unwrap_or_default()))
}

/// Creates a note named after its title (format.md §3).
#[tauri::command]
async fn new_note(state: State<'_, AppState>, text: String) -> CmdResult<NewNote> {
    with_vault(&state, |v| {
        let created = v.create_titled("", &text)?;
        reindex(&state, v, &[indexing::note_key(&created.path)]);
        Ok(created)
    })
}

/// After the user left a note: renames it to match its title if that changed
/// since `title_at_open`. Returns its path, new or unchanged.
#[tauri::command]
async fn rename_after_edit(
    state: State<'_, AppState>,
    path: String,
    title_at_open: String,
) -> CmdResult<String> {
    with_vault(&state, |v| {
        let renamed = v.rename_after_edit(&path, &title_at_open)?;
        if renamed != path {
            reindex(
                &state,
                v,
                &[indexing::note_key(&path), indexing::note_key(&renamed)],
            );
        }
        Ok(renamed)
    })
}

/// Moves a note to the trash, unchanged (format.md §8).
#[tauri::command]
async fn trash_note(state: State<'_, AppState>, path: String) -> CmdResult<TrashedNote> {
    with_vault(&state, |v| {
        let trashed = v.trash_note(&path)?;
        let keys = [indexing::note_key(&path), indexing::trash_key(&trashed.key)];
        reindex(&state, v, &keys);
        Ok(trashed)
    })
}

#[tauri::command]
async fn list_trash(state: State<'_, AppState>) -> CmdResult<Vec<TrashedNote>> {
    with_vault(&state, |v| Ok(v.list_trash()?))
}

/// Moves a note out of the trash; returns where it is now.
#[tauri::command]
async fn restore_note(state: State<'_, AppState>, key: String) -> CmdResult<String> {
    with_vault(&state, |v| {
        let restored = v.restore_note(&key)?;
        reindex(
            &state,
            v,
            &[indexing::trash_key(&key), indexing::note_key(&restored)],
        );
        Ok(restored)
    })
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
        let saved = v.write_note(&path, &text, &expected_hash)?;
        reindex(&state, v, &[indexing::note_key(&path)]);
        Ok(saved)
    })
}

/// Pins or unpins a note if it still has the content with `expected_hash`;
/// returns the text now on disk and its hash.
#[tauri::command]
async fn set_pinned(
    state: State<'_, AppState>,
    path: String,
    pinned: bool,
    expected_hash: String,
) -> CmdResult<SavedNote> {
    with_vault(&state, |v| {
        let saved = v.set_pinned(&path, pinned, &expected_hash)?;
        reindex(&state, v, &[indexing::note_key(&path)]);
        Ok(saved)
    })
}

#[tauri::command]
async fn create_note(
    state: State<'_, AppState>,
    path: String,
    text: String,
) -> CmdResult<SavedNote> {
    with_vault(&state, |v| {
        let saved = v.create_note(&path, &text)?;
        reindex(&state, v, &[indexing::note_key(&path)]);
        Ok(saved)
    })
}

/// Settles a sync conflict: `text` becomes the note, every other version goes
/// into its history (ADR-0006, ADR-0011). Returns the note as now on disk.
#[tauri::command]
async fn resolve_conflict(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    path: String,
    text: String,
    expected_hash: String,
) -> CmdResult<NoteContent> {
    with_vault(&state, |v| {
        let note = v.resolve_conflict(&path, &text, &expected_hash)?;
        // Copies with the same id may have been removed anywhere.
        state.index.sync_in_background(&app, v);
        Ok(note)
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
    with_vault(&state, |v| {
        let note = v.keep_both(&path, &copy_path)?;
        reindex(
            &state,
            v,
            &[indexing::note_key(&path), indexing::note_key(&copy_path)],
        );
        Ok(note)
    })
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

/// The app's own menu bar (desktop). Unlike Tauri's default it has "Close
/// Window" only once, in the File menu, next to "New Note" (⌘N), "Open
/// Vault…" (⌘O) and "Import…". A click on those three is sent to the UI as a
/// `menu` event carrying the item id; everything else is a standard system
/// item.
#[cfg(desktop)]
fn install_menu(app: &tauri::App) -> tauri::Result<()> {
    use tauri::Emitter as _;
    use tauri::menu::{AboutMetadata, Menu, MenuItem, PredefinedMenuItem, Submenu};

    let handle = app.handle();
    let info = handle.package_info();
    let about = AboutMetadata {
        name: Some(info.name.clone()),
        version: Some(info.version.to_string()),
        copyright: handle.config().bundle.copyright.clone(),
        ..AboutMetadata::default()
    };
    let separator = || PredefinedMenuItem::separator(app);

    let file = Submenu::with_items(
        app,
        "File",
        true,
        &[
            &MenuItem::with_id(app, "new-note", "New Note", true, Some("CmdOrCtrl+N"))?,
            &MenuItem::with_id(app, "open-vault", "Open Vault…", true, Some("CmdOrCtrl+O"))?,
            &separator()?,
            &MenuItem::with_id(app, "import", "Import…", true, None::<&str>)?,
            &separator()?,
            &PredefinedMenuItem::close_window(app, None)?,
            #[cfg(not(target_os = "macos"))]
            &PredefinedMenuItem::quit(app, None)?,
        ],
    )?;
    let edit = Submenu::with_items(
        app,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::undo(app, None)?,
            &PredefinedMenuItem::redo(app, None)?,
            &separator()?,
            &PredefinedMenuItem::cut(app, None)?,
            &PredefinedMenuItem::copy(app, None)?,
            &PredefinedMenuItem::paste(app, None)?,
            &PredefinedMenuItem::select_all(app, None)?,
        ],
    )?;
    let window = Submenu::with_items(
        app,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(app, None)?,
            &PredefinedMenuItem::maximize(app, None)?,
            #[cfg(target_os = "macos")]
            &PredefinedMenuItem::fullscreen(app, None)?,
        ],
    )?;

    #[cfg(target_os = "macos")]
    let menu = {
        let app_menu = Submenu::with_items(
            app,
            info.name.clone(),
            true,
            &[
                &PredefinedMenuItem::about(app, None, Some(about))?,
                &separator()?,
                &PredefinedMenuItem::services(app, None)?,
                &separator()?,
                &PredefinedMenuItem::hide(app, None)?,
                &PredefinedMenuItem::hide_others(app, None)?,
                &PredefinedMenuItem::show_all(app, None)?,
                &separator()?,
                &PredefinedMenuItem::quit(app, None)?,
            ],
        )?;
        Menu::with_items(app, &[&app_menu, &file, &edit, &window])?
    };
    #[cfg(not(target_os = "macos"))]
    let menu = {
        let help = Submenu::with_items(
            app,
            "Help",
            true,
            &[&PredefinedMenuItem::about(app, None, Some(about))?],
        )?;
        Menu::with_items(app, &[&file, &edit, &window, &help])?
    };
    #[cfg(target_os = "macos")]
    window.set_as_windows_menu_for_nsapp()?;

    app.set_menu(menu)?;
    app.on_menu_event(|app, event| {
        let id = event.id().as_ref();
        if matches!(id, "new-note" | "open-vault" | "import") {
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
            tag_folders,
            search_notes,
            untagged_count,
            notes_in_tags,
            snapshot_note,
            list_versions,
            read_version,
            restore_version,
            copy_version,
            add_attachment,
            read_linked_file,
            stray_notes,
            move_stray_notes,
            new_note,
            rename_after_edit,
            trash_note,
            list_trash,
            restore_note,
            read_note,
            save_note,
            set_pinned,
            smart_folders::list_smart_folders,
            smart_folders::add_smart_folder,
            smart_folders::update_smart_folder,
            smart_folders::remove_smart_folder,
            smart_folders::move_smart_folder,
            smart_folders::smart_folder_notes,
            create_note,
            resolve_conflict,
            keep_both,
            download_note,
            extract_tags,
            importing::pick_import,
            importing::plan_import,
            importing::cancel_import,
            importing::apply_import,
            importing::read_apple_notes_metadata
        ])
        .run(tauri::generate_context!())
        .expect("error while running Schnipsle");
}
