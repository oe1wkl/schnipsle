// SPDX-License-Identifier: Apache-2.0
//! Choosing and remembering the vault folder, per platform.
//!
//! - Desktop: the system folder dialog; the path is stored in the app's config folder.
//! - iOS: `tauri-plugin-vault-access` (folder picker + security-scoped bookmark).

use std::path::{Path, PathBuf};

use tauri::AppHandle;

use crate::{CmdResult, CommandError};

#[cfg(desktop)]
mod platform {
    use super::{AppHandle, CmdResult, CommandError, Path, PathBuf};
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
        let file = config_file(app)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir).map_err(CommandError::other)?;
        }
        let json = serde_json::json!({ "path": path });
        std::fs::write(&file, json.to_string()).map_err(CommandError::other)?;
        Ok(Some(path))
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

    #[allow(clippy::unnecessary_wraps)] // same signature as the iOS version
    pub fn download(_app: &AppHandle, _path: &Path) -> CmdResult<()> {
        Ok(())
    }
}

#[cfg(target_os = "ios")]
mod platform {
    use super::{AppHandle, CmdResult, CommandError, Path, PathBuf};
    use tauri_plugin_vault_access::VaultAccessExt as _;

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
    use super::{AppHandle, CmdResult, CommandError, Path, PathBuf};

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
}

/// Lets the user pick a vault folder and remembers it.
pub fn pick(app: &AppHandle) -> CmdResult<Option<PathBuf>> {
    platform::pick(app)
}

/// The folder picked last time, if still reachable.
pub fn restore(app: &AppHandle) -> CmdResult<Option<PathBuf>> {
    platform::restore(app)
}

/// Requests the download of a cloud placeholder (iOS only).
pub fn download(app: &AppHandle, path: &Path) -> CmdResult<()> {
    platform::download(app, path)
}
