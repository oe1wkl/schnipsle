# ADR-0032: Scores in the editor

- Status: Accepted; the controls of rule 6 decided by [ADR-0035](0035-playing-scores.md)
- Date: 2026-11-08
- Deciders: Willi Kraml
- Related: F-16, ADR-0007, ADR-0013, ADR-0031, [format.md §5.6](../format.md#56-music-notation)

## Context

ADR-0013 engraves `abc` blocks with abcjs in the web view, with the source shown while the cursor is in the block. It leaves open:

- when abcjs is loaded: it is about 500 kB minified, as large as the rest of the front end;
- what counts as an unsupported construct (§5.6 rule 3), and how its warning shows without losing text;
- what `nosource` (rule 6) means in the editor;
- how the user gets from the score to the line they want to change.

## Decision

1. **Live preview.** An `abc` block shows as its engraved score unless the cursor is in the block while the editor has focus; then its source shows, as with tables and song sheets (ADR-0007, ADR-0031). Source mode shows the source.
   - **Several tunes:** every tune of the block is engraved, one after the other.
   - **Line breaks:** each staff line is a line of the source, as the author wrote it; the score is scaled to the width of the editor.
   - **Colour:** the score is drawn in the text colour, so it follows the light and dark theme.
   - **What abcjs does not read** is prepared before it parses the block, with every character kept in its place for clicks and warnings:
     - `+:` continuations (ABC 2.1) are joined to their field: the line end, the `+:` and the field's `%` comment become spaces;
     - HTML entities (`&Uuml;`, `&#220;`, `&#xDC;`), Unicode escapes (`\u00dc`, `\U0001F3B5`) and `\&` in fields and in quoted chords and annotations become the characters they stand for, followed by invisible word joiners (U+2060) for the rest of their width. Codes that would change how abcjs reads the line (`&quot;`, `&#37;`) stay as written. abcjs reads the backslash forms (`\"U`) itself.
2. **Loaded on demand.** abcjs is a chunk of its own, loaded the first time a note with an `abc` block is shown. Until it is loaded the block shows as source. If it cannot be loaded, the block stays source with a note saying so.
3. **Warnings.** Any warning abcjs gives while parsing a block counts as an unsupported construct. The block then shows as its source, with the warnings under it: line, column and message. A click on a warning puts the cursor there. Nothing is engraved until the block parses cleanly.
4. **A click on the score** puts the cursor at the note, rest or bar line clicked, or at the start of the block, so the source shows where the user wants to edit it.
5. **Info-string options.** `transpose=<semitones>` moves the engraved score (abcjs's visual transposition); the file is unchanged. `nosource` keeps the score engraved in read-only views (a version in the history, a read-only vault), even with the cursor in the block, unless it has warnings.
6. **No view controls yet.** A transpose control and playback per block come with playback (F-17), so the controls are designed together.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Load abcjs on demand (chosen) | Notes without music start as fast as now | A short delay before the first score of a session shows |
| Bundle abcjs with the editor | The first score shows at once | Every start loads 500 kB more, music or not |
| Any warning falls back to source (chosen) | Follows rule 3: nothing is silently dropped; the user sees where | One typo hides the whole score until it is fixed |
| Engrave anyway and list the warnings | The score stays visible | abcjs ignores what it cannot read, so the score shows less than the source says |
| The author's line breaks (chosen) | The score looks as written, as abcjs and other ABC tools show it | Long lines are small on the iPhone |
| Line breaks fitted to the width | Readable on a phone | Unlike every other ABC tool; needs a render per width change |
| Entities decoded, filled with word joiners (chosen) | Shown as ABC 2.1 means them; clicks and warnings keep their place | abcjs reads a copy, not the block itself |
| Entities left to abcjs | Nothing to prepare | `&Uuml;` shows as written, without a warning |

## Consequences

- Positive: scores show in the editor on the Mac and the iPhone, and nothing in a block is hidden without a warning saying where.
- Negative: on a phone a long staff line is scaled down; a single error hides the score until it is fixed; the score is drawn from a prepared copy of the block, so a fix in abcjs for `+:` or entities needs a change here too.
- Follow-up work: playback and the view controls (F-17); scores in the exports, with the same reading of rule 3; line breaks fitted to the width, if the check on the iPhone asks for it.
