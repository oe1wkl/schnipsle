# ADR-0026: A first heading of any level gives the title

- Status: Accepted
- Date: 2026-11-05
- Deciders: Willi Kraml
- Related: F-01, ADR-0020, ADR-0021, [format.md §5.2](../format.md#52-title)

## Context

format.md §5.2 took a note's title from a leading level-1 heading, and otherwise from its first line as written. A note that starts with `## Field Day checklist` was therefore titled "## Field Day checklist": with the marks in the note list, in the file name the app gives it after an edit, in `[[…]]` links, and in exports, which are named `## Field Day checklist.docx` and carry that title in their metadata. 13 of the 538 notes imported from Apple Notes and Simplenote start like that, because those apps let a note start with a smaller heading.

## Decision

The title is the text of an ATX heading of any level, one to six `#`, if it is the first non-empty block. `#tag` and `####### text` are no headings and stay as they are. Everything else in §5.2 is unchanged: otherwise the first line, at most 120 characters, otherwise the file name.

In one document of several notes, such a heading becomes the level-1 heading that starts the note's section (ADR-0021 rule 3), instead of being repeated under a new one. A single note is still exported as written.

The index schema goes to version 5, so stored titles are read again.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Any level (chosen) | The list, links, file names and exports show the title a reader sees | A change to the format: other tools reading the vault need the rule |
| Only in exports | No change to the format | The list and `[[…]]` links keep showing the marks |
| Leave it | Nothing to do | Marks in names and titles wherever they appear |

## Consequences

- Positive: notes from other apps read as their authors wrote them, in the list and in exports.
- Negative: a note whose title changes this way is renamed after its next edit (§3), as with any change of title. Before that, a `[[## Title]]` link no longer finds it, but no such link exists in the notes imported so far.
- Follow-up work: format.md 0.14; the conformance cases `tests/fixtures/format/5.2-title/titles.tsv`.
