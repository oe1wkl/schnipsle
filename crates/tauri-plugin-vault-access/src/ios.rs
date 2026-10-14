// SPDX-License-Identifier: Apache-2.0
//! Bridge to the Swift implementation in `ios/Sources/VaultAccess/VaultAccessPlugin.swift`.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use tauri::{
    Runtime,
    plugin::{PluginApi, PluginHandle},
};

tauri::ios_plugin_binding!(init_plugin_vault_access);

pub(crate) fn init<R: Runtime>(
    api: PluginApi<R, ()>,
) -> Result<VaultAccess<R>, Box<dyn std::error::Error>> {
    Ok(VaultAccess(
        api.register_ios_plugin(init_plugin_vault_access)?,
    ))
}

/// The plugin's API on iOS.
pub struct VaultAccess<R: Runtime>(PluginHandle<R>);

#[derive(Deserialize)]
struct PathResponse {
    path: Option<String>,
}

#[derive(Serialize)]
struct PathArgs<'a> {
    path: &'a str,
}

impl<R: Runtime> VaultAccess<R> {
    /// Shows the system folder picker. `Ok(None)` if the user cancelled.
    ///
    /// # Errors
    /// If iOS refuses access to the picked folder or the bookmark cannot be stored.
    pub fn pick_folder(&self) -> Result<Option<PathBuf>, tauri::plugin::mobile::PluginInvokeError> {
        let response: PathResponse = self.0.run_mobile_plugin("pickFolder", ())?;
        Ok(response.path.map(PathBuf::from))
    }

    /// Re-opens the folder picked earlier. `Ok(None)` if none was picked or it is gone.
    ///
    /// # Errors
    /// If the stored bookmark cannot be resolved.
    pub fn restore_folder(
        &self,
    ) -> Result<Option<PathBuf>, tauri::plugin::mobile::PluginInvokeError> {
        let response: PathResponse = self.0.run_mobile_plugin("restoreFolder", ())?;
        Ok(response.path.map(PathBuf::from))
    }

    /// Starts downloading an iCloud file that is only a placeholder on this device.
    /// `path` is the file's real path (without the `.icloud` placeholder name).
    ///
    /// # Errors
    /// If iOS refuses the download request.
    pub fn download(&self, path: &str) -> Result<(), tauri::plugin::mobile::PluginInvokeError> {
        self.0
            .run_mobile_plugin::<()>("download", PathArgs { path })
    }
}
