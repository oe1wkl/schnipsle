# ADR-0011: Version history and trash inside the vault

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: F-14, F-15, N-04, [format.md §7–8](../format.md#7-version-history)

## Context

Notes are often used as scratch pads, deleted, and later found to contain something useful. Nothing the user writes may be lost silently, and recovery must work across devices.

## Decision

- **Snapshots** of a note are stored in `.notesapp/history/<note-id>/` as zstd-compressed Markdown, taken: after a typing pause (default 30 s), on close, before delete, before an import or tag rename changes the note, and when an external change or sync conflict arrives.
- **Retention:** all snapshots for 7 days, then one per day for 3 months, then one per week; configurable. Thinning runs in the background.
- **Trash:** deleting moves the note file to `.notesapp/trash/` with its history untouched. The trash is the built-in smart folder `in:trash`, full-text searchable. Default: never emptied automatically.
- **Restore** brings a note back under its original id; any snapshot can also be restored as a new note.
- History and trash live **inside** the vault, so they sync and recovery works on any device.
- Attachments are deleted only when no note, snapshot or trashed note references them (reference counting during index build).

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Snapshots in vault (chosen) | Works on iOS, syncs, simple | Own retention logic |
| Git repository behind the vault | Mature, diff tools | No reliable Git on iOS; `.git` inside iCloud Drive risks corruption |
| History only in local database | Fast | Lost with the cache, not synced |

## Consequences

- Vault size grows; compression and thinning keep it small (text compresses roughly 3–5×).
- The history UI needs a timeline, diff view and restore actions.
- A Git export of the whole vault can be added later as an extra.
