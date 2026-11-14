# ADR-0037: PDF on iOS, with the links put back

- Status: Proposed
- Date: 2026-11-14
- Deciders: Willi Kraml
- Related: F-06, ADR-0010, ADR-0021, ADR-0022, ADR-0024, ADR-0029, [spike](../spikes/ios-pdf.md)

## Context

Export on iOS is a must for 1.0 (F-06, roadmap M7), and PDF is one of its built-in formats. ADR-0022 left the iOS path open: iOS has no `NSPrintOperation`.

The spike (`docs/spikes/ios-pdf.md`) shows:

- A web view that is never shown, with its print formatter in a `UIPrintPageRenderer`, makes the same pages as macOS: one page per note, A4 or Letter, the margins, selectable text, the title, in about 0.1 s for 304 pages. The dates of ADR-0024 and the footer numbers of ADR-0029 are added as on macOS.
- **Every link is lost.** The formatter draws WebKit's own PDF pages into ours, and drawing a page keeps what is drawn, not its annotations. No public API gives the places of the links.
- Without links, the entries of a table of contents get no page numbers, as `pdf_pages` finds the page of an entry from its link (ADR-0029).
- `createPDF` keeps web links, but makes one long page, and on iOS its links inside the document point to objects missing from the file.
- A prototype found the text of each link on the printed pages, in document order, and placed 14 of 15 links of the samples and 600 of 600 on a 304-page stress page, in under 10 ms. The one miss was the footnote back link "↩", which iOS draws as an emoji picture.

## Decision

On iOS the PDF exporter prints through the web view's print formatter, then puts the links back where their texts are on the pages. macOS keeps the print path of ADR-0022, which keeps the links.

1. **Pages** (`schnipsle-apple`, `print_pdf` on iOS):
   - The exporter's page loads into a `WKWebView` with JavaScript off, never put into a window.
   - When it has loaded, its `viewPrintFormatter` goes to a subclass of `UIPrintPageRenderer` that returns the paper and the printable part from `paperRect` and `printableRect`.
   - The pages are drawn with `UIGraphicsBeginPDFContextToData`, which also sets the title.
   - It runs on the main thread behind the same `Printer` as on macOS.
2. **Anchors** (`schnipsle-export`): the print job carries, in document order, every link of the page with its target and the text it shows, and every target with the first words of the block it starts (up to four, not past the end of that block). The exporter knows them as it writes the HTML, so nothing is parsed back. Links without text, such as a picture alone, are left out.
3. **Finding them** (`schnipsle-apple`, PDFKit through `objc2-pdf-kit`):
   - Each link's text is looked for in the text of the pages, from where the previous link was found, so that the same text further on (300 "Back to the start") goes to its own place.
   - White space is ignored in comparing, as the page breaks lines where the HTML has a space, and within long words (`overflow-wrap: anywhere`).
   - A target is looked for from the previous link without moving on, as its text is often that of the following link.
   - The printer returns, with the PDF, one rectangle per line of each link found, and the page and height of each target found.
4. **Links** (`schnipsle-export`): the links are added with `lopdf` as an incremental update, after the dates and before the page numbers, so the table of contents gets its numbers as on macOS. A web link gets a URI action; a link inside the document gets a destination to its target's page and height. PDFKit does not write them, as it writes destinations without their page.
5. **Not found:** a link whose text or target is not found stays without a link, and the export's report counts these in one warning per file. The PDF is never refused for it.
6. **The footnote back link** is printed in its text form on iOS (`↩︎`, U+21A9 with U+FE0E), so it can be found. If the text form is drawn as a picture all the same, back links are left out of the anchors and do not count as missed.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Print formatter, links put back (chosen) | Pages as on macOS; links, table of contents and page numbers too | Finding by text can miss a link, or in rare cases put one on the wrong copy of a text; PDFKit and `lopdf` work in one more step |
| Print formatter, no links | Simplest; the contents could get its numbers from the note titles | Nothing clickable; a PDF from the iPhone is worse than one from the Mac |
| A web view per note | Page numbers for the contents from each formatter's start page | Still no links; many web views for a long export |
| `createPDF` | Keeps web links | One long page (ADR-0022); links inside the document broken on iOS |
| `createPDF` for the places of the links, the formatter for the pages | The places come from WebKit | Places on one long page do not map to the printed pages; the targets are missing |
| Our own PDF writer | Independent of WebKit | Layout, fonts and pictures to do again (ADR-0022) |

## Consequences

- Positive:
  - A PDF exported on the iPhone looks and works like one from the Mac: pages, links, a table of contents with page numbers, footer numbers, dates and title.
  - The `Printer` and the steps after it are shared; only the pages and the finding are iOS code.
- Negative:
  - Links are found by their text. A text the page draws differently from the HTML (an emoji, a ligature PDFKit does not read back) is missed and reported.
  - `schnipsle-apple` declares `WKWebView` on iOS itself, as `objc2-web-kit` 0.3 binds it on macOS only.
  - `objc2-pdf-kit` becomes an iOS dependency of `schnipsle-apple`, beside `objc2-ui-kit` and `objc2-web-kit` from the spike.
- Follow-up work:
  - The anchor list in the HTML exporter, and the link step with `lopdf`, with unit tests on a PDF without links.
  - Finding the anchors with PDFKit on iOS, and a test on a device with the samples and the stress page.
  - The footnote back link's text form, checked on the iPhone.
  - Export on iOS (roadmap M7): the dialog, the share sheet or Files, and PDF in `export_choices` on iOS.
  - ADR-0022's "iOS is decided when export comes there" is settled by this.
