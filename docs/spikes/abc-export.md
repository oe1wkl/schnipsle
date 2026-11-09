# Spike: engraved ABC in exports

- Branch: `spike/abc-export`
- Started: 2026-11-09
- Decides: how the exporters, written in Rust, get the scores abcjs engraves in the web view (M6, "To decide first")
- Status: **concluded 2026-11-09: the app's web view engraves before the export and hands the SVG to it** (decided in [ADR-0033](../adr/0033-scores-in-exports.md)). Tested on macOS with small programs and in the editor playground; not in the Tauri app, not on iOS, not in Word

## Question

ADR-0013 says an `abc` block is "rendered to SVG once in the export pipeline and embedded as an image", and ADR-0021 rule 8 exports it as source until the music milestone. abcjs runs only in a web view; the exporters run in Rust. Where does the engraving happen, and how does each format take the result?

## What was built

- `crates/apple/examples/engrave_spike.rs` loads `abcjs-basic-min.js` into a `WKWebView` that is never shown, with JavaScript on, engraves ABC files through `evaluateJavaScript` and writes one SVG per tune.
- `crates/apple/examples/export_spike.rs` exports notes of a vault as one document in HTML, PDF, ODT and, through Pandoc, DOCX and EPUB, with the exporters of M5.
- In the editor playground (`pnpm exec vite`, `ui/src/dev/editor-demo.ts`), the page's own abcjs engraved the fixture `tests/fixtures/format/5.6-index/score.abc`, prepared by `forAbcjs` of `ui/src/score.ts`, into an element off screen.

```sh
cargo run -p schnipsle-apple --example engrave_spike -- out-dir a.abc b.abc
cargo run -p schnipsle-apple --example export_spike -- vault out-dir note.md
```

Input: the score fixture (two voices, lyrics, chords, `+:`, `&Uuml;`), the kanon of the export sample vault and a block of two tunes, put into an invented vault as SVG pictures. Results were looked at as page images (`sips`), the PDF text read with PDFKit, ODT and DOCX turned into PDF by LibreOffice 25, the EPUB checked with EPUBCheck. macOS 27.0.1, abcjs 6.7.2, Pandoc 3.12.1.

## Results

Engraving:

| Where                           | Result                                                                                                                                                   |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Hidden `WKWebView`, Rust only   | **Works.** abcjs loaded in 0.24 s; four tunes engraved in 40–90 ms; 110 kB of JSON back through `evaluateJavaScript`                                     |
| The app's web view (playground) | **Works.** The score in 54 ms into an element at `left: -10000px; visibility: hidden`; it must be in the document, as abcjs measures text with `getBBox` |
| Source as written               | abcjs warns on the `+:` line and draws a stray staff, and `&Uuml;` stays as written: the export needs the same preparation as the editor (ADR-0032)      |
| Source prepared by `forAbcjs`   | No warnings; the score as in the editor                                                                                                                  |

What abcjs gives with `foregroundColor: "#000000"` and `staffwidth: 600`:

| Checked      | Result                                                                                                               |
| ------------ | -------------------------------------------------------------------------------------------------------------------- |
| Size         | `width="600"` and the height in px, no `viewBox`; 600 px is 15.9 cm, inside the text width of A4 and Letter          |
| Colour       | Lines and notes `#000000`; the root has `fill` and `stroke` `currentColor`, which text inherits                      |
| Text         | Titles, chords and lyrics stay `<text>`, in Times New Roman and Helvetica                                            |
| Extras       | An abcjs `<style>` rule for dragging, a `<title>` in the XHTML namespace, `role="img"` and an `aria-label`; no `id`s |
| Word joiners | The U+2060 that `forAbcjs` puts in to keep offsets end up in the text (`Ü⁠⁠⁠⁠⁠ber`)                                  |
| Sizes        | 10–14 kB for a line of music, 63–72 kB for the two-voice score                                                       |

The SVGs in each format (as pictures of a note, through the exporters of M5):

| Format                   | Result                                                                                                                                                                                                                               |
| ------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| HTML                     | `data:` URL in an `<img>`; shown. Text inside is not found by the browser's search                                                                                                                                                   |
| PDF (print path)         | Shown as vectors; title, chords and lyrics are text in the PDF                                                                                                                                                                       |
| HTML and PDF, SVG inline | Shown, text in the PDF as above. With a `viewBox` added and `max-width: 100%; height: auto` it scales. Titles, voice names and `W:` lines took the page's text colour (red in the test), while notes, chords and lyrics stayed black |
| ODT                      | Shown in LibreOffice, sized from `width` and `height`                                                                                                                                                                                |
| DOCX (Pandoc)            | Shown in LibreOffice. SVG only: the exporter warns for each picture, as Word before 2016 shows nothing without a PNG copy (`rsvg-convert` adds one)                                                                                  |
| EPUB (Pandoc)            | SVG files in `EPUB/media/`; EPUBCheck 5: no errors or warnings                                                                                                                                                                       |

LibreOffice draws text with word joiners correctly. abcjs's own layout shows as in the editor: lyrics in bold and some space above the tempo.

## Pitfalls found

1. **The source must be prepared** as for the editor (`+:`, entities, escapes, `\&`), or exports and editor differ.
2. **Word joiners leak into the text** of the SVG. Harmless in LibreOffice and WebKit, but they are characters in copied or searched text, so the export removes them.
3. **The engraving element must be in the document,** off screen and hidden, not `display: none`.
4. **The SVG should stand on its own:** inline, the text without a colour of its own takes the page's (see the table), so the root's `currentColor` becomes black. abcjs's `<style>` and XHTML `<title>` go, and a `viewBox` is added so the score can shrink to a narrower page.

## Recommendation

- The app's web view engraves, before the export, with the editor's code: no second copy of abcjs and the preparation in Rust, and the same on every platform Tauri runs on. The hidden `WKWebView` works too, but only on Apple platforms, and would need abcjs bundled for Rust.
- Rust lists the score blocks of the notes to export; the UI engraves them and passes the SVGs with the export command; the exporters look them up and fall back to the source, with a report entry, for a block without one.
- HTML and PDF take the SVG inline, so lyrics are text a browser finds; ODT, DOCX and EPUB as a picture file.
