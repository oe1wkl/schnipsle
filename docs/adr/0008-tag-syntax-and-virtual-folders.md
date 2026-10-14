# ADR-0008: Tag syntax, tag hierarchy and combination folders

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: F-07, F-08, F-11, [format.md §5](../format.md#5-tags)

## Context

Organization is by hashtags, not physical folders. A note with several tags must appear in several folders, and tag combinations must appear as subfolders. Users also want deliberate structure.

## Decision

1. **Tags are inline only** (written in the note body). No `tags:` in front matter in v1.
2. **Syntax** as specified in `format.md` §5: `#` + tag characters at a word boundary, at least one letter, `/` separates hierarchy levels, not inside code, not a heading marker, not a hex colour code containing a digit (`#00f`), not after an escape (`\#`).
3. **Both folder mechanisms** are provided:
   - **Hierarchical tags:** `#projects/tuner` shows as `projects › tuner`. A note tagged `#projects/tuner` also counts for `#projects`.
   - **Combination subfolders:** under tag A, every tag B that co-occurs with A in at least one note is a subfolder `A › B` = notes having A **and** B. Paths are intersections and therefore order-independent.
4. Combination depth is limited (default 3, configurable); only non-empty combinations are shown, each with a count.
5. Grouping is case-insensitive (NFC, then Unicode lowercase mapping); the most frequent spelling is displayed.
6. Tag folders are implemented as queries on the same engine as smart folders (`#a #b`).

## Consequences

- Rename and merge of tags (F-11) rewrite note bodies; this is a bulk edit with history snapshots and undo.
- The sidebar must distinguish visually between hierarchy (`/`) and combination (`›` with intersection icon).
- Tag extraction is part of the Markdown parse in the core; golden-file tests cover the edge cases listed in `format.md`.
