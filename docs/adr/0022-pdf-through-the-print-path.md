# ADR-0022: PDF through the print path on macOS

- Status: Proposed
- Date: 2026-11-02
- Deciders: Willi Kraml
- Related: F-06, ADR-0010, ADR-0021, [spike](../spikes/pdf-print.md)

## Context

ADR-0010 decides that a PDF is the exported HTML rendered and printed by the platform WebView, and names `WKWebView.createPDF` on Apple platforms. ADR-0021 asks for a new page per note, and the export dialog offers a paper size. The spike shows that `createPDF` makes one long page with neither.

## Decision

On macOS the PDF exporter prints with `WKWebView.printOperationWithPrintInfo:`, run by `runOperationModalForWindow:…` with no panels, in a window that is never shown, saving to a file (`NSPrintSaveJob`). This replaces the mention of `createPDF` in ADR-0010 for macOS; the rest of ADR-0010 stands.

- `runOperation` is not used: in the spike it did not finish and wrote a file of hundreds of megabytes.
- The print operation's success is not trusted: the exporter checks that the file exists.
- Title and dates (ADR-0021 rule 1) are set afterwards with PDFKit, through `objc2-pdf-kit`.
- iOS is decided when export comes there (M6).

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Print path (chosen) | Real pages, paper size, margins, CSS page breaks; links and text stay; fast | Asynchronous on the main thread; needs an unshown window; macOS only |
| `createPDF` | Simple, on macOS and iOS | One long page, so ADR-0021 rule 3 cannot be met |
| `createPDF`, cut into pages with a rectangle per page | Works on both platforms | Cuts through lines of text and pictures; ignores CSS page breaks |
| Own PDF writer, or a PDF library | Independent of WebKit | Layout, fonts and pictures to do again; looks different from the HTML export |
| Pandoc | Good typesetting | Desktop only and optional (ADR-0010), so no must-have format can rest on it |

## Consequences

- Positive: PDFs have the pages ADR-0021 asks for, with the same look as the HTML export.
- Negative: one more framework binding (`objc2-pdf-kit`) in `schnipsle-apple`; a separate path on iOS later.
- Follow-up work: PDF exporter (M5 package 4); iOS path (M6); check under a sandbox if a Mac App Store build comes.
