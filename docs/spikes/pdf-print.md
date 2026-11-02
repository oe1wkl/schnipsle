# Spike: PDF from an exported HTML page

- Branch: `spike/pdf-print`
- Started: 2026-11-02
- Decides: how the PDF exporter (M5 package 4) makes pages on macOS
- Status: **concluded 2026-11-02: use the print path, not `createPDF`** (decided in [ADR-0022](../adr/0022-pdf-through-the-print-path.md)). Tested in a small program, not yet inside the Tauri app; iOS not tested

## Question

ADR-0010 names `WKWebView.createPDF`, which was expected to make one long page. ADR-0021 asks for a new page per note, and the export dialog for A4 or Letter and margins. Can the WebKit print path (`NSPrintOperation` to a file) do this without showing anything?

## What was built

`crates/apple/examples/pdf_spike.rs` loads an HTML file into a web view that is never shown, with JavaScript off, and writes a PDF by one of three methods. Input: the golden file `tests/fixtures/export/html/expected/one-document/all.html` (six notes) and generated stress pages. Results were read with PDFKit and looked at as page images.

```sh
cargo run -p schnipsle-apple --example pdf_spike -- in.html out.pdf --method modal --paper a4 --margin 20
```

Tested on macOS 27.0.1 with Xcode 27.2, in a command-line process with its own `NSApplication` and run loop.

## Results

| Method                          | Result                                                                                                     |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| `createPDF`                     | One page of 464 × 2807 pt, the width of the web view. CSS page breaks do nothing; no paper size or margins |
| `NSPrintOperation.runOperation` | **Unusable.** Did not finish; the output file had grown to 738 MB when the process was stopped after 45 s  |
| `runOperationModalForWindow:…`  | **Works.** Six pages, one per note, on A4 or Letter, finished in about 0.3 s. No panel or window appears   |

What the modal print path gives:

| Checked                               | Result                                                                  |
| ------------------------------------- | ----------------------------------------------------------------------- |
| Paper size                            | A4 595 × 841 pt, Letter 612 × 792 pt                                    |
| Margins                               | 10, 20 and 25 mm as set in `NSPrintInfo`, measured on the rendered page |
| `break-before: page` from `theme.css` | Each note starts a page, no empty first page                            |
| Text                                  | Selectable; the system font is embedded                                 |
| Web and `mailto:` links               | Stay links                                                              |
| Links between notes, footnote links   | Stay links to the right page                                            |
| Pictures as `data:` URLs              | Shown; a 36 MB PNG took 1.7 s and gave a 36 MB PDF                      |
| 1000 short notes                      | 1000 pages in 0.45 s, 650 KB                                            |
| Long table, long code block           | Break across pages                                                      |
| `setJobTitle`                         | Becomes the PDF's Title                                                 |
| JavaScript off                        | Printing still works                                                    |

Not needed, tested by leaving them out: setting the frame of the print view, and putting the web view into the window. The window only has to exist, as the method asks for one. Horizontal pagination `Fit` and `Automatic` gave the same number of pages.

## Pitfalls found

1. **Success does not mean a file.** With a target in a folder that does not exist the operation reports success and writes nothing. The exporter must check that the file is there.
2. **Wide content shrinks and is cut off.** A table of 14 columns or a long unbroken word scaled the whole document down, and what stuck out was still cut off at the page edge. These print rules fixed both, with normal text size and nothing cut off (the stress page went from 15 to 31 pages):

   ```css
   @media print {
     p,
     li,
     td,
     th,
     h1,
     h2,
     h3,
     blockquote {
       overflow-wrap: anywhere;
     }
     table {
       max-width: 100%;
     }
   }
   ```

3. **Dates are not the note's.** The PDF's CreationDate and ModDate are the time of printing, while ADR-0021 rule 1 wants the note's. In a Swift test, PDFKit (`documentAttributes`, then `write(to:)`) set them and a title with umlauts, and kept the pages, the text and both kinds of link. The file grew by about 80 bytes. The Rust crate `objc2-pdf-kit` (Zlib OR Apache-2.0 OR MIT) has the bindings; they are not tried yet.
4. **The call is asynchronous and on the main thread.** Loading and printing both finish through callbacks while the run loop runs. In the app the export command must start this on the main thread and wait for the result without blocking it (CLAUDE.md, pitfalls).

## Recommendation for package 4

- A safe function in `schnipsle-apple`: HTML, paper size, margins, title and dates in; a PDF at a target path out, reported through a callback.
- Steps: a hidden web view with JavaScript off; load; on `didFinishNavigation` print with the modal method and an unshown borderless window; write to a temporary file next to the target; check that it exists; set title and dates with PDFKit; rename it over the target (format.md §10 style).
- Add the print rules above to `crates/export/src/theme.css`.
- The exporter's `Rules` already has `web_pictures: false` for PDF, so the page loads nothing from the network.

## Open

- **Inside the app:** nothing here ran next to Tauri's windows and event loop. Package 4 must check that printing does not disturb the open window or take the focus.
- **Sandbox:** the app is not sandboxed today. A Mac App Store build may need the print entitlement (`com.apple.security.print`) and access to the target; to be tested when sandboxing comes.
- **iOS (M6):** `NSPrintOperation` does not exist there, and `createPDF` should have the same one-page limit. The likely path is `UIPrintPageRenderer` with the web view's print formatter. Not tested.
- **Page numbers:** the print path adds no headers or footers. Not asked for by ADR-0021, so left out.
