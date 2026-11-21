# ADR-0044: A note keeps its `id` when another program drops it

- Status: Accepted
- Date: 2026-11-21
- Deciders: Willi Kraml
- Related: N-04, ADR-0011, ADR-0042, format.md §4, §7, §10

## Context

A note's history is filed under its `id` (format.md §7). The id is permanent once the app has written it (§4). But another program can still drop it: an editor that rewrites the file without its front matter, or a sync tool that brings such a version from another device.

The N-04 hand checks on 2026-11-21 showed two ways a note then loses its link to its past. No text was lost either way.

1. **The change is taken in, and the next save gives a new `id`.** When the app has no unsaved text for the note, it stores the old version in the history and shows the new one. When the note is saved next, `stamp_for_save` finds an `id` neither in the text nor on disk and makes a new one. From then on the history under the old `id` is on disk but no longer shown. A note that is not open gets a new `id` the same way, on its next save.
2. **"Use other version" keeps the `id`, but not `created`.** `Vault::use_other` rightly files the editor's text under that text's `id` and writes that `id` into the file (ADR-0042 rule 1). It takes `created`, however, from the file's birth time. The file is often rewritten in place, so the note's creation time jumped from 20 to 21 November.

The search index already remembers each note's `id` and `created` per path (`index.rs`, columns `id` and `created`). So does the editor, for the version it loaded last.

## Decision

**A note whose file loses its `id` through a change made elsewhere gets back the `id` and `created` it had, unless another note carries that `id` now.**

1. **When the index takes in a changed file** (`sync_index` and the updates after a `vault-changed` event): if the file's last indexed version had a valid `id` and the new version has none, the core writes that `id` and that version's `created` into the file's front matter. Only the front matter changes, checked against the hash it just read, as for any write (§10). If the file has changed again meanwhile, nothing is written, and the next update tries again.
2. **Not if the `id` is taken.** If another note or a note in the trash carries the old `id` now (the other program may have moved the note and left a copy), the file is left alone. It gets a new `id` at its next save, as today.
3. **"Use other version" takes `created` from the editor's text**, as it does the `id`, when it writes them into a file that has neither. Only if the text has no `created` does it fall back to the file's time.
4. **format.md §4 says so:** "If a change made elsewhere removes `id`, the app SHOULD write back the `id` and `created` the file had, unless another note carries that `id`."

This happens for every note in the vault, open or not, and does not depend on the UI.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| A: restore when the index takes in the change (chosen) | Covers every note, open or not. The core has what it needs. The history is reachable at once. | One more write after a change made elsewhere, which syncs back once. |
| B: restore only on the next save (`write_note` asks the index) | No write the user did not cause. | A note that is not edited again stays without `id`, and its history stays hidden until then. `write_note` would need the index, which it does not use today. |
| C: the UI passes the `id` of the version it had loaded | Small change. | Only for the open note. Logic in the UI, against "core first". |
| D: leave it as it is | Nothing to do. | The history of such a note is hidden for good, though it is on disk (the reason ADR-0042 rejected unreachable history folders). |

## Consequences

- Positive: a note keeps its history and its creation time, whatever another program does to its front matter.
- Negative: when another program drops the front matter, the app writes the file once more right after. If that program drops it on every save, each of its saves is followed by one write of the app. That is what happens today at the app's next save anyway.
- The open note: if the user has unsaved text when the change arrives, the conflict banner shows as now. If the `id` is written back while the banner is up, the banner's version is out of date. "Keep mine" then reports that the note changed again and shows the banner with the new version, as for any second change (ADR-0042). "Use other version" files the editor's text under the same `id`.
- Implemented in `Vault::sync_index` and `Vault::index_file` (`crates/core/src/index_sync.rs`) and `Vault::use_other` (`settle.rs`), with unit tests and the fixture `tests/fixtures/format/4-keep-id/`. format.md revision 0.18.
