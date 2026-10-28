# Import fixtures

Sample exports for the importers (format.md §12, ADR-0012, ADR-0019). All text, names, IDs and attachments are invented. Only the structure follows real exports: Apple Notes on macOS 27.0.1 in German, 137 notes in 3 folders, plus `apple-notes-metadata.json` for 679 notes; Simplenote, 463 notes plus 166 in its trash. The expected notes for each sample come with the adapters.

## `pipeline/`

Golden files for the pipeline itself (`crates/import/tests/pipeline.rs`). Each case gives a batch as JSON, the form an adapter hands over. `expected/` holds:

- `plan.json`: the report after the import;
- `notes/`: the notes written;
- `attachments.txt`: the attachments stored;
- `import-log.yaml`: the log, with snapshot names, which carry the real time, replaced by `<snapshot>`.

The test clock is 2026-10-29T10:00:00Z, and ids count up. Run the tests with `SCHNIPSLE_BLESS=1` to rewrite `expected/` after a deliberate change, then review the diff.

- `basic/`: placement, collisions, folder and source tags, NFC and line endings, one attachment shared by two notes, unreadable attachments, dates missing or not understood, a duplicate `source_id`, front matter of another tool, an empty note.
- `reimport/`: a second import after one note was edited here and another deleted. One note is updated, one added, and four are skipped (changed here, not newer, no modification time, missing).

## `apple-notes/export/`

What Notes writes with Ablage › Exportieren als › Markdown for notes selected in one folder, saved into a Finder folder per Notes folder, plus `apple-notes-metadata.json` from `scripts/apple-notes-metadata.js`.

The export as Notes writes it:

- Every line ends with two spaces, except table rows and the empty line before a table. A file ends right after its last line. An empty note is just `"  \n"`.
- There is no front matter, no ID and no date. The file times are the time of the export.
- The title is the first line. It can be `# Title`, `## Title`, plain text, `**bold**` or a link.
- Notes' own tags are already in the text (`#recipe #baking`).
- Attachments are in `Attachments/<UUID>.<ext>` next to the notes and are linked as images, PDFs too. The alt text is the file name, `PDF`, or text Notes recognized in the picture.
- Other quirks:
  - stray U+2028 at the end of some lines;
  - `++underlined++`;
  - checklists as `- [ ]` (the real export had no ticked items);
  - tables;
  - numbered and `*` lists;
  - `>` quotes.
- The file name is the note's name without `:`, `"` and `/`. Notes itself shortens long names with U+00A0 and `…`, so the metadata `name` ends that way too.

The cases:

| File | Metadata | What it tests |
| --- | --- | --- |
| `Radio/Antenna tuner.md` | p101, nested folder `Amateurfunk & Radio` › `QRP Kits` | Table, U+2028, PNG attachment, `++…++`. Placement follows the Notes folder, not `Radio`; folder tag `#Amateurfunk-Radio/QRP-Kits` |
| `Radio/Field day checklist.md` | p102 | Checklist |
| `Radio/Keyer firmware … bench ….md` | p103, name shortened with `…` | `:` and `"` missing from the file name; `#define`/`#include` in plain text become tags (listed in the report) |
| `Songs/Morning song.md` | p201; p202 has the same name in `Recently Deleted` | Plain-text title; chords `C#m` are not tags; the snippet picks p201 |
| `Mixed/Pizza dough.md` | p301 and p302: same name, same folder | The export holds only one of them. The snippet picks p301, and the report lists p302 as not exported |
| `Mixed/Bread rolls.md` | p303, folder `Notes`, smart folder `✱ Recipes` | `##` title, PDF and HEIC attachments, OCR alt text; smart folders give no tag |
| `Mixed/Club history the first ten years.md` | p401 `Club history: the first ten years` | Bold first line, `:` dropped in the file name |
| `Mixed/Trip planning 2026-03-14 0930.md` | p402 | Link as first line; `:` dropped in the file name |
| `Mixed/Empty note.md` | p403, empty snippet | Empty note |
| `Mixed/Old shopping list.md` | none | No match: keeps the export time and its export folder; `source_id` `export:Mixed/Old shopping list.md` |
| (not exported) | p404, p405 locked | Notes that were not exported or are locked are not imported; locked notes are listed in the report |

`Mixed/` stands for an export made from the smart folder `✱ Recipes`, which mixes notes from several Notes folders.

## `simplenote/`

`simplenote-export.zip` is laid out like the zip Simplenote writes:

- `source/notes.json` holds the data;
- each active note is also a `.txt` file at the top level, and each trashed note one in `trash/`;
- some file names are CP437 without the UTF-8 flag (`Grüße an alle.txt`, which is not in `notes.json`).

The adapter reads only `source/notes.json`, also given unpacked as `notes.json`.

| Note (first line) | What it tests |
| --- | --- |
| `Lake trip` | `pinned: true`, CRLF |
| `Club song` | `markdown: true` with emphasis |
| `Repeater list` | `tags: ["radio", "Ö-Club"]`. `#radio` is already in the text, so only `#Ö-Club` goes on the final line |
| `Pizza dough` | `tags: [""]` (an empty tag, as in the real export, gives no tag); same title as an Apple note |
| `Keyer sketch` | `#define`, `#include` in plain text |
| `Café list` | NFD text (e + U+0301), written NFC; `publicURL` |
| (empty) | Empty `content` |
| `No line break at the end` | No final line break |
| trashed `Old idea` | `trashedNotes` go to the trash (§8) |
