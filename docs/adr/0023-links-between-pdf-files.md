# ADR-0023: Links between separate PDF files become text

- Status: Proposed
- Date: 2026-11-03
- Deciders: Willi Kraml
- Related: F-06, ADR-0021, ADR-0022, [spike](../spikes/pdf-print.md)

## Context

ADR-0021 rule 7 says a link to a note exported too points to that note: an anchor in one document, the other file in a folder. For a PDF per note the print path cannot do the second. WebKit drops a relative link when the page has no base URL. With a base URL it prints an absolute link, and rewriting that with PDFKit's `PDFActionRemoteGoTo` and a relative file wrote the action without the file.

## Decision

Rule 7 changes for PDF only: when notes are exported as a PDF per note, a link to a note in another of these PDFs keeps its text without the link, and the report says so (`noteInOtherFile`). Links within one PDF, links to the web and `mailto:` stay links. HTML keeps links between its files, and ODT is decided with its writer.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Text without a link, reported (chosen) | Honest and simple; no dead links; works in every viewer | The link is lost |
| Absolute `file:` links to the target folder | Work in viewers right after the export | Break when the folder moves or is sent on; put the user's name and folders into the file |
| Relative `URI` links through PDFKit | Stay inside the folder | Not tried; relative URIs depend on the viewer, so a click may do nothing |
| Writing `GoToR` into the PDF without PDFKit | The form the PDF format has for this | Needs a PDF editing library for one kind of link |

## Consequences

- Positive: a folder of PDFs can be moved and sent on, and has no dead links or private paths.
- Negative: the PDFs of a folder do not link to each other; one document with a section per note keeps its links, and is the choice for that.
- Follow-up work: the report text in the export UI (M5 package 6).
