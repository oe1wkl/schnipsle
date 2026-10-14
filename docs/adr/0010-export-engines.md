# ADR-0010: Export engines: Pandoc on desktop, built-in on iOS

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: F-06, F-13, ADR-0002, ADR-0005

## Context

Export to HTML, PDF and ODT is a must on all platforms. Pandoc is the most capable converter but is GPL-licensed and cannot run on iOS, where apps may not start other programs.

## Decision

- One Rust trait `Exporter` with format, options (template, page size, single file vs folder) and input (one note, selection, virtual folder).
- **Built-in exporters (all platforms):**
  - HTML: comrak HTML + CSS theme; images inlined as data URIs for single-file export.
  - PDF: the HTML rendered and printed by the platform WebView (`WKWebView.createPDF` on Apple platforms).
  - ODT: written directly as zipped OpenDocument XML from the Markdown AST (a focused writer covering headings, paragraphs, emphasis, lists, tables, code, images, links).
- **Pandoc exporter (desktop only, optional):** used when a Pandoc executable is found; enables DOCX, EPUB, Typst-based PDF and reference-document styling. Never bundled into the app binary.
- The UI offers only formats available on the current platform.

## Consequences

- iOS gets HTML, PDF and ODT without Pandoc; desktop gets the same plus extra formats.
- Writing an ODT writer is real work but keeps us independent and licence-clean; golden files validated with LibreOffice in CI (`soffice --headless --convert-to pdf` must succeed).
- Licence check needed before shipping a desktop build that downloads or bundles Pandoc separately (ADR-0005).
