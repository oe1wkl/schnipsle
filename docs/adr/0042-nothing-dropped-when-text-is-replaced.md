# ADR-0042: Nothing dropped when a version is replaced or a note is left

- Status: Accepted
- Date: 2026-11-19
- Deciders: Willi Kraml
- Related: N-04, ADR-0006, ADR-0011, format.md §4, §7, §10; roadmap M7 "Data-safety tests (N-04)"

## Context

N-04 asks for a long run of random edits, renames, deletes, restores and outside changes against a model of the vault, which must never lose text, and for tests of a kill while saving, a full disk and a vault that cannot be written. These tests now exist (`crates/core/tests/data_safety.rs`, `crates/core/tests/faults.rs`). The core's own writes held:

- 25 kills (`SIGKILL`) while saving, snapshotting, deleting and restoring never left a note cut short or missing, and every snapshot stayed readable;
- on a full disk (a 2 MB disk image) and in a vault without write permission, every write failed with a clear error and changed nothing;
- in 80 random runs of 300 steps, deleting, restoring from the trash and from the history, renaming, and settling same-`id` copies never lost a version.

They found three ways text is lost today:

1. **The conflict banner and notes without an `id`.** "Keep mine" stores the version on disk in the history before replacing it, and "Use other version" stores the editor's text first, both by `snapshot_note`, which files a snapshot under the note's `id`. When neither the text nor the file has one (a note made by another program, or one whose front matter another program removed), `snapshot_note` stores nothing and says so only by returning `None`. The UI goes on: the other version, or the user's text, is gone. Found in 18 of 80 runs.
2. **Leaving a note with unsaved text.** When a save fails (a full disk, no permission) or the conflict banner is shown, opening another note, a new note, a note in the trash or another vault loads the other note into the editor; the unsaved text is dropped without a word. Delete and Restore already refuse in this state ("resolve the conflict first"). Found by reading `leaveNote` in `ui/src/main.ts`; the Rust model cannot run the UI.
3. **Quitting.** ⌘Q and closing the window do not wait for the autosave (up to 1 s of typing) and do not ask when text could not be saved. On the iPhone the app saves when it goes to the background, which covers it.

Not a loss, but left behind: a kill during a write leaves its temporary file (`.name.md.tmp-<pid>-<n>`, 18 after 25 kills). The app passes over such files, but they pile up and are synced. A complete one may hold the last text typed before a crash.

## Decision

**Whenever the app replaces or leaves behind a version of a note, that version is stored first, and if it cannot be stored, nothing is replaced and the user is told.**

1. **Conflict choices move into the core.** `Vault::keep_mine(path, text, other_hash)` and `Vault::use_other(path, text)` do what the banner's buttons do now, as one call each, and fail without writing if the version they must keep cannot be stored:
   - The snapshot goes under the `id` the note has afterwards, so the history is reachable from the note, as ADR-0011 asks. "Keep mine" files the other version under the `id` of the editor's text, else that of the file; if neither has a valid one, the text gets a new `id`. "Use other version" files the editor's text under the file's `id`; if the file has no valid one, the text is filed under its own `id` (or a new one), and then that `id` and `created` are written into the file's front matter (as for any write, format.md §4; only the front matter changes, checked against its hash). The snapshot comes first, so a failure leaves the note as it was.
   - If the note was deleted elsewhere, "Keep mine" writes it again (as now) and "Use other version" puts the editor's text into the trash under its old path (format.md §8), so it can be restored.
   - `snapshot_note` keeps its meaning for the typing pause, where nothing is replaced.
2. **A note with unsaved text is not left.** After a failed save or while the conflict banner is shown, opening another note (also from a link, an import or the list of imports), a new note, a note in the trash, a copy of a version, going back to the list and opening another vault stay on the note and say why ("The note stays open, so your changes are not lost: …"). This is what Delete and Restore do already. An export does not leave the note; it already warns that the note's latest changes are not in it.
3. **Quit and Close wait for the save.** On the Mac, ⌘Q and ⌘W (and the window's close button) save first. If text is still unsaved, the app asks: "Quit without saving? Your changes to ‹title› could not be saved: …" with Cancel as the default. A page that does not even answer the request within 3 seconds is taken to be stuck, and asking again quits, so the app can always be quit.
4. **Leftover temporary files are kept or cleared on open.** When a vault is opened, temporary files older than an hour in `notes/`, `attachments/` and `.notesapp/` are removed in the background. One may hold the complete new text (killed after it was flushed) or only its start; which, cannot be told. So the temporary file of a note is first stored as a snapshot of that note if it has a valid `id` and differs from the note and from the last snapshot; if that fails, the file stays for the next time. A temporary file younger than an hour may belong to a write in progress on another device and stays. Temporary files elsewhere are copies of text that is still in place (a snapshot, a trash record, settings), and are removed.
5. **The tests stay.** `data_safety.rs` runs eight short random runs, and `faults.rs` the kill test and the read-only vault, in every `cargo test`. The long random run (24 runs of 3000 steps, about 7 minutes) runs in CI in a job of its own. The full-disk test needs `hdiutil` and is run by hand before a release; `docs/design/data-safety.md` lists what to run and what to check by hand.

## Consequences

- The random runs pass only with rule 1 (`Vault::keep_mine`, `Vault::use_other` in `crates/core/src/settle.rs`). Rule 4 is `Vault::clear_leftovers` (`crates/core/src/leftovers.rs`) with unit tests. Rules 2 and 3 are in the UI and are checked by hand, with a list in `docs/design/data-safety.md`.
- A note made by another program gets front matter in one more case: when its conflict is settled with "Use other version". Today it gets one at its first save anyway.
- `snapshot_note` returning `None` no longer decides whether text survives; the UI's `snapshot()` helper is kept for the typing pause, "Apply transposition" and a change from elsewhere, where the app replaces nothing itself.
- The window close and quit paths need a small handshake between Rust and the page (`CloseRequested` and `ExitRequested` held back, a `quit-requested` event, `quit_heard` and `quit_app`; `src-tauri/src/closing.rs`). It is desktop only. Quit (⌘Q) is the app's own menu item, which asks the page the same way: the system's Quit item calls `terminate:` at once, and tao (0.37) does not let the app hold that back. For the same reason, Quit from the Dock and logging out still quit without waiting for the save.
- The vault format does not change: format.md §10 already asks for temporary files next to their targets, and §8 for the trash as written here.
- C7 and C8 (two devices, offline) are not part of this ADR. They are run again with the 1.0 release build, as the roadmap says.

## Alternatives considered

- **Leave with the unsaved text in the history** instead of staying on the note. Rejected: when saving fails, storing a snapshot usually fails too (a full disk), and a user who finds the note unchanged would not know to look in the history.
- **A history folder under a new `id` that no note carries** for a note without one. Rejected: nothing in the app would ever show it.
- **Removing leftover temporary files unseen.** Rejected: a complete one can be the last text before a crash.
