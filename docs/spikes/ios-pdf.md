# Spike: PDF on iOS

- Branch: `spike/ios-pdf`
- Started: 2026-11-14
- Decides: how the PDF exporter makes pages on the iPhone and iPad (M7, export on iOS; the iOS path of ADR-0022)
- Status: **concluded 2026-11-14: the print formatter for the pages, the links put back from the exporter's HTML** (way 2 below, chosen by the owner). ADR to follow

## Question

iOS has no `NSPrintOperation`, which makes the pages on macOS (`docs/spikes/pdf-print.md`, ADR-0022). Can a web view that is never shown give the same PDF on iOS through `UIPrintPageRenderer` and its print formatter: one page per note, A4 or Letter, margins, selectable text, links, and the page numbers and table of contents of ADR-0029?

## What was built

- `crates/apple/src/pdf_ios.rs`: `print_pdf` with the macOS signature (`PdfPage` in, PDF bytes out through a callback on the main thread).
  - It loads the page into a `WKWebView` with JavaScript off. The web view is never put into a window.
  - When the page has loaded, it adds the web view's `viewPrintFormatter` to a `UIPrintPageRenderer` subclass, whose `paperRect` and `printableRect` give the paper and the margins.
  - It then draws each page into a PDF context. Three ways to draw are tried (`Method`):
    - `UIGraphicsBeginPDFContextToData`;
    - `UIGraphicsPDFRenderer`;
    - `WKWebView.createPDF` of the whole page, for comparison.
  - `objc2-web-kit` binds `WKWebView` on macOS only, so the module declares the four methods it needs itself, as wry does.
- `src-tauri/src/spike_pdf.rs`: runs on an iPhone when the app is started with `SCHNIPSLE_SPIKE_PDF` set (`xcrun devicectl device process launch --environment-variables`).
  - It copies the invented export samples (`tests/fixtures/export/vault`) into a temporary vault.
  - It prints the golden page `pdf/expected/one-document/all.html` and a stress page, and exports the samples through `PdfExporter` as the dialog would.
  - It writes everything to `Documents/spike-pdf/` in the app's container, which `xcrun devicectl device copy from --domain-type appDataContainer` copies to the Mac.
- The export command now prints on iOS too, through the same `MainThreadPrinter` as on macOS.

Tested on an iPhone 18 Pro with iOS 27, release build; the PDFs were read with PDFKit on the Mac and looked at as page images.

## Results

| Checked              | Print formatter (`UIGraphicsBeginPDFContextToData` or `UIGraphicsPDFRenderer`)                                             | `createPDF`                                                                                             |
| -------------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------- |
| Pages                | One per note, as on macOS (6, or 7 with a table of contents)                                                               | One page of 482 × 2782 pt, the width of the web view                                                    |
| Paper                | A4 595 × 842 pt, Letter 612 × 792 pt                                                                                       | –                                                                                                       |
| Margins              | 20 mm and 10 mm as given, measured on the page images                                                                      | –                                                                                                       |
| `break-before: page` | Each note starts a page, no empty first page                                                                               | Ignored                                                                                                 |
| Text                 | Selectable, the system font embedded                                                                                       | Selectable                                                                                              |
| Links                | **None.** No link annotation at all, neither web nor inside the document                                                   | Web and `mailto:` links kept, with their places; links inside the page without a target PDFKit can read |
| Title                | From the PDF context's information (`UIGraphicsBeginPDFContextToData`); none from `UIGraphicsPDFRenderer` without a format | None                                                                                                    |
| Time                 | 0.05–0.1 s for the samples; 304 pages in 0.1 s                                                                             | 0.04 s                                                                                                  |

With the exporter (`PdfExporter`, the same code as on macOS):

- The dates of ADR-0024 are set as on macOS.
- The footer numbers of ADR-0029 are drawn.
- A table of contents is printed, but **its entries get no page numbers**, as `pdf_pages` takes them from the links, and there are none.
- A PDF per note, six files: 0.27 s.

The stress page behaves as on macOS: a table of 14 columns breaks its cells within words and stays on the page; a long word wraps; a long code block runs over three pages.

Not needed, tested by leaving them out: a window for the web view, and showing it.

## Why the links are lost

The print formatter draws each page from a PDF that WebKit makes in its own process and keeps to itself. Drawing a page of a PDF into another one keeps what is drawn, not the annotations, so the links of that inner PDF do not reach ours. `createPDF` hands over WebKit's PDF as it is, links included, but as one long page. No public API gives the inner PDF of the print path or the places of its links.

## Ways to get links back

1. **No links on iOS.** The table of contents gets its numbers from the note titles, as every note starts a page. Nothing in the PDF is clickable.
2. **Links put back (chosen).** Find the text of every link on the printed pages and add the link there.
3. **A web view per note.** Each note gets a print formatter of its own; its `startPage` numbers the table of contents. Links stay lost, and a long export needs many web views.

`createPDF` looked like the source for way 2, as it keeps the links with their places. On iOS, however, it writes internal links with destinations that point to objects missing from the file, so it tells where a link is but not where it goes. The exporter's own HTML is the better source: it holds every link with its text and target, in the order the pages show them.

## Links put back: the prototype

`ios-pdf-relink.swift`, a prototype on the Mac next to these notes, reads a PDF the iPhone printed and the HTML it was printed from:

1. From the HTML, in document order: every link with its text and address, and every element with an `id` with the text of its first block (a heading, a paragraph), up to four words.
2. With PDFKit, each text is looked for in the text of the pages, from where the previous link was found. Words may be apart by any white space, as the PDF has a line break where the HTML has a space. A target is looked for from the previous link without moving on, as its text is often that of a following link.
3. A link gets one rectangle per line (`selectionsByLine`), with its web address, or the page and height of its target.

| PDF                                                        | Links in the HTML | Put back | Missed                       | Time |
| ---------------------------------------------------------- | ----------------- | -------- | ---------------------------- | ---- |
| The samples, with a table of contents (7 pages)            | 15                | 14       | the footnote's back link "↩" | 3 ms |
| Stress page (304 pages), 300 identical "Back to the start" | 600               | 600      | –                            | 8 ms |

Every entry of the table of contents went to the page of its note, the footnote reference to its footnote, the links between notes to their notes, and each "Back to the start" was found in its own section.

What it showed besides:

- **PDFKit writes internal links wrongly.** `PDFAnnotation` with a `PDFDestination`, written with `write(to:)`, became `/Dest [ /#23232 /XYZ … ]`: a name where the page belongs. The links are therefore best added with `lopdf` as an incremental update, as the page numbers of ADR-0029 already are. PDFKit only finds the text.
- **A target's text must end at its block.** Four words across blocks ("Antenna tuner The finished") were found in another note; matching word by word anywhere had the same fault.
- **"↩" is drawn as a picture** (emoji), not as text, so it cannot be found. The exporter can ask for its text form (`↩︎`, with U+FE0E), or the back link can stay without a link.
- One link per line: the rule of `pdf_pages` for an entry of the table of contents (lowest, then furthest right) finds the end of its last line without the extra end link of one space, which iOS cannot find, having no text.

## Recommendation for the ADR

- **iOS printer** (`schnipsle-apple`): the print formatter through `UIPrintPageRenderer` into `UIGraphicsBeginPDFContextToData`, as built; then PDFKit (`objc2-pdf-kit`, which binds iOS) finds the anchors.
- **Anchors** (`schnipsle-export`): the exporter lists the links and targets of the page it prints, in order, with the text the page shows, and passes them in the print job. It knows them as it writes the HTML, so nothing is parsed back.
- **Links** (`schnipsle-export`): the found places come back with the PDF, and links are added with `lopdf` before the dates and page numbers. A link not found is left out and counted in the report's warnings.
- **macOS** stays as it is: its print path keeps the links.

## Pitfalls found

1. **`objc2-web-kit` 0.3 has no `WKWebView` on iOS.** The class and the methods used are declared in `pdf_ios.rs`.
2. **`paperRect` and `printableRect` are read-only.** A subclass of `UIPrintPageRenderer` returns them. Setting them by key-value coding, as often shown, works too, but relies on private storage.
3. **`UIGraphicsPDFRendererFormat::defaultFormat` is deprecated** in the bindings; `new` gives the same.
4. **A locked iPhone** refuses `devicectl device process launch`.
5. **`createPDF` on iOS** writes internal links whose destinations are missing from the file.
