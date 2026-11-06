# ADR-0021: What an exported note looks like

- Status: Accepted; links between separate PDF files (rule 7) amended by [ADR-0023](0023-links-between-pdf-files.md), between separate EPUB files by [ADR-0025](0025-docx-and-epub-through-pandoc.md); the section heading of a note in one document (rule 3) by [ADR-0026](0026-a-heading-of-any-level-gives-the-title.md); pictures (rule 5) by [ADR-0027](0027-pictures-converted-for-an-export.md); a table of contents of one document, left as follow-up work, by [ADR-0029](0029-table-of-contents-and-page-numbers.md)
- Date: 2026-11-01
- Deciders: Willi Kraml
- Related: F-06, F-13, ADR-0008, ADR-0010, ADR-0013, ADR-0014, [format.md §4–§6](../format.md#4-front-matter)

## Context

ADR-0010 decides the engines (HTML from comrak, PDF from that HTML, an own ODT writer, Pandoc on desktop) but not what an exported note contains. A note file holds more than its reader should see in a document: front matter, a final line of tags that imports add (format.md §12.1 rule 5), links to pictures inside the vault, links to other notes, music and song blocks the app does not render yet. Several notes can be exported at once, as one document or one file each. All formats should follow the same rules, so a note looks alike in HTML, PDF, ODT and through Pandoc, and the rules need test data before the exporters are written.

## Decision

An export shows a note as the app shows it, without what is only there for the vault. All formats follow these rules; the exporters differ only in how they write the result.

1. **Front matter** is left out. The title (§5.2), `created` and `modified` go into the document's metadata (HTML `<title>` and `<meta>`, ODT `meta.xml`, PDF document info). Other keys are not exported.
2. **Title.** A single note is exported as written: a note titled by its first line gets no added heading. The document title is the note's title.
3. **Several notes**, in the order of the list they come from, become by choice either
   - one document: each note is a section; a note that does not start with a level-1 heading gets one with its title, so every section starts with its title; PDF and ODT start each note on a new page. The document title is the name of the tag folder, search or smart folder; or
   - one file per note in a chosen folder, named after the note's title (characters a file name cannot hold replaced, collisions numbered `-2`, `-3`, … as in §3).
4. **Final tag line.** If the last block of a note is a paragraph of nothing but tags (§5.3) and spaces, it is left out unless the user chooses to keep it. Tags anywhere else stay as the text they are, without links or styling; `\#word` shows as `#word`.
5. **Pictures** in the vault (§5.5, §6) are included: as `data:` URLs in a single HTML file, PDF and ODT; copied with their `attachments/…` paths next to the files in a folder export. A picture that is not there shows its description in brackets and is reported. Pictures on the web stay web references in HTML; PDF and ODT do not fetch them (the export works offline, N-02) and show them as a link with their description.
6. **Other attachments** (PDFs, audio, …) are copied in a folder export and linked. A single file, PDF and ODT keep the link text without the link and report that the attachment is not included.
7. **Links to other notes** (relative `.md` links and `[[Title]]`, `[[Title|label]]`, §5.4) point to that note inside the export if it is exported too: an anchor in one document, the other file in a folder. Otherwise only the link text (or `label`, or `Title`) remains, without a link. Links to the web and `mailto:` stay as they are.
8. **Music and songs.** Until the music milestone, `abc` and `chordpro` blocks are exported as code blocks with their source; info-string options such as `nosource` are not applied yet. `mermaid` blocks and math are exported as their source too.
9. **Raw HTML** passes through an allowlist: formatting elements without attributes (`b`, `i`, `u`, `s`, `sub`, `sup`, `mark`, `kbd`, `br`, `span`, `small`); everything else, scripts, styles, frames, forms and event attributes included, is left out with its text kept. ODT keeps only the text.
10. **Everything else** of the dialect (§5.1) is exported as rendered Markdown: headings, emphasis, strikethrough, code, quotes, lists, tables with their alignment, footnotes (at the end of each note in HTML and PDF, as real footnotes in ODT), task lists with ☐ and ☑.
11. **Notes in the trash** are never exported.
12. **Report.** Like an import, an export ends with a report: pictures not found, attachments not included, links to notes not exported.

The rules are tested with the sample vault in `tests/fixtures/export/vault/`; each exporter adds golden files for it.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Export as the app shows a note, vault-only parts left out (chosen) | Documents read like the note; same result in every format | Rules to write and test; the exported text differs from the file |
| Export the file text as written, front matter and tag line included | Simplest; nothing lost | Documents start with YAML and end with tags meant for the vault |
| Rules per format | Each format can do its best | Notes look different in HTML, PDF and ODT; more to test |

## Consequences

- Positive: one set of rules and one sample vault for all exporters; documents carry no vault internals.
- Negative: links to notes not exported lose their target; music shows as source until the music milestone.
- Follow-up work: exporters and golden files (M5 packages 2–7); rendered music and songs in exports with the music milestone; a table of contents for one document when wanted.
