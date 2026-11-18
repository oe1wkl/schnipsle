# ADR-0040: Renaming and merging tags

- Status: Accepted
- Date: 2026-11-18
- Deciders: Willi Kraml
- Related: F-11, N-04, ADR-0006, ADR-0008, ADR-0011, ADR-0018, [format.md §5.3, §7, §9, §10](../format.md#53-tags)

## Context

Tags live only in note bodies (ADR-0008), so renaming one means editing every note that has it. ADR-0008 already says this is a bulk edit with history snapshots and undo, and ADR-0011 already takes a snapshot "before an import or tag rename changes the note". What is still open:

- which text exactly is changed;
- what happens to subtags and to smart folders;
- how the user sees the change before it happens;
- how a rename is undone.

The edit touches many notes at once, possibly while sync or another device changes some of them, so it must follow the writing rules (format.md §10) for each note and never lose text (N-04).

## Decision

**A rename is a find-and-replace of tag names, limited to what format.md §5.3 recognizes as tags. A merge is the same rename, into a tag that already exists.** It is shown as a report first, written note by note with a snapshot of each, and can be undone while the app runs.

1. **What is renamed.** The user renames tag `old` to `new`. Every tag in a note body whose key (§5.3: NFC, lowercase) equals `old`'s key, or starts with it followed by `/`, is changed. Only the part matching `old` is replaced: `#Projects/Tuner` renamed `projects` → `Work` becomes `#Work/Tuner`.
   - Text that §5.3 does not take as a tag stays exactly as it is: code, links, `\#`, headings and colour codes.
   - Everything else in the note stays byte for byte.
   - Notes in the trash and versions in the history keep the old tag; they are not edited.
2. **What the new name may be.** `new` must be a valid tag name by §5.3 rules 1, 2 and 5. It must not be `old` itself or one of `old`'s subtags; `projects` → `projects/old` would rename forever. A rename that changes only the spelling, such as `radio` → `Radio`, is allowed and rewrites every spelling to the new one. That is how the user picks the display form (§5.3).
3. **Merging.** If `new` already exists, the notes of both tags end up under `new`. A note that already had both keeps both occurrences, now spelled the same: the rename only replaces text and never removes it. The report says that these are merges.
4. **Smart folders.** Tags in smart-folder queries (`.notesapp/smart-folders.yaml`, §9) are renamed by the same rule, written as §9 rule 7 says. The report lists the folders that change.
5. **Report first.** Before writing, the app shows:
   - how many notes and occurrences change, with the note titles;
   - which smart folders change;
   - whether the tag is merged into an existing one.

   Nothing is written until the user confirms.
6. **Writing.** Each note is written as in an ordinary save:
   - **Hash check and conflict:** the content hash is checked against the one read for the report. A note changed since is not written, and the result lists it as skipped.
   - **Snapshot:** a snapshot is taken first (§7, ADR-0011).
   - **Front matter:** `modified` is updated, since the content changed (§4).

   A note that cannot be written is skipped and reported; the others are still renamed.
7. **Undo.** After a rename the result offers Undo, which goes through the renamed notes:
   - **Unchanged since:** a note whose hash is still the one the rename wrote gets its snapshot back. The current version is first stored as a snapshot, as a history restore does.
   - **Changed since:** the note is kept and listed.
   - **Smart folders:** they are renamed back.

   Undo is offered until the app quits or the vault closes; it is not stored in the vault. After that, every note's earlier version stays in its history.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Rename in the parse tree, by source position (chosen) | Changes only real tags; same rule as extraction | Needs exact source offsets from the parser |
| Plain text search for `#old` | Simple | Also changes code, links and `\#old`; violates §5.3 |
| Undo kept in memory (chosen) | No new file in the vault, no format change | Gone after quitting; then only per-note history |
| Undo log in the vault, like the import log (§12.2) | Undo after a restart, on another device | New file type, format.md change, fixtures; more than F-11 needs |
| Merge removes the old tag where the note has both | No duplicate `#x … #x` | Removes text the user wrote |

## Consequences

- Positive:
  - A rename is safe in the same way as a save: snapshots, hash checks, conflicts kept.
  - Smart folders keep working after a rename.
  - Spelling-only renames give the user control over the display form.
- Negative:
  - Undo does not survive a restart; after that, notes are restored one by one from history.
  - A merge can leave a tag written twice in a note.
- Follow-up work:
  - `schnipsle-core`: a `retag` module with plan, apply and undo, plus tests covering every row of the §5.3 table.
  - Tauri commands.
  - In the sidebar: a Rename… button on tag folders, a dialog with the report, and Undo.
  - The manual's section on tags.
