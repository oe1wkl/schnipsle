# ADR-0025: DOCX and EPUB through Pandoc

- Status: Accepted
- Date: 2026-11-05
- Deciders: Willi Kraml
- Related: F-13, ADR-0005, ADR-0010, ADR-0021, ADR-0023

## Context

ADR-0010 makes Pandoc an optional exporter on desktop and leaves open how it gets a note. All formats share one tree (ADR-0021), and a note must look alike in all of them. Trying Pandoc 3.12 showed some things the rules have to deal with:

- `data:` pictures work, but a DOCX then holds each picture twice, as its source is copied into the picture's description.
- `--sandbox` keeps Pandoc from reading files and the network, but it needs `data:` pictures and sets all dates to 1970.
- In DOCX a title can only be given together with a title paragraph on the first page, which a note exported as written does not have (ADR-0021 rule 2). The paper needs a reference document.
- Link targets are compared undecoded, so a percent-encoded `#grüße` finds no bookmark.
- Pandoc's Markdown extensions read text differently: with `attributes`, the `{.b}` in `` `a`{.b} `` disappears.
- An e-book reader follows no link out of its book.

## Decision

1. **Input.** The shared tree is written as `CommonMark` with comrak, for `commonmark+pipe_tables+footnotes+strikeout+attributes+raw_attribute`: only what the tree needs. Every `{` in text becomes an entity, so nothing is read as attributes; a code block whose info string starts with `{=` loses it, so it is not read as raw output. Pictures go as files in a temporary folder that also holds the Markdown, deleted afterwards. Pandoc is run in that folder. It fetches nothing, as pictures on the web are links already (ADR-0021 rule 5), so no `--sandbox`.
2. **Finding it.** The first `pandoc` on `PATH` or in Homebrew's folders that says its version. It is looked for each time the dialog opens, and DOCX and EPUB are offered with that version only then (ADR-0010). There is no minimum version: an option Pandoc does not know ends in its own error message, shown in the dialog.
3. **Metadata (ADR-0021 rule 1).** For one note the modification date goes through `SOURCE_DATE_EPOCH`. EPUB gets title and day of creation from Pandoc's `--metadata`, and shows them on a title page, as a book does. DOCX gets no title from Pandoc. The finished package is changed instead, in `docProps/core.xml` (title, date of creation) and in the last section of `word/document.xml` (paper, 20 mm margins as in PDF and ODT). All other parts are copied as they are. A document of several notes has no dates of its own and keeps the time of the export.
4. **Several notes (ADR-0021 rule 3).** Each note's first heading gets its anchor as an identifier. DOCX has a raw page break between notes, EPUB a chapter per note.
5. **Links between files (ADR-0021 rule 7).** DOCX keeps them as relative links, like ODT. In EPUB they become text and are reported as `noteInOtherFile`, like PDF (ADR-0023).
6. **Attachments.** As in PDF and ODT (ADR-0021 rule 6), they stay behind and are reported, also in a folder.
7. **What Pandoc says.** Its warnings go into the report, such as an SVG it cannot convert for lack of `rsvg-convert`. If it fails or runs longer than 5 minutes nothing is written, and what is there stays.

Pandoc stays a separate program the user installs, never linked or bundled. The legal check before the first desktop release (ADR-0005) stays open. Typst-based PDF and reference documents (ADR-0010) are not part of this.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Markdown of the shared tree, pictures as files (chosen) | Follows the shared rules; real footnotes; readable golden files without Pandoc | Text must be protected from Pandoc's reading of it; no `--sandbox` |
| `data:` pictures with `--sandbox` | Pandoc can read nothing else | Pictures twice in a DOCX; dates of 1970 |
| The HTML of the HTML exporter | No Markdown to write; `<b>` stays bold | Footnotes become text at the end of each note, not real footnotes |
| Pandoc's JSON format, written directly | Nothing to protect | A second writer of the tree; depends on Pandoc's API version |
| The note's own text | Simplest | None of ADR-0021 applies |

## Consequences

- Positive: DOCX and EPUB follow the same rules as the other formats; the Markdown is tested without Pandoc, the real conversion in a CI job of its own.
- Negative: raw HTML elements keep only their text in DOCX, as in ODT, and DOCX has Pandoc's default look. The EPUB language is Pandoc's default, `en-US`, as a note says nothing about its language.
- Not tried yet: Word itself, which comes with the check with real notes.
