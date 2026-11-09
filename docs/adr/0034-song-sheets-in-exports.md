# ADR-0034: Song sheets and ABC sources in exports

- Status: Accepted
- Date: 2026-11-09
- Deciders: Willi Kraml
- Related: F-06, F-16, F-18, ADR-0013, ADR-0021, ADR-0025, ADR-0030, ADR-0031, ADR-0033, [format.md §5.7](../format.md#57-lyrics-with-chords)

## Context

ADR-0030 renders a `chordpro` block into a sheet in `schnipsle-core` "for the editor and the exports", and ADR-0021 rule 8 exports it as its source until the music milestone. An export shows a note in a document that has no editor, so the sheet must be written in each format, with each chord over the start of its text, as in the editor (ADR-0031). ADR-0013 also offers to add the ABC source of a score to an export, which ADR-0033 left to this work.

HTML, and so PDF and EPUB, can lay out a chord over its text as the editor does. Word processors cannot do it the same way. Tried in LibreOffice 25 with ODT and DOCX:

- **Ruby annotations** (chords as ruby over the lyrics) ran wide chords into each other (`Am7Cmaj7(#11)D`). LibreOffice dropped the spaces at the end of a ruby base in DOCX and ignored the chord style in ODT.
- **A borderless table per line**, a column per chord, spread across the width of the page.
- **A line of chords over a line of lyrics in a monospace font**, the usual layout of printed chord sheets, kept every chord over its syllable.

## Decision

1. **Sheets as the file says.** A `chordpro` block is rendered with `chordpro::sheet::render`, as written: its `transpose=`, `notation=` and `nochords`, otherwise the vault's naming. No view settings of the editor apply. `nosource` changes nothing, as an export shows the sheet anyway.
2. **HTML, PDF and EPUB** show the sheet as the editor does. Each chord is over the start of its text, and a chorus is indented with a bar. The style is `song.css`, added to a page only if it holds a sheet, and to every EPUB.
3. **ODT and DOCX** write each line of lyrics as two lines: the chords, bold and blue, over the lyrics, both in Courier New. Courier New is on every Mac and Windows computer, and LibreOffice replaces it by a font of the same widths.
   - Each chord starts over the start of its text, and the lyrics get spaces where a chord is wider than the text under it.
   - A line of chords stays on the page of its lyrics.
   - Title, subtitle and details are in the normal font, as in the editor. A chorus is indented with a bar.
   - ODT uses paragraph styles named `Song …`, which users can change. DOCX formats the paragraphs directly, as raw Word XML for Pandoc.
4. **How it gets into the documents.** The sheet replaces the block in the tree as a raw node already written for the format: HTML, ODT XML, or a raw `openxml` or `html` block for Pandoc. No note can make such a node, as comrak never parses one.
5. **ABC source.** The export dialog offers "Add the ABC source under each score" (`abcSource`). With it, the block's source follows its engraved score as a code block. Without it, only the score is exported.

This amends rule 8 of ADR-0021 for `chordpro` blocks, and ADR-0030's consequence that sheets look the same "in the editor and every export": ODT and DOCX look like a printed chord sheet instead.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Chords over lyrics in a monospace font in ODT and DOCX (chosen) | Every chord stays over its syllable in any word processor; the usual layout of chord sheets | Not the editor's font; a line wider than the page (about 80 characters on A4) breaks its two lines apart |
| Ruby annotations | Proportional font; one line of text per line | Tried: wide chords overlap, spaces lost, chord style ignored |
| A table per line | Proportional font, chords exactly over their text | Tried: spreads across the page; text in cells does not copy as lines |
| Chords inline, raised before their syllable | Robust and compact | The chords are not over their syllables, as the roadmap asks |
| The monospace layout in every format | All exports alike | HTML, PDF and EPUB can do better, and look like the editor |
| The ABC source under its score, as an option (chosen) | Simple; the source stays where the score is | Long sources take room in the document |
| The ABC source in an appendix or a file next to the export | The document stays short | An appendix per document or note to design; a single file has nowhere to put a second file |

## Consequences

- Positive:
  - Song sheets are readable in every export format, transposed and named as the file says.
  - ODT and DOCX open the same in Word, LibreOffice and Pages.
  - Scores can carry their source for those who play from ABC.
- Negative:
  - ODT and DOCX do not look like the editor.
  - Very long lyric lines break their chord line apart there.
  - The view settings of the editor (transposition, naming, lyrics only) are not exported.
- Follow-up work:
  - Exporting with the editor's view settings, if wanted.
  - A check of the DOCX in Word when Word is at hand.
