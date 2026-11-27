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

#[derive(Deserialize)]
struct FolderResponse {
    path: PathBuf,
}

#[derive(Deserialize)]
struct ShareResponse {
    completed: bool,
}

#[derive(Serialize)]
struct PathArgs<'a> {
    path: &'a str,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct DefaultFolderArgs {
    until_ready: bool,
}

/// Where the default vault goes (ADR-0050 rules 2 and 3).
#[derive(Debug, Deserialize)]
#[serde(tag = "state", rename_all = "camelCase")]
pub enum DefaultFolder {
    /// iCloud Drive is off for the app: its own `Documents` folder.
    Local {
        /// The folder.
        path: PathBuf,
    },
    /// The `Documents` folder of the app's iCloud container.
    #[serde(rename = "iCloud")]
    ICloud {
        /// The folder.
        path: PathBuf,
        /// What iCloud listed in it, relative to it: the items named
        /// `vault.yaml` or starting with "Schnipsle".
        listed: Vec<String>,
    },
    /// iCloud's answer cannot be trusted yet (offline, never synced here).
    Offline,
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

    /// Shows the share sheet for the file or folder at `path`, and waits until
    /// it closes. `Ok(true)` if something was done with it, `Ok(false)` if the
    /// user closed the sheet.
    ///
    /// # Errors
    /// If the sheet cannot be shown or the chosen action failed.
    pub fn share(&self, path: &str) -> Result<bool, tauri::plugin::mobile::PluginInvokeError> {
        let response: ShareResponse = self.0.run_mobile_plugin("share", PathArgs { path })?;
        Ok(response.completed)
    }

    /// Where the default vault goes. With `until_ready` false this answers
    /// [`DefaultFolder::Offline`] when iCloud cannot be asked yet; with true it
    /// waits until it can, which may be never.
    ///
    /// # Errors
    /// If the plugin call fails.
    pub fn default_folder(
        &self,
        until_ready: bool,
    ) -> Result<DefaultFolder, tauri::plugin::mobile::PluginInvokeError> {
        self.0
            .run_mobile_plugin("defaultFolder", DefaultFolderArgs { until_ready })
    }

    /// The app's own `Documents` folder ("On My iPhone" in Files).
    ///
    /// # Errors
    /// If the plugin call fails.
    pub fn local_folder(&self) -> Result<PathBuf, tauri::plugin::mobile::PluginInvokeError> {
        let response: FolderResponse = self.0.run_mobile_plugin("localFolder", ())?;
        Ok(response.path)
    }

    /// Remembers a folder the app chose itself, so that
    /// [`VaultAccess::restore_folder`] opens it after a restart.
    ///
    /// # Errors
    /// If the bookmark cannot be stored.
    pub fn remember_folder(
        &self,
        path: &str,
    ) -> Result<(), tauri::plugin::mobile::PluginInvokeError> {
        self.0
            .run_mobile_plugin::<()>("rememberFolder", PathArgs { path })
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
