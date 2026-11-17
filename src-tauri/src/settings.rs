// SPDX-License-Identifier: Apache-2.0
//! The settings view (ADR-0039) as commands: the settings of the open vault,
//! kept by the core in `.notesapp/settings.yaml`, and those of this device, in
//! `settings.json` in the app's configuration folder, next to the remembered
//! vault. The iPhone has no settings of its own yet.

use std::path::PathBuf;

use schnipsle_core::{Change, DeviceSettings, Retention, Thinning, VaultSettings};
use schnipsle_export::Pandoc;
use serde::Serialize;
use tauri::{AppHandle, Manager as _, State};

use crate::{AppState, CmdResult, CommandError, with_vault};

/// The settings of the open vault.
#[tauri::command]
pub async fn read_settings(state: State<'_, AppState>) -> CmdResult<VaultSettings> {
    with_vault(&state, |v| Ok(v.settings()?))
}

/// What thinning the history with these periods would remove now, asked
/// before they are saved.
#[tauri::command]
pub async fn history_to_thin(
    state: State<'_, AppState>,
    all_days: u64,
    daily_days: u64,
) -> CmdResult<Thinning> {
    let retention = Retention {
        all_days,
        daily_days: daily_days.max(all_days),
    };
    with_vault(&state, |v| {
        Ok(v.history_to_thin(retention, schnipsle_core::stamp::now_ms())?)
    })
}

/// Makes `changes` in the vault's settings and returns those now in effect.
/// A change of the retention thins the history at once, so what the view
/// counted before is what happens (ADR-0039).
#[tauri::command]
pub async fn write_settings(
    state: State<'_, AppState>,
    changes: Vec<Change>,
) -> CmdResult<VaultSettings> {
    with_vault(&state, |v| {
        let settings = v.edit_settings(&changes)?;
        if changes.iter().any(|c| c.is_retention()) {
            let removed = v.thin_history(settings.retention(), schnipsle_core::stamp::now_ms())?;
            if removed > 0 {
                eprintln!("history: {removed} old snapshots removed after a settings change");
            }
        }
        Ok(settings)
    })
}

/// The settings of this device, for the view.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceView {
    /// The Pandoc program the user picked; `None` to find it.
    pandoc_chosen: Option<String>,
    /// The Pandoc that DOCX and EPUB use, if one runs.
    pandoc: Option<PandocView>,
    /// Why the picked Pandoc does not run.
    pandoc_problem: Option<String>,
    /// Whether the app looks for updates (ADR-0036).
    check_for_updates: bool,
}

/// A Pandoc that runs.
#[derive(Serialize)]
pub struct PandocView {
    path: String,
    version: String,
}

/// The settings of this device; `None` on a phone, which has none yet.
#[tauri::command]
pub async fn read_device_settings(app: AppHandle) -> CmdResult<Option<DeviceView>> {
    if cfg!(mobile) {
        return Ok(None);
    }
    Ok(Some(view(&load(&app)?)))
}

/// Turns the update check on or off.
#[tauri::command]
pub async fn set_update_check(app: AppHandle, on: bool) -> CmdResult<DeviceView> {
    change(&app, |settings| settings.set_check_for_updates(on))
}

/// Finds Pandoc again instead of using a picked one.
#[tauri::command]
pub async fn find_pandoc_again(app: AppHandle) -> CmdResult<DeviceView> {
    change(&app, |settings| settings.set_pandoc(None))
}

/// Asks for the Pandoc program to use; `None` if the user cancelled. A file
/// that does not say its Pandoc version is refused and not saved.
#[tauri::command]
pub async fn pick_pandoc(app: AppHandle) -> CmdResult<Option<DeviceView>> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt as _;
        let mut dialog = app.dialog().file().set_title("Choose the Pandoc program");
        if let Some(folder) = ["/opt/homebrew/bin", "/usr/local/bin"]
            .into_iter()
            .map(PathBuf::from)
            .find(|folder| folder.is_dir())
        {
            dialog = dialog.set_directory(folder);
        }
        let Some(picked) = dialog.blocking_pick_file() else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(CommandError::other)?;
        Pandoc::at(path.clone()).map_err(|e| CommandError {
            kind: "notPandoc",
            ..CommandError::other(format!("{} is not a Pandoc that runs: {e}", path.display()))
        })?;
        change(&app, |settings| settings.set_pandoc(Some(&path))).map(Some)
    }
    #[cfg(mobile)]
    {
        let _ = app;
        Err(CommandError::other("Pandoc is not used on this device"))
    }
}

/// The Pandoc DOCX and EPUB go through: the one picked in the settings, or
/// else the one found (ADR-0025). A picked one that does not run is not
/// replaced by another; the error says why. `Ok(None)` if there is none, and
/// always on a phone, which starts no other programs.
pub fn pandoc(app: &AppHandle) -> Result<Option<Pandoc>, String> {
    if cfg!(mobile) {
        return Ok(None);
    }
    let chosen = load(app).ok().and_then(|settings| settings.pandoc());
    match chosen {
        Some(path) => Pandoc::at(path.clone()).map(Some).map_err(|e| {
            format!(
                "The Pandoc chosen in Settings, {}, does not run: {e}",
                path.display()
            )
        }),
        None => Ok(Pandoc::find()),
    }
}

fn view(settings: &DeviceSettings) -> DeviceView {
    let chosen = settings.pandoc();
    let (pandoc, pandoc_problem) = match &chosen {
        Some(path) => match Pandoc::at(path.clone()) {
            Ok(pandoc) => (Some(pandoc), None),
            Err(e) => (None, Some(e.to_string())),
        },
        None => (Pandoc::find(), None),
    };
    DeviceView {
        pandoc_chosen: chosen.map(|p| p.display().to_string()),
        pandoc: pandoc.map(|p| PandocView {
            path: p.path().display().to_string(),
            version: p.version().to_owned(),
        }),
        pandoc_problem,
        check_for_updates: settings.check_for_updates(),
    }
}

fn file(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(CommandError::other)?
        .join("settings.json"))
}

fn load(app: &AppHandle) -> CmdResult<DeviceSettings> {
    DeviceSettings::load(&file(app)?).map_err(CommandError::other)
}

/// Applies `edit` to the device's settings and saves them. A damaged file is
/// started over, as it holds nothing that cannot be set again.
fn change(app: &AppHandle, edit: impl FnOnce(&mut DeviceSettings)) -> CmdResult<DeviceView> {
    let file = file(app)?;
    let mut settings = DeviceSettings::load(&file).unwrap_or_else(|e| {
        eprintln!("device settings damaged, starting over: {e}");
        DeviceSettings::default()
    });
    edit(&mut settings);
    settings.save(&file).map_err(CommandError::other)?;
    Ok(view(&settings))
}
