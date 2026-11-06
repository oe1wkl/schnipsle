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

Reports expected for all six notes (12): the missing picture `attachments/00/0000…0000.png` and the wiki link to "Logbook 2025" in `antenna-tuner.md`; for formats other than a folder export, the PDF attachment not included; for PDF and ODT, the picture on the web shown as a link; for a PDF or EPUB per note, links to the other files as text (ADR-0023, ADR-0025).

`html/expected/` holds the HTML export (`crates/export/tests/html.rs`): `single-file/` each note alone with its report, `one-document/` all six in one file holding its pictures, `folder/` all six in one document with copies of the attachments, `file-per-note/` a document per note with copies. `copied.txt` lists the copies, which the test compares with the vault.

`pdf/expected/` holds what the PDF export hands to the printer (`crates/export/tests/pdf.rs`): for each PDF its HTML page and a `.job.txt` with title, paper, margin and dates, and the report. `one-document/` is all six notes in one PDF, `file-per-note/` a PDF per note, where links to notes in the other PDFs become text and are reported as `noteInOtherFile`. `crates/apple/tests/pdf.rs` prints `one-document/all.html` for real.

`odt/expected/` holds the ODT export (`crates/export/tests/odt.rs`): for each file `content.xml`, `meta.xml`, `manifest.xml` and `files.txt`, the size of its other parts, in `one-document/all/` for all six notes and `file-per-note/<title>/` for a file each, plus `styles-a4.xml` and each layout's report. Links between the files of `file-per-note/` stay links, to `../<title>.odt`. `cargo test -p schnipsle-export --test odt -- --ignored` has LibreOffice convert all of them, and a note with more of the dialect, to PDF and text (`SOFFICE` names the program, `SCHNIPSLE_ODT_KEEP` a folder that keeps the results); CI runs this in a job of its own.

`pandoc/expected/` holds what the export hands to Pandoc (`crates/export/tests/pandoc.rs`, ADR-0025), in `docx/` and `epub/`: for each file its Markdown and a `.job.txt` with title, dates and pictures, and the report. `one-document/` is all six notes in one file, with a page break between them in DOCX, `file-per-note/` a file each, where links to the other files stay links in DOCX and become text in EPUB. `cargo test -p schnipsle-export --test pandoc -- --ignored` has the real Pandoc write them and looks into the results (`PANDOC` names the program, `SCHNIPSLE_PANDOC_KEEP` a folder that keeps them); CI runs this in a job of its own.

`pictures/turned.jpg` is a small invented JPEG stored 24 × 40 pixels with orientation 6, so it shows as 40 × 24: `crates/export/tests/pictures.rs` checks that ODT gets it upright and HTML keeps it as it is (ADR-0028).

A table of contents and page numbers (ADR-0029) are checked without golden files: in `odt.rs` and `pandoc.rs` for all six notes, and with `--ignored` in the real DOCX and EPUB, and in `crates/apple/tests/pdf.rs`, which prints all six with both and reads the numbers back (`SCHNIPSLE_PDF_KEEP` names a folder that keeps the PDF).
