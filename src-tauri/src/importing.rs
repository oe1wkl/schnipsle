// SPDX-License-Identifier: Apache-2.0
//! Importing notes (ADR-0012, ADR-0019, format.md §12) as commands, on the
//! desktop only: pick the export, a dry run whose report the UI shows, then
//! the import of exactly that plan with progress events, and the index
//! updated for the notes it wrote.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use schnipsle_core::ImportLogError;
use schnipsle_import::{ImportError, Outcome, Plan, Source, SystemEnv, apple_notes, simplenote};
use serde::Serialize;
use tauri::{AppHandle, Emitter as _, State};

use crate::{AppState, CmdResult, CommandError, indexing, with_vault};

/// The dry run the user is looking at, with the root of its vault: Import
/// writes exactly this plan, never one made afresh.
#[derive(Default)]
pub struct PendingImport(Mutex<Option<(PathBuf, Plan)>>);

impl From<ImportError> for CommandError {
    fn from(e: ImportError) -> Self {
        let message = e.to_string();
        let kind = match e {
            ImportError::Vault(v) | ImportError::Log(ImportLogError::Vault(v)) => return v.into(),
            ImportError::Log(_) => "importLog",
            ImportError::Source(_) => "importSource",
        };
        Self {
            kind,
            message,
            current_hash: None,
        }
    }
}

/// The dry run's report and what the dialog shows around it.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportPreview {
    plan: Plan,
    /// The folder or file read.
    path: String,
    /// Apple Notes: whether the export has the metadata file; false for
    /// other sources.
    has_metadata: bool,
}

/// Progress of [`apply_import`], sent as `import-progress`.
#[derive(Clone, Serialize)]
struct Progress {
    done: usize,
    total: usize,
}

/// Lets the user pick the export of `source`: the folder of an Apple Notes
/// export, the zip (or `notes.json`) of a Simplenote export. `None` if
/// cancelled.
#[tauri::command]
pub async fn pick_import(app: AppHandle, source: Source) -> CmdResult<Option<String>> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt as _;
        let dialog = app.dialog().file();
        let picked = match source {
            Source::Simplenote => dialog
                .add_filter("Simplenote export", &["zip", "json"])
                .blocking_pick_file(),
            _ => dialog.blocking_pick_folder(),
        };
        let Some(picked) = picked else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(CommandError::other)?;
        Ok(Some(path.display().to_string()))
    }
    #[cfg(mobile)]
    {
        let _ = (app, source);
        Err(desktop_only())
    }
}

#[cfg(mobile)]
fn desktop_only() -> CommandError {
    CommandError::other("Import is available in Schnipsle on the Mac")
}

/// The dry run: reads the export at `path` and plans the import into the
/// open vault. Writes nothing; the plan waits for [`apply_import`].
#[tauri::command]
pub async fn plan_import(
    state: State<'_, AppState>,
    source: Source,
    path: String,
) -> CmdResult<ImportPreview> {
    let input = PathBuf::from(&path);
    let batch = match source {
        Source::AppleNotes => apple_notes::read(&input)?,
        Source::Simplenote => simplenote::read(&input)?,
        other => {
            return Err(CommandError::other(format!(
                "Import from {} is not available yet",
                other.folder()
            )));
        }
    };
    let has_metadata =
        source == Source::AppleNotes && input.join(apple_notes::METADATA_FILE).is_file();
    with_vault(&state, |v| {
        let plan = schnipsle_import::plan(v, &batch, &SystemEnv)?;
        *state.import.0.lock().map_err(CommandError::other)? =
            Some((v.root().to_owned(), plan.clone()));
        Ok(ImportPreview {
            plan,
            path,
            has_metadata,
        })
    })
}

/// Forgets the dry run, e.g. when the dialog is cancelled.
#[tauri::command]
pub async fn cancel_import(state: State<'_, AppState>) -> CmdResult<()> {
    *state.import.0.lock().map_err(CommandError::other)? = None;
    Ok(())
}

/// Writes the plan of the last dry run, sending `import-progress` events,
/// and updates the index for the notes written. Call only after the user
/// chose Import.
#[tauri::command]
pub async fn apply_import(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Outcome> {
    let pending = state.import.0.lock().map_err(CommandError::other)?.take();
    with_vault(&state, |v| {
        let Some((_, plan)) = pending.filter(|(root, _)| root == v.root()) else {
            return Err(CommandError::other(
                "There is no dry run for this vault; choose the export again",
            ));
        };
        let mut progress = |done, total| {
            let _ = app.emit("import-progress", Progress { done, total });
        };
        let outcome = schnipsle_import::apply_with_progress(v, plan, &SystemEnv, &mut progress)?;
        let keys: Vec<String> = outcome
            .written
            .iter()
            .map(|w| indexing::note_key(&w.path))
            .collect();
        state.index.update(v, &keys);
        Ok(outcome)
    })
}

/// Runs `scripts/apple-notes-metadata.js` (built into the app) on the export
/// folder at `path`, which writes `apple-notes-metadata.json` there. Reads
/// Notes only; macOS asks the first time whether Schnipsle may control Notes.
/// Takes about half a minute for a few hundred notes.
#[tauri::command]
pub async fn read_apple_notes_metadata(path: String) -> CmdResult<String> {
    run_metadata_script(Path::new(&path))
}

#[cfg(target_os = "macos")]
fn run_metadata_script(export: &Path) -> CmdResult<String> {
    const SCRIPT: &str = include_str!("../../scripts/apple-notes-metadata.js");
    if !export.is_dir() {
        return Err(CommandError::other(format!(
            "{} is not a folder",
            export.display()
        )));
    }
    let dir = tempfile_dir()?;
    let script = dir.join("apple-notes-metadata.js");
    std::fs::write(&script, SCRIPT).map_err(CommandError::other)?;
    let output = std::process::Command::new("/usr/bin/osascript")
        .arg("-l")
        .arg("JavaScript")
        .arg(&script)
        .arg(export)
        .output()
        .map_err(CommandError::other)?;
    let _ = std::fs::remove_file(&script);
    if output.status.success() {
        return Ok(String::from_utf8_lossy(&output.stdout).trim().to_owned());
    }
    let error = String::from_utf8_lossy(&output.stderr);
    // -1743: the user (or an earlier answer) did not allow controlling Notes.
    let message = if error.contains("-1743") {
        "Schnipsle may not control Notes. Allow it in System Settings › Privacy & Security › \
         Automation, then try again."
            .to_owned()
    } else {
        format!("Reading Notes failed: {}", error.trim())
    };
    Err(CommandError {
        kind: "metadataScript",
        message,
        current_hash: None,
    })
}

/// A folder of our own for the script file.
#[cfg(target_os = "macos")]
fn tempfile_dir() -> CmdResult<PathBuf> {
    let dir = std::env::temp_dir().join("cc.kraml.schnipsle");
    std::fs::create_dir_all(&dir).map_err(CommandError::other)?;
    Ok(dir)
}

#[cfg(not(target_os = "macos"))]
fn run_metadata_script(_export: &Path) -> CmdResult<String> {
    Err(CommandError::other(
        "Reading the Notes metadata needs macOS; run the script on a Mac",
    ))
}
