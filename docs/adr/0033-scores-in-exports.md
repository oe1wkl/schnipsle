# ADR-0033: Scores in exports, engraved by the app's web view

- Status: Accepted
- Date: 2026-11-09
- Deciders: Willi Kraml
- Related: F-06, F-16, ADR-0010, ADR-0013, ADR-0021, ADR-0022, ADR-0025, ADR-0032, [format.md §5.6](../format.md#56-music-notation), [spike](../spikes/abc-export.md)

## Context

ADR-0013 says an `abc` block is rendered to SVG once in the export pipeline and embedded as a picture; ADR-0021 rule 8 exports it as its source until the music milestone. The exporters are written in Rust, but abcjs, the only engraver allowed (ADR-0013), runs only in a web view. The editor already engraves there, and it prepares the source first, since abcjs does not read `+:` continuations, entities and escapes (ADR-0032). An export must look like the editor and work on every platform that gets exports, iOS and Linux later included.

The spike engraved scores both in a hidden `WKWebView` driven from Rust and in the app's own web view, and put the SVGs into every export format: all formats show them, and the PDF keeps titles, chords and lyrics as text.

## Decision

1. **The app's web view engraves, before the export.** The export command does not wait for the UI. Instead:
   - `export_scores` lists the score blocks of the notes to export: for each distinct block text and `transpose=`, a key and the source.
   - The UI engraves each with the editor's code, the preparation of ADR-0032 included, into an element off screen. Settings: black, staff width 600 px (15.9 cm, inside the text width of A4 and Letter), one SVG per tune, in steps so the window keeps responding.
   - `export_notes` gets the SVGs by key.
2. **A block without a score stays source.** A block keeps its source as a code block, as ADR-0021 rule 8 has it until now, and the report names it (`scoreNotEngraved`), when it:
   - has abcjs warnings,
   - could not be engraved (abcjs not loaded), or
   - has no SVG because its note changed between the two commands.
3. **The SVG is cleaned in Rust** before it goes into a document. Only drawing elements and their attributes are kept (no scripts, styles, links or event attributes). The root's `currentColor` becomes black, and a `viewBox` is added. The word joiners of the preparation are removed from the text. The `aria-label` stays.
4. **Per format:**
   - **HTML and PDF:** inline `<svg>` in a `<figure class="score">`, scaled down to a narrower page. Lyrics stay text a browser finds, and text in the PDF.
   - **ODT:** a picture in `Pictures/`, sized from the SVG.
   - **DOCX and EPUB:** the SVG goes to Pandoc as a picture file, in place of the block (ADR-0013, ADR-0025). DOCX keeps the warning for SVG without a PNG copy.
5. **Options of the block.** `transpose=` applies. `nosource` changes nothing, as an export shows the score anyway. The option of ADR-0013 to add the ABC source to an export is left to the work on music in exports.

This amends rule 8 of ADR-0021 for `abc` blocks, and replaces "in the export pipeline" in ADR-0013 by "in the app's web view, before the export". `chordpro` blocks are rendered by the core (ADR-0030) and are not part of this decision.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| The UI engraves first and passes the SVGs with the export (chosen) | The editor's code and preparation, so exports look like the editor; every platform with a web view; the exporters stay testable with SVG files | Export needs the window; two commands; a note changed in between loses its score in that export |
| Rust asks the UI during the export (event, then a reply command) | One command from the dialog | A Rust thread waits on JavaScript, with time-outs; harder to test; nothing gained over engraving first |
| A hidden `WKWebView` driven from Rust (tried: 0.24 s to load, 40–90 ms per batch) | Independent of the window | Apple only; abcjs and the preparation bundled a second time for Rust; Linux and Android need their own |
| abcjs in a JavaScript engine in Rust (Boa, QuickJS) | No web view | abcjs needs a DOM and measures text with `getBBox`; a DOM to build or fake |
| abcjs as a script in the exported HTML | Nothing to engrave in advance | Scripts are left out of exports (ADR-0021 rule 9) and the PDF prints with JavaScript off (ADR-0022); ODT and DOCX get nothing |
| SVG as an `<img>` in HTML too | One way for all formats | The browser does not find the lyrics |

## Consequences

- Positive: scores show in every export format as in the editor. The PDF keeps the words of a song as text. One engraver and one preparation serve the editor and exports, and iOS gets the same path when export comes there.
- Negative:
  - The export dialog has a step before writing, which takes about 50 ms per score.
  - SVG fonts (Times New Roman, Helvetica) are measured in the web view and substituted where a reader lacks them.
  - DOCX shows the scores in Word 2016 and later only, unless `rsvg-convert` adds a PNG copy.
- Follow-up work (M6 "Music in exports"):
  - `export_scores`, the engraving step in the export dialog, `scoreNotEngraved`, and the cleaning and placing in the exporters.
  - Golden files with SVGs made by abcjs 6.7.2.
  - The ABC source option.
