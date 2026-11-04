# ADR-0024: PDF dates are corrected in the file

- Status: Accepted
- Date: 2026-11-04
- Deciders: Willi Kraml
- Related: F-06, ADR-0021, ADR-0022, [spike](../spikes/pdf-print.md)

## Context

ADR-0021 rule 1 puts a note's `created` and `modified` into the PDF's metadata, and ADR-0022 sets them afterwards with PDFKit. That works on macOS 27 but not on macOS 26.6: PDFKit holds the dates set on the document and then writes the current time for both, through `dataRepresentation` and `writeToURL` alike. Nothing in the API avoids this.

## Decision

This replaces the PDFKit part of ADR-0022; the print path stands. The title comes from the print job. The exporter corrects the two dates in the bytes of the finished PDF:

- only in the Info dictionary the file's trailer names;
- only with a value of the same length, `D:YYYYMMDDHHmmSS+00'00'` in UTC, so no offset in the file changes;
- a date in any other form stays as printed, and the PDF stays intact.

A PDF of several notes has no dates of its own (ADR-0021) and keeps the time of printing, on every macOS version.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Correct the bytes (chosen) | Same result on every macOS version; no PDFKit in the app; small, tested code without `unsafe` | Depends on how Quartz writes the Info dictionary, which a test on CI watches |
| Keep PDFKit, and accept the time of export on macOS 26 | No new code | The note's dates are lost on macOS 26; results differ by version |
| A PDF library such as `lopdf` | A real PDF edit, whatever the form | A large dependency, and rewrites the whole file for two dates |
| Redraw the pages into a new PDF context with the dates | Only Apple APIs | Loses the links; a second rendering step |

## Consequences

- Positive: a single note's PDF carries its dates on macOS 26 and 27; `objc2-pdf-kit` is only needed by a test.
- Negative: if a later macOS writes dates in another form, they stay the time of printing until the code follows. `crates/apple/tests/pdf.rs` fails on CI when that happens, once CI runs that version.
- Follow-up work: none.
