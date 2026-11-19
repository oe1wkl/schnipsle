// SPDX-License-Identifier: Apache-2.0
//! The text of the desktop menu (ADR-0041 rule 4), in one table per
//! language; English only in 1.0. The menu is built before the web view, so
//! it cannot use the UI's catalog (`ui/src/strings/en.ts`). The standard items
//! (Undo, Copy, Quit, …) keep the texts the menu library gives them.

/// The labels of the menu's own items and submenus.
pub struct MenuText {
    pub file: &'static str,
    pub edit: &'static str,
    pub window: &'static str,
    #[cfg_attr(target_os = "macos", allow(dead_code))]
    pub help: &'static str,
    pub new_note: &'static str,
    pub open_vault: &'static str,
    pub import: &'static str,
    pub imports: &'static str,
    pub export: &'static str,
    pub settings: &'static str,
}

const EN: MenuText = MenuText {
    file: "File",
    edit: "Edit",
    window: "Window",
    help: "Help",
    new_note: "New Note",
    open_vault: "Open Vault…",
    import: "Import…",
    imports: "Imports…",
    export: "Export…",
    settings: "Settings…",
};

/// The menu's text in the user's language; a second table is picked here by
/// the system language once there is one.
pub fn menu() -> &'static MenuText {
    &EN
}
