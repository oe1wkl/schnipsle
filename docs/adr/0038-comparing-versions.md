# ADR-0038: Comparing versions in the history

- Status: Accepted
- Date: 2026-11-17
- Deciders: Willi Kraml
- Related: F-14, ADR-0011, [format.md §7](../format.md#7-version-history)

## Context

F-14 asks to browse, compare and restore versions of a note. Browsing and restoring exist (ADR-0011); ADR-0011 already foresees a diff view. Before restoring or copying a version, the user wants to see what it changes: against the note as it is now, or against another version. The view has to work on the Mac and on a phone (about 375 pt wide), and must not hang on a long note.

Snapshots are complete note files (format.md §7), so two versions often differ in `modified` alone, or in the front matter only.

## Decision

The history view compares the selected version with the note as it is now, or with another version of the same note; the older text is always the "before" side. Comparing reads only; it writes nothing.

1. **One column.** The comparison is a unified view, not two columns side by side: lines removed and lines added, one under the other, with the words changed inside a line marked. It fits a phone. A summary on top counts the lines added and removed.
2. **Source text.** Both sides are compared and shown as Markdown source, not rendered; what changed in a link or a chord is visible.
3. **Front matter apart.** The body is compared line by line. The front matter is compared key by key and shown above it as a short list (`modified: 17 Nov 10:02 → 10:31`); keys only one side has appear as added or removed. A comparison where only `modified` differs says so.
4. **Context folded.** Unchanged lines more than three away from a change are folded into a line "… 42 unchanged lines" that unfolds on a click. Lines are numbered as in the source view, front matter included: removed lines with their number in the older version, the others with their number in the newer one. A sign (−, +) beside each changed line keeps the view readable without colour.
5. **In the core.** `schnipsle-core` computes the comparison (lines, then words inside paired changed lines) and hands the UI a finished structure; the UI only draws it. The diff engine is the `similar` crate (Apache-2.0; its only dependency here, `unicode-segmentation`, was already in the build), with the patience algorithm for lines, Unicode word boundaries inside lines, and a time limit of half a second, after which it gives a coarser but still correct result. Removed and added lines in one change are paired in order; a pair with less than half its words in common is shown whole, without word marks.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Unified, source text, in the core (chosen) | Fits a phone; exact; tested in Rust; same on every platform | Markdown not rendered in the comparison |
| Two columns side by side | Familiar from code tools | Too narrow on a phone; two layouts to keep |
| Rendered Markdown with changes marked | Reads like the note | Marking changes in rendered HTML is fragile (tables, scores, song sheets); hides changes in markup |
| Diff in TypeScript (jsdiff) | No IPC round trip per comparison | Logic outside the core (against CLAUDE.md "core first"); no Rust tests |
| Own Myers implementation | No dependency | More code to test; `similar` is small, widely used and licence-clean |

## Consequences

- Positive: before restoring, the user sees what changes; versions that differ only in `modified` are recognisable as such.
- Negative: a new dependency (`similar`); scores and song sheets are compared as their source text.
- Follow-up work: the comparison in the manual's history chapter; comparing a trashed note's versions comes with the trash view if wanted.
