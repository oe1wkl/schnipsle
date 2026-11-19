# Data safety: what is tested and what is checked by hand

N-04 asks that no text is ever lost. ADR-0042 says how: whenever the app replaces or leaves behind a version of a note, that version is stored first, and if it cannot be stored, nothing is replaced and the user is told. This page lists what tests that, where it runs, and what has to be checked by hand before a release.

## Automatic tests

| Test                                                  | What it does                                                                                                                                                                                                                                                                                                                                                                                                                                                       | Where it runs                                                           |
| ----------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ----------------------------------------------------------------------- |
| `crates/core/tests/data_safety.rs`, `random_runs`     | Eight runs of 300 random steps: the app types, saves, settles conflicts ("Keep mine", "Use other version"), takes snapshots, renames after a title change, deletes, restores from the trash and the history, and settles same-`id` copies; other devices change, delete, rename, copy and create notes behind its back. After every step of the app, every version on disk before must still be a note, a snapshot or in the trash, unless the user saved over it. | `cargo test`, every CI run                                              |
| `data_safety.rs`, `long_random_runs`                  | The same, 24 runs of 3000 steps (about 7 minutes).                                                                                                                                                                                                                                                                                                                                                                                                                 | CI job "Data safety, long random run"                                   |
| `crates/core/tests/faults.rs`, `killed_while_saving`  | A child process saves, snapshots, creates, deletes and restores without end and is killed (`SIGKILL`) 25 times at random moments. The note is always a complete version, never older than before; every snapshot is readable; every note created is a note or in the trash.                                                                                                                                                                                        | `cargo test`, every CI run                                              |
| `faults.rs`, `read_only_vault`                        | A vault whose folders may not be written to: save, create, delete and snapshot fail with "permission denied" and change nothing.                                                                                                                                                                                                                                                                                                                                   | `cargo test` (skipped where permissions are not enforced, e.g. as root) |
| `faults.rs`, `full_disk`                              | A 2 MB disk image filled to the last byte: save, create and snapshot fail with "no space left" and change nothing; once there is room, the same save works.                                                                                                                                                                                                                                                                                                        | By hand on a Mac, see below                                             |
| Unit tests in `settle.rs`, `leftovers.rs`, `trash.rs` | The conflict choices for notes with and without `id`, deleted elsewhere, changed again, or with a history that cannot be written; leftover temporary files kept in the history or removed.                                                                                                                                                                                                                                                                         | `cargo test`                                                            |

`SCHNIPSLE_SEED=<n>` repeats a random run with the seed a failure printed:

```sh
SCHNIPSLE_SEED=1007 cargo test -p schnipsle-core --release --test data_safety long_random_runs -- --ignored --nocapture
```

## Before a release

Run the long tests:

```sh
cargo test -p schnipsle-core --release --test data_safety -- --ignored --nocapture
cargo test -p schnipsle-core --test faults -- --ignored --nocapture
```

The second runs the full-disk test (macOS, `hdiutil`); it attaches and detaches a disk image in a temporary folder.

Then check in the release build of the app, on the Mac, in a test vault (not your own notes):

1. **Conflict, Keep mine.** Open a note and type. Before it saves (within a second), change the same note in another editor. The conflict banner shows. Press "Keep mine": your text is the note, and the other version is the newest entry in its history.
2. **Conflict, Use other version.** As 1, but press "Use other version": the note shows the other text, and your text is the newest entry in its history.
3. **A note without front matter.** As 1 and 2 with a note whose front matter the other editor removes. Both choices work as above, and the history shows the version not chosen.
4. **Deleted elsewhere.** Open a note and type; delete its file in the Finder before it saves. "Keep mine" writes it again. Repeat with "Use other version": the note goes, and your text is in the Trash under the note's title; Undo brings it back.
5. **Staying on the note.** Make a save fail: in the Finder, set the note's folder to read-only (Get Info, Sharing & Permissions), then type. The status line says the note was not saved. Now open another note, press New, open a note in the Trash, go back to the list, copy a version from the history, and pick another vault: each time the note stays open and the status line says why. Set the folder back to read and write; the next save works and leaving works again.
6. **Quitting.** Type and press ⌘Q at once: the app saves, then quits; the text is in the note. With a save that fails (as in 5) or with the conflict banner showing, ⌘Q, ⌘W, the window's close button, Quit in the Dock and `osascript -e 'tell application id "cc.kraml.schnipsle" to quit'` ask "Quit without saving?". Return and Esc cancel; "Quit Without Saving" quits. Logging out waits for the same answer: Cancel cancels the logout, "Quit Without Saving" lets it go on.
7. **Leftover temporary files.** With the app closed, put a file `.Note.md.tmp-1-1` with other text and the note's `id` next to a note, and set its date more than an hour back (`touch -t 202601010000 .Note.md.tmp-1-1`). Open the vault: the file is gone, and its text is the newest entry in the note's history.

On the iPhone the app saves when it goes to the background; item 5 applies there too (a vault in iCloud Drive that is full).

## Two devices offline (C7, C8)

The M1 checks C7 and C8 are run again with the 1.0 release build: two devices edit the same note while one or both are offline, then go online. Each version must end up as the note, in its history, or in the conflict banner; none may be lost. The results go into the roadmap's N-04 line.
