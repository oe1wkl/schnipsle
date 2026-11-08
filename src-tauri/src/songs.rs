// SPDX-License-Identifier: Apache-2.0
//! Song sheets (format.md §5.7, ADR-0030, ADR-0031) as commands: the editor
//! shows a `chordpro` block as the sheet the core renders, and "Apply
//! transposition" asks the core for the rewritten block.

use schnipsle_core::chordpro::chord::Notation;
use schnipsle_core::chordpro::sheet::{self, Sheet, View};
use tauri::State;

use crate::{AppState, CmdResult, with_vault};

/// The sheet of a `chordpro` block: `source` its content, `info` its info
/// string. `transpose`, `lyrics_only` and `shown` are the viewer's choices;
/// a block without `notation=` is read in the vault default.
#[tauri::command]
pub async fn render_song(
    state: State<'_, AppState>,
    source: String,
    info: String,
    transpose: i32,
    lyrics_only: bool,
    shown: Option<Notation>,
) -> CmdResult<Sheet> {
    let notation = with_vault(&state, |v| Ok(v.chord_notation())).unwrap_or_default();
    let view = View {
        transpose,
        lyrics_only,
        notation,
        shown,
    };
    Ok(sheet::render(&source, &info, view))
}

/// "Apply transposition" (§5.7 rule 5): the block's content with its chords
/// and `{key}` moved by `semitones`, in the names it is written in. The
/// editor stores a history snapshot before it puts the result in the note.
#[tauri::command]
pub fn apply_transposition(source: &str, semitones: i32, notation: Notation) -> String {
    sheet::apply_transposition(source, semitones, notation)
}
