// SPDX-License-Identifier: Apache-2.0
//! Choosing and remembering the vault folder, per platform.
//!
//! - Desktop: the system folder dialog; the path is stored in the app's config folder.
//! - iOS: `tauri-plugin-vault-access` (folder picker + security-scoped bookmark).
//!
//! It also tells where the default vault goes at the first start (ADR-0048
//! rule 2, ADR-0050 rules 2 to 4), and whether this device has had a vault
//! before: the default vault is made only once per device (ADR-0048 rule 5).

use std::path::{Path, PathBuf};

use serde::Serialize;
use tauri::AppHandle;
use tauri::Manager as _;

use crate::{CmdResult, CommandError};

/// Where the default vault goes.
#[allow(dead_code)] // each system makes only some of them
pub enum DefaultFolder {
    /// In the app's iCloud folder, with what iCloud's query listed there
    /// (iPhone; empty on the Mac, which decides from the disk).
    ICloud { path: PathBuf, listed: Vec<String> },
    /// On this device only, and why.
    Local { path: PathBuf, why: LocalWhy },
    /// iPhone: iCloud cannot be asked yet (ADR-0050 rule 3).
    Offline,
}

/// Why the default vault is on this device only.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
#[allow(dead_code)] // each system makes only some of them
pub enum LocalWhy {
    /// iCloud Drive is off for the app (iPhone).
    #[serde(rename = "iCloudOff")]
    ICloudOff,
    /// The app's iCloud folder is not on this Mac: no iPhone has used it yet,
    /// or iCloud Drive is off (ADR-0050 rule 4).
    #[serde(rename = "noICloudFolder")]
    NoICloudFolder,
    /// The user chose a folder on this device while iCloud could not be asked.
    Chosen,
    /// A system without iCloud.
    NoICloud,
}

const STARTED_FILE: &str = "first-start.json";

fn started_file(app: &AppHandle) -> CmdResult<PathBuf> {
    Ok(app
        .path()
        .app_config_dir()
        .map_err(CommandError::other)?
        .join(STARTED_FILE))
}

/// Whether this device has had a vault before, picked or made at a first
/// start (ADR-0048 rules 1 and 5).
pub fn had_vault(app: &AppHandle) -> bool {
    started_file(app).is_ok_and(|file| file.is_file())
}

/// Notes that this device has a vault, so that no default vault is made
/// again, also when this one is deleted.
fn note_vault(app: &AppHandle) -> CmdResult<()> {
    if had_vault(app) {
        return Ok(());
    }
    let file = started_file(app)?;
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir).map_err(CommandError::other)?;
    }
    std::fs::write(&file, r#"{"hadVault":true}"#).map_err(CommandError::other)
}

#[cfg(desktop)]
mod platform {
    use super::{AppHandle, CmdResult, CommandError, DefaultFolder, LocalWhy, Path, PathBuf};
    use tauri::Manager as _;
    use tauri_plugin_dialog::DialogExt as _;

    const FILE_NAME: &str = "vault.json";

    fn config_file(app: &AppHandle) -> CmdResult<PathBuf> {
        Ok(app
            .path()
            .app_config_dir()
            .map_err(CommandError::other)?
            .join(FILE_NAME))
    }

    pub fn pick(app: &AppHandle) -> CmdResult<Option<PathBuf>> {
        let Some(picked) = app.dialog().file().blocking_pick_folder() else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(CommandError::other)?;
        remember(app, &path)?;
        Ok(Some(path))
    }

    pub fn remember(app: &AppHandle, path: &Path) -> CmdResult<()> {
        let file = config_file(app)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(CommandError::other)?;
        }
        let json = serde_json::json!({ "path": path });
        std::fs::write(&file, json.to_string()).map_err(CommandError::other)
    }

    pub fn local_folder(app: &AppHandle) -> CmdResult<PathBuf> {
        app.path().document_dir().map_err(CommandError::other)
    }

    /// The Mac uses the app's iCloud folder by its path, without the
    /// entitlement, if an iPhone has made it (ADR-0050 rule 4).
    #[cfg(target_os = "macos")]
    #[allow(clippy::unnecessary_wraps)] // same signature as the iOS version
    pub fn default_folder(app: &AppHandle, _until_ready: bool) -> CmdResult<DefaultFolder> {
        let icloud = app
            .path()
            .home_dir()
            .map_err(CommandError::other)?
            .join("Library/Mobile Documents/iCloud~cc~kraml~schnipsle/Documents");
        Ok(if icloud.is_dir() {
            DefaultFolder::ICloud {
                path: icloud,
                listed: Vec::new(),
            }
        } else {
            DefaultFolder::Local {
                path: local_folder(app)?,
                why: LocalWhy::NoICloudFolder,
            }
        })
    }

    #[cfg(not(target_os = "macos"))]
    pub fn default_folder(app: &AppHandle, _until_ready: bool) -> CmdResult<DefaultFolder> {
        Ok(DefaultFolder::Local {
            path: local_folder(app)?,
            why: LocalWhy::NoICloud,
        })
    }

    pub fn restore(app: &AppHandle) -> CmdResult<Option<PathBuf>> {
        let Ok(text) = std::fs::read_to_string(config_file(app)?) else {
            return Ok(None);
        };
        let value: serde_json::Value = serde_json::from_str(&text).map_err(CommandError::other)?;
        Ok(value["path"]
            .as_str()
            .map(PathBuf::from)
            .filter(|p| p.is_dir()))
    }

    /// Asks iCloud for a file not on this Mac (ADR-0050).
    #[cfg(target_os = "macos")]
    pub fn download(_app: &AppHandle, path: &Path) -> CmdResult<()> {
        schnipsle_apple::download(path).map_err(CommandError::other)
    }

    #[cfg(not(target_os = "macos"))]
    #[allow(clippy::unnecessary_wraps)] // same signature as the iOS version
    pub fn download(_app: &AppHandle, _path: &Path) -> CmdResult<()> {
        Ok(())
    }
}

#[cfg(target_os = "ios")]
mod platform {
    use super::{AppHandle, CmdResult, CommandError, DefaultFolder, LocalWhy, Path, PathBuf};
    use tauri_plugin_vault_access::{DefaultFolder as Found, VaultAccessExt as _};

    pub fn remember(app: &AppHandle, path: &Path) -> CmdResult<()> {
        app.vault_access()
            .remember_folder(&path.to_string_lossy())
            .map_err(CommandError::other)
    }

    pub fn local_folder(app: &AppHandle) -> CmdResult<PathBuf> {
        app.vault_access()
            .local_folder()
            .map_err(CommandError::other)
    }

    /// `ubiquityIdentityToken` and the query decide (ADR-0050 rules 2, 3).
    pub fn default_folder(app: &AppHandle, until_ready: bool) -> CmdResult<DefaultFolder> {
        Ok(
            match app
                .vault_access()
                .default_folder(until_ready)
                .map_err(CommandError::other)?
            {
                Found::Local { path } => DefaultFolder::Local {
                    path,
                    why: LocalWhy::ICloudOff,
                },
                Found::ICloud { path, listed } => DefaultFolder::ICloud { path, listed },
                Found::Offline => DefaultFolder::Offline,
            },
        )
    }

    pub fn pick(app: &AppHandle) -> CmdResult<Option<PathBuf>> {
        app.vault_access()
            .pick_folder()
            .map_err(CommandError::other)
    }

    pub fn restore(app: &AppHandle) -> CmdResult<Option<PathBuf>> {
        app.vault_access()
            .restore_folder()
            .map_err(CommandError::other)
    }

    pub fn download(app: &AppHandle, path: &Path) -> CmdResult<()> {
        app.vault_access()
            .download(&path.to_string_lossy())
            .map_err(CommandError::other)
    }
}

#[cfg(target_os = "android")]
mod platform {
    use super::{AppHandle, CmdResult, CommandError, DefaultFolder, Path, PathBuf};

    fn unsupported<T>() -> CmdResult<T> {
        Err(CommandError::other(
            "Vault folders are not supported on Android yet",
        ))
    }
    pub fn pick(_app: &AppHandle) -> CmdResult<Option<PathBuf>> {
        unsupported()
    }
    pub fn restore(_app: &AppHandle) -> CmdResult<Option<PathBuf>> {
        Ok(None)
    }
    pub fn download(_app: &AppHandle, _path: &Path) -> CmdResult<()> {
        unsupported()
    }
    pub fn remember(_app: &AppHandle, _path: &Path) -> CmdResult<()> {
        unsupported()
    }
    pub fn local_folder(_app: &AppHandle) -> CmdResult<PathBuf> {
        unsupported()
    }
    pub fn default_folder(_app: &AppHandle, _until_ready: bool) -> CmdResult<DefaultFolder> {
        unsupported()
    }
}

/// Lets the user pick a vault folder and remembers it.
pub fn pick(app: &AppHandle) -> CmdResult<Option<PathBuf>> {
    let picked = platform::pick(app)?;
    if picked.is_some() {
        note_vault(app)?;
    }
    Ok(picked)
}

/// The folder picked last time, if still reachable. Also notes that this
/// device has a vault, for installs from before the first start existed.
pub fn restore(app: &AppHandle) -> CmdResult<Option<PathBuf>> {
    let restored = platform::restore(app)?;
    if restored.is_some() {
        note_vault(app)?;
    }
    Ok(restored)
}

/// Remembers the default vault as a picked one is (ADR-0048 rule 5).
pub fn remember(app: &AppHandle, path: &Path) -> CmdResult<()> {
    platform::remember(app, path)?;
    note_vault(app)
}

/// Where the default vault goes. On the iPhone with `until_ready` false this
/// may be [`DefaultFolder::Offline`]; with true it waits for iCloud.
pub fn default_folder(app: &AppHandle, until_ready: bool) -> CmdResult<DefaultFolder> {
    platform::default_folder(app, until_ready)
}

/// The folder for a default vault on this device only.
pub fn local_folder(app: &AppHandle) -> CmdResult<PathBuf> {
    platform::local_folder(app)
}

/// Requests the download of a cloud placeholder (iOS only).
pub fn download(app: &AppHandle, path: &Path) -> CmdResult<()> {
    platform::download(app, path)
}
