# Export samples

`vault/` is a small vault for the exporters (ADR-0010) and the export rules (ADR-0021). Its text is invented; the pictures are the drawings of the import samples, stored under their SHA-256 (format.md §6). Each exporter adds its golden files next to it, in `<format>/expected/`. `crates/export/tests/sample_vault.rs` checks that the vault is as described here.

| Note | Title | What it covers (ADR-0021 rule) |
| --- | --- | --- |
| `notes/welcome.md` | Welcome | Front matter with `pinned` left out (1); emphasis, strikethrough, inline code, a hard break, headings 1–3 (10); web, `mailto:` and bare links (7); a footnote (10); tags in a sentence stay text (4); final tag line `#schnipsle #demo` left out by default (4) |
| `notes/packing-list.md` | Packing list for Field Day | Title from the first line, no heading added for a single note (2); nested and ordered lists, a task list, a quote, a rule (10); `\#notatag`, a colour code and `C#` (4); a last paragraph with words and tags, which is no tag line and stays (4) |
| `notes/Radio/antenna-tuner.md` | Antenna tuner | Keys of an import and an unknown key left out (1); a picture in the vault (5); a PDF attachment (6); a picture that is not there (5); a picture on the web (5); a relative link and a wiki link to notes in the vault, a wiki link with a label to a note that does not exist (7); final tag line `#Radio` (4) |
| `notes/Recipes/bread-rolls.md` | Bread rolls | A JPEG picture (5); a table with left, right and centre alignment and an empty cell, a fenced and an indented code block (10); final tag line `#Recipes #baking` (4) |
| `notes/Songs/abendlied.md` | Abendlied | `chordpro` with `nosource`, `abc` and `mermaid` blocks, inline and block math, all exported as their source (8) |
| `notes/raw-html.md` | Raw HTML | Allowed elements (`b`, `kbd`, `sub`, `span` without its `style`), a `script`, an `iframe` and a `div` with an event handler (9) |
| `.notesapp/trash/…007.md` | Thrown away | In the trash, never exported (11) |

Several notes: the tests use the order of this table. As one document every section starts with its title: `packing-list.md`, which has no level-1 heading, gets `# Packing list for Field Day`; the others start with their own (3).

Reports expected for all six notes (12): the missing picture `attachments/00/0000…0000.png` and the wiki link to "Logbook 2025" in `antenna-tuner.md`; for formats other than a folder export, the PDF attachment not included; for PDF and ODT, the picture on the web shown as a link.

`html/expected/` holds the HTML export (`crates/export/tests/html.rs`): `single-file/` each note alone with its report, `one-document/` all six in one file holding its pictures, `folder/` all six in one document with copies of the attachments, `file-per-note/` a document per note with copies. `copied.txt` lists the copies, which the test compares with the vault.

`pdf/expected/` holds what the PDF export hands to the printer (`crates/export/tests/pdf.rs`): for each PDF its HTML page and a `.job.txt` with title, paper, margin and dates, and the report. `one-document/` is all six notes in one PDF, `file-per-note/` a PDF per note, where links to notes in the other PDFs become text and are reported as `noteInOtherFile`. `crates/apple/tests/pdf.rs` prints `one-document/all.html` for real.
