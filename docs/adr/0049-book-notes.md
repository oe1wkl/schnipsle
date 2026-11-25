# ADR-0049: Book notes: a title page and an order for exported notes

- Status: Accepted
- Date: 2026-11-25
- Deciders: Willi Kraml
- Related: F-06, F-12, ADR-0021, ADR-0025, ADR-0029, ADR-0037, ADR-0048

## Context

An export of several notes takes them in the order of the list they come from (ADR-0021 rule 3), and a list sorts by date or by title. ADR-0029 adds a table of contents and page numbers. There is no way to put notes in an order of one's own, and no title page.

The owner asked on 2026-11-25 for the manual to be exported and printed as one document (ADR-0048), and for three things it needs, which are useful for other notes too: a title page, a table of contents (there), and an order of the notes.

The order has to be stored in the vault, so that it syncs, ships with the manual and is there at the next export. Notes already link to each other (format.md §5.4: `[[Title]]`, `[[Title|label]]` and relative `.md` links), and the exporter already resolves those links (ADR-0021 rule 7).

## Decision

**A note that links to other notes can be exported as a book: the note itself is the title page, then comes the table of contents, then the notes it links to, in the order of the links. Nothing new is stored in the files.**

1. **Any note can be a book.** When the note to export links to other notes, the export dialog offers "As a book", with the number of notes it would hold. Nothing marks a note as a book; a book is the note and what its links say.
2. **Order.** The notes of a book are the notes its links point to, in the order of their first link in its text, each once. Links are resolved as the exporter resolves them (ADR-0021 rule 7). The book note itself does not count again. Only the book's own links count: links in its notes link inside the document as before but add no notes. Links to the web and to attachments are not notes of the book.
3. **What is left out, and reported** (ADR-0021 rule 12): a link to a note in the trash (rule 11), to a note that is not there, or to a title more than one note has.
4. **Title page.** The book note's text is printed on a page of its own, centred, starting a third down the page, with its first heading as the largest line. Left out of it, besides the front matter and the final tag line (ADR-0021 rules 1 and 4): every paragraph and list item that holds nothing but links to notes of the book, with spaces and punctuation; the table of contents takes their place. Text around a link, a picture, a subtitle, an author and a date stay.
5. **Table of contents and page numbers** as ADR-0029, after the title page and listing the notes of the book in its order. The title page counts as page 1 and shows no number.
6. **Document title** (metadata, the proposed file name) is the book note's title, not the name of a folder.
7. **Formats.**
   - PDF (both platforms, ADR-0022, ADR-0037), ODT and DOCX: the title page alone on the first page, each note from a new page as before.
   - HTML: the title page at the start, a rule below it.
   - EPUB: the title page as the book's first page, in place of Pandoc's own.
8. **Layouts.** One document (one file, or a folder with its pictures) as above. One file per note and the zip archive: the book note and its notes as files of their own, their names starting with their place in the book (`00 Schnipsle manual`, `01 Getting started`, …), so a file list shows them in order.
9. **Where.** The export button in the note's bar on the Mac and the iPhone. The choices of ADR-0029 and the paper are as for any export.

No change to the vault format: a book is an ordinary note with links.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| A: a book note whose links give the order and which is the title page (chosen) | Plain Markdown that syncs and ships with the manual; other editors show it as a list of links; no format change; one note to keep | Links typed by hand until note links get completion (F-12); a note in two books needs two links, which is fine |
| B: an `order:` field in each note and a `title_page: true` note | Lists could sort by it | Format change; reordering rewrites several notes and their history; one order per note, so a note cannot be in two orders |
| C: order and title page in the export dialog only | No new rule for notes | Not stored: the manual's order cannot ship, every export sets it again |
| D: smart folders with an explicit list of notes | In the sidebar | Format change (format.md §9); a title page still needs another rule |
| Title page: the book note without its lists of links (chosen) | The note reads well as a note and as a title page | A rule about which blocks are left out |
| Title page: the whole book note | Nothing left out | The list of links is printed twice, once as the contents |
| Title page from fields in the dialog (title, subtitle, author) | Simple | Not stored; no picture, no free text |

## Consequences

- Positive: any set of notes prints as a document with a title page and an order of one's own, such as the manual, a songbook or a recipe collection; the same note is the book's table of contents in the app.
- Negative: nothing marks a note as a book; a tag such as `#book` gathers them in a tag folder, and the manual's book note carries it as an example (ADR-0048). No list in the app shows the notes in book order. A wrong link is found only by the export's report.
- Follow-up work:
  - Core: the notes of a book, in order, with what is left out (`schnipsle-core`, with tests on `links.rs`).
  - Export: `Input` gets the book note; the title page in each format and layout; golden files with a book in the sample vault (`tests/fixtures/export/vault/`); ADR-0021 rule 3 and ADR-0029 rule 2 are amended by this.
  - The export dialog on the Mac and the iPhone.
  - Later, with F-12: completion of note links, and a book shown as a list in the app.
