# ADR-0016: Rewrite relative links when a note moves to another folder

- Status: Accepted
- Date: 2026-10-17
- Deciders: Willi Kraml
- Related: N-04, ADR-0004, ADR-0011, [format.md §5.5](../format.md#55-images-and-attachments-in-the-body), §8

## Context

Notes link attachments and other files with paths relative to the note file (format.md §5.5), e.g. `../attachments/3f/….png`. Some moves change a note's folder depth and so break those links: a restore whose original path is taken falls back to `notes/` (§8), and notes found at the vault root are moved into `notes/`. Format.md §5.4 forbids rewriting note links (`[[Title]]`) silently, but says nothing about file links. Any fix either changes text the user wrote or changes where notes go.

## Decision

When the app moves a note to another folder, it rewrites the relative links in it that resolved before the move, so they resolve to the same files afterwards. The previous text is stored as a snapshot in the note's history first (ADR-0011); a note without `id` gets one for this. URLs, absolute paths, note links, links in code, reference definitions and links that did not resolve stay as written. Files that move along with the note (a whole folder moved in) keep their links unchanged.

format.md §5.5 records this as a writer rule (SHOULD rewrite, MUST snapshot first).

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Rewrite links, snapshot first (chosen) | Images keep working without user action; old text recoverable | Changes the note's text on a move |
| Restore next to the taken path instead of into `notes/` | No rewrite for restores | Changes §8; moves in from the vault root still need rewrites |
| Only warn | Never touches text | Broken images until the user fixes every link by hand |

## Consequences

- Positive: restores and moves never leave broken images behind; the history holds the text as it was.
- Negative: a move can change the note's text and its `modified` time; other devices see a normal edit.
- Follow-up work: future moves (e.g. a "move to folder" command, if one is ever added) use the same function.
