# ADR-0029: Table of contents and page numbers

- Status: Accepted
- Date: 2026-11-06
- Deciders: Willi Kraml
- Related: F-06, F-13, ADR-0010, ADR-0021, ADR-0022, ADR-0024, ADR-0025

## Context

A check of the exports in LibreOffice, WordCraft and Books found no table of contents in any format: an EPUB has one only in the reader's navigation. No format but EPUB, whose reader counts pages itself, had page numbers. ADR-0021 left "a table of contents for one document when wanted" as follow-up work, and the PDF spike left page numbers out, as the print path adds no header or footer (`docs/spikes/pdf-print.md`).

How the formats can do it differs:

- Pandoc's table of contents in DOCX is an empty field. Word fills it in when it updates fields; LibreOffice shows nothing.
- ODT and DOCX have fields that show the page a bookmark is on. LibreOffice works them out when it opens the file, so the exporter need not know where the pages break.
- In PDF only the print path knows the pages. `WebKit` has no running footers in CSS, and headers and footers of the print path are private API. But a printed link inside the document points to the page of its target, so the page numbers can be found and drawn afterwards.

## Decision

1. **Choices.** The export dialog has "Start with a table of contents", for one document of several notes, and "Page numbers", for PDF, ODT and DOCX. A table of contents turns page numbers on, as its entries refer to them. Both choices are remembered like the others.
2. **Table of contents.** It is headed "Contents" and lists the title of every note in order, each linking to its note, on a page of its own before the first note. It lists the notes only, not the headings inside them.
   - HTML: a list of links at the start of the page.
   - PDF: that list, printed; then each entry gets the page its link goes to, at the right margin after a dotted line.
   - ODT: an entry has a link, a tab with dots and a page reference field to the note's bookmark, which the office program fills in.
   - DOCX: the same in Word's XML, with `PAGEREF` fields to bookmarks the exporter puts at the start of each note. The fields are marked to be updated: LibreOffice shows the numbers at once; Word updates them when it opens the file and may ask first.
   - EPUB: Pandoc's own (`--toc`), a page in the book besides the reader's navigation.
3. **Page numbers** are centred at the foot of every page, small and grey; the page of the table of contents is page 1.
   - ODT: a footer in the lower half of the bottom margin, so the text has as much room as without.
   - DOCX: a footer with a `PAGE` field, added to the package Pandoc wrote.
   - PDF: drawn on the printed pages in Helvetica, a standard font that needs no embedding. They are added with `lopdf` (MIT, without its default features) as an incremental update, so the bytes of the print path and the dates of ADR-0024 stay as they are. If that fails, the PDF stays without numbers and the report says so.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Table of contents as a choice (chosen) | Only where wanted | One more checkbox |
| Always for several notes | Nothing to choose | A page more where nobody needs it |
| Page numbers always with a table of contents, else a choice (chosen) | Entries always have their pages | One choice depends on another |
| PDF numbers drawn into the printed file (chosen) | One print; exact pages | Another font than the text; a new library |
| PDF printed twice, the second time with the numbers in the HTML | The text's font | Twice the time; still no footers |
| Private header and footer API of `WebKit` | Footers by the print path | Private API, not for the App Store |
| Pandoc's table of contents in DOCX | No code of ours | Empty in LibreOffice and other programs that do not update fields |

## Consequences

- Positive: a long export can be found in, on paper and on screen, in every format.
- Negative: `lopdf` brings 25 crates, among them ciphers for encrypted PDFs, which these exports are not. Word may ask to update fields when it opens a DOCX with a table of contents. The numbers in a PDF are in Helvetica, not in the font of the text.
- Follow-up work: printing on iOS (M6) needs the same numbers; a table of contents of the headings of a single long note, if wanted. ADR-0021 is amended by this, and the open point on page numbers in `docs/spikes/pdf-print.md` is settled.
