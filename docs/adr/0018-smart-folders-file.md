# ADR-0018: Reading and writing `smart-folders.yaml`

- Status: Accepted
- Date: 2026-10-28
- Deciders: Willi Kraml
- Related: ADR-0006, ADR-0008, ADR-0015, [format.md §9](../format.md#9-smart-folders), [§10](../format.md#10-writing-rules)

## Context

Smart folders (F-10) are saved searches in one vault file, `.notesapp/smart-folders.yaml`, which syncs to every device (ADR-0006). format.md §9 gave the keys but not how to read entries that are incomplete or written by another tool, how folders are ordered, or what a writer does when the file changed elsewhere. For a note, §10 says to keep both versions; for one shared list of folders, two versions side by side would confuse more than help, and nearly all concurrent edits touch different folders. The file is nested YAML, which the flat reader of `vault.yaml` cannot handle.

## Decision

- The file is read and written with **`yaml-rust2`** (MIT OR Apache-2.0), which keeps the order of keys. The app works on the YAML tree itself, so keys, values and entries it does not understand are written back unchanged; only comments and formatting are lost.
- **Lenient reading** (format.md §9 rules 1–4): `id`, `name` and `query` are required; an entry without them, or repeating an id, is hidden but kept in place. A missing or cyclic parent shows the folder at the top level, an unknown `sort` shows as `modified-desc`; both stay as written until changed. The order of `folders` is the order among siblings, so reordering and nesting by drag and drop is a move in the list plus a `parent` change.
- **Edits are operations, applied to the current file** (rule 7): add, change name, query or sort, remove, move. Each edit reads the file, applies the operation and writes the result atomically and coordinated (§10, ADR-0015), replacing the file only if its hash is still the one just read. If it is not, the operation is applied again to the new content, up to three times. A change from another device is therefore never overwritten, and the user's change is not dropped.
- Removing a folder moves its subfolders to its parent: a saved search is never deleted as a side effect.
- A newer `format` or a file that cannot be parsed is never written.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Operations re-applied to the current file (chosen) | Both changes kept; no conflict dialog for a settings-like list | An operation on a folder removed elsewhere fails and must be reported |
| Keep both versions, as for notes | Same rule as §10 | Two smart-folder lists to merge by hand for a minor clash |
| Last writer wins | Simple | Silently loses folders made on another device |
| `serde_yaml` into typed structs | Less code | Unmaintained; unknown keys and odd values dropped unless modelled one by one |

## Consequences

- Positive: other tools may add keys and entries without the app destroying them; edits from two devices merge.
- Negative: comments in the file disappear on the first save; an edit can run more than once, so it must not have side effects outside the file.
- Follow-up work: the UI (M3) reports a failed operation (for example a folder removed elsewhere) and refreshes the list.
