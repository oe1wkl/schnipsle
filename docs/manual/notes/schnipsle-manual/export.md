---
id: 01M8EAJZW0RG5YX4GV23M7X05W
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Export

An export turns notes into documents for people who do not use Schnipsle: a web page, a PDF to print or send, a file for a word processor, or an e-book. The notes themselves stay as they are.

## Starting an export

- **On the Mac:** **File › Export…**.
- **On the iPhone:** **Export** in the bar under a note exports that note; **Export** above the list exports the notes the list shows.

The dialog first asks which notes: the open note, or all notes in the list. The list can show a tag folder, a smart folder or a search, so to export all your recipes, pick their tag folder first. Notes in the trash are left out.

The open note is saved before it is exported.

## Formats

| Format                  | Good for                                                                |
| ----------------------- | ----------------------------------------------------------------------- |
| HTML                    | a web page, to open in any browser or put on a website                  |
| PDF                     | printing and sending; looks the same everywhere                         |
| OpenDocument text (ODT) | editing further in LibreOffice, Pages or Word                           |
| Word (DOCX)             | editing further in Word; Mac only, needs Pandoc                         |
| EPUB                    | reading as an e-book in Books or on an e-reader; Mac only, needs Pandoc |

Word and EPUB are made by Pandoc, a free program that Schnipsle uses if it is on your Mac; it is not part of Schnipsle. Install it, for example with Homebrew (`brew install pandoc`), and the two formats appear in the dialog. [Settings](settings.md) shows which Pandoc is used, and lets you choose another.

## Choices

- **Files:** several notes go into **one document**, each note on a new page (in EPUB, a new chapter), or into **one file per note**, in a folder or a zip archive. Links between the notes lead from file to file where the format allows; in PDF they become plain text, and the report says so. HTML can take copies of the notes' attachments along.
- **Page size:** A4 or Letter, for PDF, ODT and Word.
- **Start with a table of contents:** lists the notes at the start of one document, each with the page it starts on and a link to it. It turns on page numbers.
- **Page numbers:** for PDF, ODT and Word.
- **Keep the line of tags at the end of a note:** without it, the last line of tags is left out, as it is meant for Schnipsle, not for readers.
- **Add the ABC source under each score:** for musicians who want the notation as text, too.

## Saving or sharing

- **Export…** (on the Mac) asks where to save. An export into the vault is refused, so it does not turn up among your notes.
- **Share…** sends the export without saving it first: by Mail or Messages, by AirDrop, to another app, or, on the iPhone, into the Files app or to a printer.

A file whose name is taken in the folder gets a number. Nothing is ever replaced.

## What the export shows

The documents show your notes as Schnipsle shows them:

- headings, lists, tasks (as ☐ and ☑), tables, quotes, code and footnotes;
- pictures, inside the file; pictures in HEIC, HEIF or TIFF are turned into JPEG or PNG, which other programs can show, and in Word and ODT, photos taken on their side are turned upright;
- scores engraved, and song sheets with the chords over the lyrics, transposed as the note says ([Music](music.md));
- the note's title and dates in the document's properties.

The front matter is left out. Formulas are shown as written, as code.

## The report

After an export, Schnipsle lists what is different in the export from the notes, each with the note it is in, which a click opens:

- a picture that was not found, of which the description is shown instead;
- a picture on the web, shown as a link (except in HTML);
- an attachment that is not a picture, of which only its link's text is in the document, except in HTML with copies of attachments;
- a link to a note that is not in the export, which becomes plain text;
- a score that could not be engraved, which is shown as its ABC source.

On the Mac, **Show in Finder** shows the export. The report can share it, too.

#schnipsle_manual
