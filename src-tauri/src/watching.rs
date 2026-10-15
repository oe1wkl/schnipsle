// SPDX-License-Identifier: Apache-2.0
//! Change notification for the open vault (ADR-0015): starts the platform's
//! sources and forwards each burst of changes to the UI as a `vault-changed`
//! event carrying [`schnipsle_core::VaultChanges`].
//!
//! - Linux, Windows: `notify`.
//! - macOS: `notify` (catches editors that do not coordinate) and `NSFilePresenter`.
//! - iOS: `NSFilePresenter`.

use std::io;
use std::time::Duration;

use schnipsle_core::Vault;
use schnipsle_core::watch::spawn_dispatcher;
use tauri::{AppHandle, Emitter as _};

/// How long the vault must be quiet before a burst of events is reported.
const QUIET: Duration = Duration::from_millis(200);

/// Keeps the sources alive; dropping it stops them and the dispatcher thread.
#[derive(Debug)]
pub struct VaultWatch {
    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    _fs: schnipsle_core::watch::FsWatcher,
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    _presenter: schnipsle_apple::VaultPresenter,
}

/// Starts watching `vault`.
///
/// # Errors
/// If a source cannot be started; the caller decides whether that matters.
pub fn start(app: &AppHandle, vault: &Vault) -> io::Result<VaultWatch> {
    let app = app.clone();
    let hints = spawn_dispatcher(vault.clone(), QUIET, move |changes| {
        let _ = app.emit("vault-changed", changes);
    })?;
    Ok(VaultWatch {
        #[cfg(not(any(target_os = "ios", target_os = "android")))]
        _fs: schnipsle_core::watch::FsWatcher::start(vault.root(), hints.clone())?,
        #[cfg(any(target_os = "macos", target_os = "ios"))]
        _presenter: schnipsle_apple::VaultPresenter::start(vault.root(), hints)?,
    })
}
