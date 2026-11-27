# ADR-0047: Folders are listed in bulk on Apple platforms

- Status: Accepted
- Date: 2026-11-24
- Deciders: Willi Kraml
- Related: N-03, ADR-0009, ADR-0015, ADR-0045, ADR-0046

## Context

With ADR-0046, listing all notes and the index sync read only new or changed files. They still ask every file for its modification time and size, to tell whether it changed. The first iPhone run (ADR-0045, `docs/design/performance.md`) showed that this check alone misses the limit: All notes took 273 ms until shown (limit 200 ms), and the estimate for the A11 is 1.9 s.

Timing the steps of one listing of 10,000 notes in iCloud Drive on the iPhone 18 Pro, in a test build:

| Step | Time |
| --- | --- |
| `lstat` for every file, as `std::fs` does it (10,000 files) | 231 ms, about 23 µs per file |
| The same `lstat` calls again right after | 75 ms |
| The same spread over 4 or 8 threads | 59 ms |
| `getattrlistbulk(2)` for the top folder (3,888 entries, 3 calls) | 1.5 ms, about 0.4 µs per entry, the same times as `lstat` |
| Reading the folder, comparing with the index, sorting | about 9 ms together |

`getattrlistbulk` returns the names, types, modification times and sizes of many entries of a folder in one call. It exists on macOS since 10.10 and on iOS since 8, and works on every file system there (the kernel falls back to `getattrlist` for each entry where a file system has no bulk support).

## Decision

**On macOS and iOS, the folders of a vault are listed with `getattrlistbulk`, which gives each entry's name, kind, modification time and size. Everywhere else, and if the call fails, the folder is listed with `std::fs::read_dir` and `lstat` as before.**

1. **A lister, passed in like the coordinator.** The core gets a `FolderLister` trait: the entries of one folder, each with its name, kind (file, folder, other) and, where the system could tell, modification time in milliseconds and size. `PlainLister` uses `read_dir` and `lstat`; `schnipsle-apple` adds `BulkLister`. The app shell gives the vault the one for its platform (ADR-0015).
2. **Who uses it:** the list of all notes, the index sync (notes and trash), and the search for copies of a note in its folder (ADR-0046). They decide about each entry as before: the same names are skipped, and an unchanged file is one whose time and size match what the index stored.
3. **Same answers as `lstat`.** Symbolic links are neither files nor folders, as with `lstat`. A time before 1970 counts as 0, as before. An entry whose attributes the system cannot give comes without time and size and is treated like a file whose `lstat` failed. Tests compare both listers on the same folders.
4. **Falls back, never fails harder.** If `getattrlistbulk` fails for a folder, the folder is listed again with `PlainLister`, so errors are the same as today.
5. **`unsafe` stays in `schnipsle-apple`** (ADR-0015): one call, with a `// SAFETY:` comment; reading the returned buffer is safe code.

No file format changes.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| A: `getattrlistbulk` on Apple platforms (chosen) | About 50 times faster per file than `lstat`; the files stay the truth as in ADR-0046. Also speeds up the index sync and the copy search. | Some `unsafe` code and a buffer to parse; a second way of listing to keep equal to the first, checked by tests. |
| B: `lstat` on several threads | Safe Rust, simple. | 59 ms at best; more threads do not help. Still above the limit on the A11. |
| C: trust the file watcher between full syncs, look only at reported files | No lookups at all. | A missed event would show an old title or miss a copy until the next sync; changes ADR-0046's rule that every file is checked. |
| D: keep `lstat` | Nothing to change. | All notes misses its limit on the iPhone 18 Pro and far more on the A11. |

## Consequences

- Positive: All notes, the index sync and opening a note no longer take time per file for the lookup; with 10,000 notes the listing is expected to drop from about 250 ms to under 20 ms on the iPhone 18 Pro.
- Negative: two listers must give the same answers; a difference would show as files read again (harmless) or, if a time were wrong in the other direction, as a change not seen until a file is read for another reason. The tests compare them entry by entry.
- Measured on the iPhone 18 Pro on 2026-11-24 (`docs/design/performance.md`): all notes 57 ms until shown instead of 273 ms, the start 204 ms instead of 583 ms, the index sync 27 ms instead of about 300 ms.
- ADR-0050 adds each entry's flags, which tell a dataless file (`ATTR_CMN_FLAGS`, `st_flags`).
- Implemented in `crates/core/src/listing.rs` (`FolderLister`, `PlainLister`), `crates/apple/src/listing.rs` (`BulkLister`), used by `vault.rs`, `index_sync.rs` and `conflicts.rs`; chosen in `src-tauri/src/lib.rs`.
