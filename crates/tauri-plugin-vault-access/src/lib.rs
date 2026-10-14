// SPDX-License-Identifier: Apache-2.0
//! Access to a user-chosen vault folder on iOS.
//!
//! iOS apps may only read a folder outside their sandbox (e.g. in iCloud Drive)
//! after the user picks it in the system folder picker, and only while a
//! security-scoped bookmark keeps that permission alive. Tauri's dialog plugin
//! does not offer a folder picker on iOS, so this plugin provides:
//!
//! - [`VaultAccess::pick_folder`]: show the picker, start access, store a bookmark;
//! - [`VaultAccess::restore_folder`]: re-open the stored folder after a restart;
//! - [`VaultAccess::download`]: ask iCloud to download a note that is only a placeholder.
//!
//! On other platforms the plugin does nothing; desktop apps use the dialog plugin.
//! All methods block until iOS answers, so call them from async commands, never
//! from the main thread.

use tauri::{
    Runtime,
    plugin::{Builder, TauriPlugin},
};

#[cfg(target_os = "ios")]
mod ios;
#[cfg(target_os = "ios")]
pub use ios::VaultAccess;

/// Creates the plugin. Register it with `tauri::Builder::plugin`.
#[must_use]
pub fn init<R: Runtime>() -> TauriPlugin<R> {
    Builder::new("vault-access")
        .setup(|app, api| {
            #[cfg(target_os = "ios")]
            {
                use tauri::Manager as _;
                app.manage(ios::init(api)?);
            }
            #[cfg(not(target_os = "ios"))]
            let _ = (app, api);
            Ok(())
        })
        .build()
}

/// Gives access to [`VaultAccess`] from an `AppHandle`, `App` or `Window`.
#[cfg(target_os = "ios")]
pub trait VaultAccessExt<R: Runtime> {
    /// The plugin's API.
    fn vault_access(&self) -> &VaultAccess<R>;
}

#[cfg(target_os = "ios")]
impl<R: Runtime, T: tauri::Manager<R>> VaultAccessExt<R> for T {
    fn vault_access(&self) -> &VaultAccess<R> {
        self.state::<VaultAccess<R>>().inner()
    }
}
