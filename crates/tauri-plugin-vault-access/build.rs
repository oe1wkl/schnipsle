// SPDX-License-Identifier: Apache-2.0
//! Builds the Swift package in `ios/` when targeting iOS.

// No commands are exposed to the web view: the app calls this plugin from Rust only.
const COMMANDS: &[&str] = &[];

fn main() {
    tauri_plugin::Builder::new(COMMANDS).ios_path("ios").build();
}
