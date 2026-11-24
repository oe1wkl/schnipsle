# ADR-0046: Unchanged files are not read again to list notes or find copies

- Status: Accepted
- Date: 2026-11-23
- Deciders: Willi Kraml
- Related: N-03, ADR-0006, ADR-0009, ADR-0044, ADR-0045, format.md §4, §5.2

## Context

The first measuring run on the Mac (ADR-0045, `docs/design/performance.md`), with 10,000 notes in iCloud Drive, missed three limits:

| What | Measured | Limit |
| --- | --- | --- |
| Opening a note | 1.43–1.68 s in Rust | 100 ms |
| All notes | 0.16–0.98 s in Rust, 0.31–1.46 s until shown | 200 ms |
| Start, until the note list shows | 1.70 s | 1 s |

Two pieces of the core read every note file:

1. **The list of all notes** (`Vault::list_notes_by`) opens each file to read its title (format.md §5.2) and `pinned` (§4). Right after a start, with iCloud's file handling cold, this takes about 0.9 s for 10,000 notes. The start waits for it.
2. **Copies of a note** (`Vault::copies_of`, used by `read_note`) are the other notes with the same `id` (§4). iCloud Drive puts them in the note's folder, so every other note in that folder is read to compare its `id`. With all notes in `notes/`, opening one note reads 10,000 files.

The search index (ADR-0009) already stores, per file, the modification time and size the sync compares, and the title, `pinned` and `id`. Tag folders and search already list notes from it. ADR-0009 makes the index a rebuildable cache, never the truth. ADR-0044 already lets the index answer what a file's `id` was.

## Decision

**Where the index has seen a file with the same modification time and size as on disk now, its title, `pinned` and `id` are taken from the index instead of reading the file. Every other file is read, as today.**

1. **A snapshot, never the truth.** The app keeps a read-only snapshot of what the index stores per active note: modification time, size, title, `pinned` and `id`. It is taken when the index of a vault opens and after each full sync. The core gets it through the lookup that already finds notes by `id` (`NotesById`).
2. **Checked against the file each time.** The list and the copy search still list the folders on disk. For each note file they compare its current modification time and size with the snapshot; only an exact match uses the snapshot. A file that is new, changed, or missing from the snapshot is read. A stale snapshot therefore costs reads, never wrong answers. This is the same test the index sync itself uses to skip unchanged files.
3. **Copies:** a file in the note's folder is skipped only if it matches the snapshot and the snapshot's `id` differs from the note's. If the note's own `id` is not a valid ULID, the index cannot have recorded it (it stores valid ids only), so every file in the folder is read, as today. Copies the index knows in other folders are read, as today.
4. **No index, no change.** Without an index (none yet, or it failed to open), everything is read, as today. Writes never use the snapshot: they keep checking the content hash against the file (format.md §10).
5. **Long lists are drawn in steps.** The UI draws the first screenful of a list at once and the rest over the next frames, so the time until the list shows does not grow with the number of notes.

No file format changes.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| A: snapshot of index facts, checked against each file's time and size (chosen) | Reads only new or changed files. Works while a sync holds the index. Files stay the truth; a stale snapshot only costs reads. | A snapshot of about 1.5 MB per 10,000 notes. A change that keeps both the modification time (to the millisecond) and the size is not seen until the file is read for another reason; the index sync has the same limit. |
| B: list all notes straight from the index, as tag folders do | Simplest; one query. | Notes not yet indexed and iCloud placeholders are missing. The index is busy for the whole first sync (14.7 s for 10,000 notes), so the list would wait or show nothing. |
| C: ask the index with its lock held, as tag folders do | No snapshot to keep. | Waits for a running sync: up to 0.2 s on each start, 14.7 s on a first open. |
| D: copies only by file name (`Name 2.md`) | No index needed. | Other sync tools name copies differently (ADR-0006: Syncthing, Nextcloud later); a renamed copy would be missed. |
| E: keep reading every file | Nothing to change. | Opening a note stays above 1 s at 10,000 notes; the limits of N-03 are missed. |

## Consequences

- Positive: opening a note, the list of all notes and the start no longer read every note file. The time grows with the number of new or changed files, not with the size of the vault.
- Negative: the app holds a second copy of a few index columns in memory. A change elsewhere that keeps a file's modification time and size shows its old title in All notes until the file is indexed again; tag folders and search behave the same today.
- Follow-up work: measure again on the Mac and the iPhone (ADR-0045) and record the results in `docs/design/performance.md`.
- Implemented in `Vault::list_notes_by` (`vault.rs`) and `Vault::copies_of` (`conflicts.rs`) with `KnownNote` and `Index::known_notes`; the app's snapshot in `src-tauri/src/indexing.rs`; drawing in steps in `ui/src/main.ts`.
