// SPDX-License-Identifier: Apache-2.0
//! Quit and Close wait for the page to save the open note (ADR-0042 rule 3).
//!
//! On the desktop, closing the window and quitting the app are held back,
//! and the page is asked with a `quit-requested` event to save first. It
//! answers at once with [`quit_heard`], then saves, asks the user if text
//! could not be saved, and quits with [`quit_app`]. A page that does not
//! answer within [`NO_ANSWER`] is taken to be stuck, and the next request
//! quits without it, so the app can always be quit.
//!
//! The iPhone saves when the app goes to the background, so only the
//! desktop holds anything back.
#![cfg_attr(mobile, allow(dead_code))]

use std::sync::{Mutex, PoisonError};
use std::time::{Duration, Instant};

use tauri::{AppHandle, Emitter, Manager, Runtime, State};

/// How long the page has to answer a request before it counts as stuck.
const NO_ANSWER: Duration = Duration::from_secs(3);

/// Where the request to quit stands.
#[derive(Default)]
pub struct Closing(Mutex<Asked>);

#[derive(Default)]
struct Asked {
    /// The page said to quit: let everything through.
    allowed: bool,
    /// When the page was asked, if it was.
    at: Option<Instant>,
    /// The page answered since it was asked.
    heard: bool,
}

/// Called when the window is to close or the app to quit: `true` lets it
/// go ahead, `false` holds it back while the page is asked.
pub fn may_close<R: Runtime>(app: &AppHandle<R>) -> bool {
    let closing = app.state::<Closing>();
    let mut asked = closing.0.lock().unwrap_or_else(PoisonError::into_inner);
    if asked.allowed {
        return true;
    }
    match asked.at {
        Some(at) if !asked.heard && at.elapsed() >= NO_ANSWER => {
            eprintln!("the page did not answer the request to quit; quitting without it");
            asked.allowed = true;
            return true;
        }
        Some(_) if !asked.heard => {} // asked a moment ago: ask again
        _ => {
            asked.at = Some(Instant::now());
            asked.heard = false;
        }
    }
    if let Err(e) = app.emit("quit-requested", ()) {
        eprintln!("the page cannot be asked to save before quitting: {e}");
        asked.allowed = true;
        return true;
    }
    false
}

/// The page heard the request to quit and is saving.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri passes command arguments by value"
)]
pub fn quit_heard(closing: State<'_, Closing>) {
    closing
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .heard = true;
}

/// The page saved the open note, or the user chose to quit without saving.
#[tauri::command]
#[allow(
    clippy::needless_pass_by_value,
    reason = "Tauri passes command arguments by value"
)]
pub fn quit_app(app: AppHandle, closing: State<'_, Closing>) {
    closing
        .0
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .allowed = true;
    app.exit(0);
}
