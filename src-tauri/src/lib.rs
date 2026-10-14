// SPDX-License-Identifier: Apache-2.0
//! Schnipsle app shell: exposes `schnipsle-core` to the UI as Tauri commands.
//!
//! The UI never touches vault files; every file operation goes through a command here (ADR-0003).

use schnipsle_core::Tag;

/// Returns the hashtags found in a Markdown text (format.md §5.3).
#[tauri::command]
fn extract_tags(markdown: &str) -> Vec<Tag> {
    schnipsle_core::extract_tags(markdown)
}

/// Builds and runs the application. Also the iOS/Android entry point.
///
/// # Panics
/// Panics if the Tauri runtime cannot start (e.g. no `WebView` available).
#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .invoke_handler(tauri::generate_handler![extract_tags])
        .run(tauri::generate_context!())
        .expect("error while running Schnipsle");
}
