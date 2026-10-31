# Vault File Format Specification

- Format version: **1** (draft)
- Spec revision: 0.12, 2026-10-31 (§12.1 rule 7 the source's title as a heading; 0.11: §12.1 rule 6 notes from the source's trash; 0.10: §12 imported notes, import log, re-import and undo; 0.9: §9 reading and writing smart folders; 0.8: §7 retention settings; 0.7: §5.5 relative links are kept working when a note moves; 0.6: §8 `original_path` is relative to the vault root; 0.5: §8 trash names for notes without `id` and for collisions; 0.4: §5.3 refined by the first implementation, §5.6 music notation, §5.7 lyrics with chords)
- Status: Draft — becomes normative when M1 ships
- Related: ADR-0004, ADR-0008, ADR-0009, ADR-0011, ADR-0013, ADR-0014, ADR-0012, ADR-0016, ADR-0017, ADR-0018, ADR-0019, ADR-0020

The key words MUST, MUST NOT, SHOULD, SHOULD NOT and MAY are used as described in RFC 2119.

This document defines how a vault is stored on disk. Any tool that follows it can read and write notes interchangeably with the app.

## 1. Vault

A vault is a directory containing a `.notesapp/vault.yaml` file. Its layout:

```text
<vault>/
  notes/                         note files (§3), MAY contain subdirectories
  attachments/                   content-addressed attachments (§6)
  .notesapp/
    vault.yaml                   vault identity and format version (§2)
    settings.yaml                user settings shared across devices
    smart-folders.yaml           saved searches (§9)
    history/<note-id>/           version snapshots (§7)
    trash/                       deleted notes (§8)
    import-log/                  one file per import run (§12)
```

- Implementations MUST ignore unknown files and directories, and MUST NOT delete them.
- Per-device data (search index, thumbnails, UI state) MUST NOT be stored in the vault (ADR-0009).
- Files and directories whose names start with `.` inside `notes/` MUST be ignored, except `.notesapp/` at the vault root.

## 2. `vault.yaml`

```yaml
format: 1                     # integer, REQUIRED
id: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T  # ULID, REQUIRED, never changes
name: My Notes                # display name, OPTIONAL
created: 2026-10-13T21:10:00+02:00
```

- A reader that finds `format` greater than it supports MUST open the vault read-only and tell the user.
- A format change that older readers cannot handle MUST increase `format` and MUST come with a migration and an ADR.

## 3. Note files

- Extension `.md`, encoding UTF-8 without BOM, line endings LF. Readers MUST accept CRLF and BOM and SHOULD preserve them when rewriting unchanged lines.
- Text SHOULD be Unicode NFC-normalized. Writers MUST NOT change the normalization of text the user did not edit.
- The file name is cosmetic. Writers SHOULD derive it from the title (lowercased, spaces to `-`, unsafe characters removed, max 80 characters) and MUST resolve collisions by appending `-2`, `-3`, … . Readers MUST identify notes by `id` (§4), never by file name or path.
- A note file consists of an OPTIONAL front-matter block (§4) followed by the body (§5).
- A `.md` file without front matter is a valid note. The app adds front matter the first time it writes the note, never just by reading it.

## 4. Front matter

YAML 1.2, between a first line `---` and the next line `---`.

| Key | Type | Required | Meaning |
| --- | --- | --- | --- |
| `id` | ULID string | yes (once written by the app) | Permanent identity |
| `created` | RFC 3339 timestamp with offset | yes | Creation time |
| `modified` | RFC 3339 timestamp with offset | yes | Last content change |
| `pinned` | boolean | no | Pinned in lists |
| `source` | string | no | Import origin: `apple-notes`, `simplenote`, `html`, `markdown`, … (§12.1) |
| `source_id` | string | no | Identifier in the source system, for re-import and duplicate detection (§12.1) |

- Unknown keys MUST be preserved unchanged (other tools may add their own).
- `tags` MUST NOT be interpreted by format version 1 (tags are inline only, ADR-0008); if present it is preserved as an unknown key.
- If two files carry the same `id`, the reader MUST treat this as a conflict (e.g. a copied file), keep both, and assign a new `id` to the file with the later `created`, after asking or logging.

## 5. Body

### 5.1 Dialect

CommonMark 0.31.2 plus these GitHub Flavored Markdown extensions: tables, task list items, strikethrough, extended autolinks. Additionally recognized:

- Footnotes (`[^1]`).
- Wiki links `[[Title]]` or `[[Title|label]]` (§5.4).
- Math `$…$` and `$$…$$`, rendered only, stored as is.
- Fenced code with language `mermaid`, rendered as a diagram.
- Fenced code with language `abc`, rendered as music notation (§5.6).
- Fenced code with language `chordpro`, rendered as lyrics with chords (§5.7).

Raw HTML is stored as is but rendered through a sanitizer.

### 5.2 Title

The title is the text of the first ATX heading of level 1 (`# Title`) if it is the first non-empty block; otherwise the first line of the body, trimmed to 120 characters; otherwise the file name without extension.

### 5.3 Tags

A tag is recognized by this rule, applied to the NFC-normalized visible text of the note (expressed as a regular expression in Unicode mode; `\p{L}` = letter, `\p{N}` = digit):

```text
(?<![\p{L}\p{N}_&/#])#([\p{L}\p{N}_-]+(?:/[\p{L}\p{N}_-]+)*)
```

with these additional conditions:

1. The captured name MUST contain at least one letter (`#123` is not a tag).
2. A name of exactly 3 or 6 hexadecimal digits that contains at least one digit is a colour code, not a tag (`#00f`, `#a0b1c2`). Names made of letters only stay tags, so words like `#add`, `#cafe` or `#decade` work; the price is that `#fff` is a tag.
3. Tags are not recognized inside code spans, fenced or indented code blocks (including `abc` and `chordpro` blocks), HTML blocks, HTML tags and their attributes, link destinations (so `page#section` is not a tag), image descriptions, math, or the front matter. Text between inline HTML tags (`<span>#x</span>`) is visible text and can carry tags; so can link text and headings.
4. A line starting with `#` followed by a space or end of line is a heading, not a tag.
5. A trailing `/` or `-` is not part of the tag (`#radio/` → `radio`).
6. An escaped hash `\#` is never the start of a tag; this is how a literal `#word` is written.

Normalization for grouping: NFC, then Unicode lowercase mapping. The display form is the most frequent original spelling in the vault.

Hierarchy: `/` separates levels. A note tagged `#a/b/c` also matches queries for `#a/b` and `#a`.

Examples (each row is a unit test in `crates/core/src/tags.rs`):

| Text | Tags |
| --- | --- |
| `Antenna test #radio #projects/tuner` | `radio`, `projects/tuner` |
| `# Heading` | none |
| `Colour #00ff00 and issue #123` | none |
| `Grüße an #Österreich` | `österreich` (display `Österreich`) |
| `` `#notatag` `` | none |
| `https://example.com/page#anchor` | none |
| `C# and F#` | none (`#` not followed by a tag character) |
| `#cw_training, #m32.` | `cw_training`, `m32` |
| `Ideas to #add for the #decade` | `add`, `decade` |
| `Not a tag: \#literal` | none |

### 5.4 Links between notes

- `[[Title]]` links by title; `[[Title|label]]` shows `label`.
- On save, the app MAY rewrite an ambiguous or renamed link to `[[Title|label]]` only if the user confirms; it MUST NOT rewrite links silently.
- The app additionally records `title → id` in the index, so renamed notes can be offered as link targets. Format version 1 does not store ids in links.

### 5.5 Images and attachments in the body

Attachments are referenced with ordinary Markdown links or images using a path **relative to the note file**:

```markdown
![Schematic](../attachments/3f/3f9a0c…e1.png)
[Datasheet](../attachments/b2/b27d…90.pdf)
```

Readers MUST resolve these relative to the note's directory. The link text SHOULD hold the original file name or a description.

A writer that moves a note to another folder SHOULD rewrite the relative link destinations in it that resolved before the move, so they resolve to the same files afterwards, and MUST store the previous version as a snapshot (§7) first. URLs, absolute paths, note links (§5.4) and links that did not resolve MUST stay as written (ADR-0016).

### 5.6 Music notation

Music is written in **ABC notation**, standard 2.1 ([abcnotation.com/wiki/abc:standard:v2.1](https://abcnotation.com/wiki/abc:standard:v2.1)), inside a fenced code block whose info string is `abc`:

````markdown
```abc
X:1
T:Kanon
C:Johann Pachelbel
M:4/4
L:1/4
Q:1/4=60
K:D
V:S name="Sopran"
f2 e2 | d2 c2 | B2 A2 | B2 c2 |
w:Lob und Dank sei dir, o Herr
V:A name="Alt"
d2 c2 | B2 A2 | G2 F2 | G2 A2 |
```
````

Rules:

1. One block holds one or more tunes; each tune starts with an `X:` line. If the first non-empty line of the block is not `X:`, readers MUST behave as if `X:1` preceded it, so short fragments like `K:G` + one line of notes are valid.
2. The block content is stored exactly as written. Writers MUST NOT reformat, transpose or normalize ABC on save.
3. Supported for rendering in format version 1: header fields `X T C M L Q K V P W w` and `%%` directives understood by the renderer; multiple voices (`V:`), lyrics under notes (`w:`) and after the tune (`W:`), chord symbols in quotes (`"Am"`), repeats, ties, slurs, tuplets, grace notes and decorations. Unsupported constructs MUST be shown, not dropped: the renderer shows a warning and the block falls back to its source text.
4. Sharps on notes are written `^F` in ABC, but `#` still appears in key fields (`K:F#`) and chord symbols (`"F#m"`). Because tags are never recognized inside fenced code (§5.3, rule 3), music blocks never create tags.
5. **Indexing:** the text of `T:` (titles), `C:` (composer), `w:` and `W:` (lyrics) lines MUST be added to the full-text index, so a note is found by the title or lyrics of a song it contains. Note content is not indexed.
6. **Info-string options** (OPTIONAL, space-separated after `abc`): `transpose=<semitones>` (display and playback only, file unchanged) and `nosource` (hide the source in read-only views). Unknown options MUST be ignored.
7. Larger scores (MusicXML `.musicxml`/`.mxl`, MIDI `.mid`, LilyPond `.ly`, PDF) are stored as attachments (§6) and linked; format version 1 does not render them inline.

### 5.7 Lyrics with chords

Songs with chord symbols over the lyrics are written in **ChordPro** ([chordpro.org](https://www.chordpro.org/chordpro/chordpro-directives/)) inside a fenced code block whose info string is `chordpro`:

````markdown
```chordpro
{title: Abendlied}
{artist: Trad.}
{key: G}
{capo: 2}

{start_of_verse}
Der [G]Mond ist aufge[D]gangen,
die [G]goldnen Sternlein [C]pran[D]gen
{end_of_verse}

{start_of_chorus}
[Em]Lalala [C]lala, [D7]lala [G]la
{end_of_chorus}

{comment: Repeat chorus twice}
```
````

Rules:

1. A chord is written in square brackets directly before the syllable it belongs to: `[Am]`, `[F#m7]`, `[C/E]`, `[Bb]`, `[H7]`. The renderer places it above that syllable. A bracket group with no following text is shown as a chord-only line.
2. Supported directives in format version 1 (long and short form): `title` (`t`), `subtitle` (`st`), `artist`, `composer`, `lyricist`, `key`, `capo`, `tempo`, `time`, `comment` (`c`), `comment_italic` (`ci`), `start_of_verse`/`end_of_verse` (`sov`/`eov`), `start_of_chorus`/`end_of_chorus` (`soc`/`eoc`), `chorus` (repeat the last chorus), `start_of_bridge`/`end_of_bridge` (`sob`/`eob`), `start_of_tab`/`end_of_tab` (`sot`/`eot`), `transpose`. Section directives MAY carry a label: `{start_of_verse: Strophe 2}`. Unknown directives MUST be kept in the file and shown as a dimmed line, never dropped.
3. Lines starting with `#` inside the block are ChordPro comments and are not rendered. As for all fenced code, they never create tags (§5.3, rule 3).
4. **Chord names:** root `A`–`G` (and `H` for the German B, see rule 6), optional `#` or `b`, optional quality and extensions, optional `/` bass note. Anything else in brackets is shown as written and not transposed.
5. **Transposition:** `{transpose: n}` in the block, an info-string option `transpose=<n>`, or the viewer's transpose control shift displayed chords by n semitones. The file is never rewritten unless the user explicitly chooses "Apply transposition", which then rewrites the chord brackets and keeps the old version in the history (§7). Sharps or flats in the result follow the target key.
6. **Note naming:** an info-string option `notation=german` displays and transposes with German names (`H` = B, `B` = B♭); default is `notation=english`. The setting can also be a vault default in `settings.yaml`.
7. **Info-string options** (OPTIONAL, space-separated after `chordpro`): `transpose=<semitones>`, `notation=english|german`, `nochords` (show lyrics only, e.g. for singers), `nosource`. Unknown options MUST be ignored.
8. **Indexing:** `title`, `subtitle`, `artist`, `composer`, `lyricist` and the lyric text without chord brackets MUST be added to the full-text index. Chord names are indexed separately so the query `has:chord` and the filter `chord:Am` work.
9. Whole files with the extension `.cho`, `.crd`, `.chopro` or `.chordpro` are imported as notes containing one `chordpro` block, with `title` used as the note title.

## 6. Attachments

- Stored at `attachments/<h0h1>/<sha256-hex>.<ext>`, where `<h0h1>` are the first two hex digits of the SHA-256 of the file content and `<ext>` is the lowercased original extension.
- Identical content is stored once.
- Original metadata MAY be stored in a sidecar `attachments/<h0h1>/<sha256-hex>.<ext>.yaml` (`name`, `mime`, `added`).
- An attachment MAY be deleted only if no note in `notes/`, no snapshot in `history/` and no note in `trash/` references it.

## 7. Version history

- Snapshots are stored as `.notesapp/history/<note-id>/<timestamp>.md.zst`, where `<timestamp>` is UTC in the form `YYYYMMDDTHHMMSSZ` (with `-n` suffix on collision).
- Content is the complete note file (front matter and body) compressed with zstd.
- Snapshots are immutable; they are only removed by the retention policy or by the user.
- Retention policy: keep all snapshots for 7 days, then the last snapshot of each UTC day until 90 days, then the last of each ISO week. Both periods are configurable in `settings.yaml` as whole numbers of days (ADR-0017):

  ```yaml
  history_keep_all_days: 7
  history_keep_daily_days: 90
  ```

  A missing or invalid value means its default; a daily period shorter than the first is extended to it.

## 8. Trash

- Deleting a note moves its file unchanged to `.notesapp/trash/<note-id>.md` and writes `.notesapp/trash/<note-id>.yaml` with `deleted` (timestamp) and `original_path`, relative to the vault root (e.g. `notes/projects/tuner.md`).
- A note without a valid `id` is moved unchanged as well, under a newly generated ULID instead of `<note-id>`. If the name is already taken in the trash (e.g. two files with the same `id`), `-2`, `-3`, … is appended, as for note files (§3).
- Restore moves it back to `original_path` (or into `notes/` if that path is taken or does not lie inside `notes/`, resolving collisions as in §3) under the same `id`. Restoring never replaces an existing file.
- The trash is emptied only by explicit user action or a user-configured period. Default: never.

## 9. Smart folders

`.notesapp/smart-folders.yaml`:

```yaml
format: 1
folders:
  - id: 01JA7R0B2C3D4E5F6G7H8J9K0L
    name: Open radio tasks
    query: '#radio has:task -#done'
    parent: null              # id of a parent smart folder, or null
    sort: modified-desc       # modified-desc | modified-asc | created-desc | title-asc
```

The query syntax is defined in the user manual: [Query language](manual/query-language.md).

1. `id` (a ULID for new folders), `name` and `query` are REQUIRED, non-empty strings. A missing `parent` means `null`; a missing `sort` means `modified-desc`.
2. The order of `folders` is the display order among folders with the same parent.
3. A `parent` that names no folder, or that leads back to the folder itself through other parents, MUST be shown as the top level. A `sort` value a reader does not know MUST be shown as `modified-desc`. Both values MUST be kept as written until the user changes them.
4. Entries without the required keys, entries that are not maps, and entries repeating an earlier `id` MUST NOT be shown, and MUST be kept unchanged in their place.
5. Unknown keys, at the top level and in entries, MUST be preserved. Comments and formatting MAY be lost when a writer rewrites the file.
6. A file with `format` greater than the reader supports MUST be shown but MUST NOT be changed. A file that is not YAML, or whose `folders` is not a list, MUST NOT be overwritten.
7. A writer changes the file by reading it, applying the user's change and writing the result under §10. If the file changed since it was read, the writer MUST apply the change again to the new content instead of replacing it (ADR-0018).

## 10. Writing rules

- Writes MUST be atomic: write to a temporary file in the same directory, flush, then rename over the target.
- On Apple platforms writes MUST use file coordination.
- Before overwriting, a writer MUST check that the file's content hash still equals the one it last read; otherwise it MUST treat the situation as a conflict (ADR-0006) and keep both versions.

## 11. Conformance tests

Fixtures in `tests/fixtures/format/` cover every MUST rule in this document, in particular all tag examples in §5.3 and the music rules in §5.6 (fragment without `X:`, multi-voice score, lyrics indexing, `#` inside an `abc` block) and §5.7 (chord placement, every directive, transposition in both note namings, unknown directives preserved). The import log rules of §12.2–§12.4 are covered by `tests/fixtures/format/12-import-log/` (a finished run, a re-import, an undone run with a kept note, an interrupted run, a newer format); sample exports for §12.1 are in `tests/fixtures/import/<source>/`. A change to this spec without matching fixtures is incomplete.

## 12. Imports

How notes from other apps are written into the vault, and the log that makes an import repeatable and undoable (ADR-0012, ADR-0019).

### 12.1 Imported notes

1. An imported note is an ordinary note (§3–§5). Its front matter gets `id` (new), `created` and `modified` (from the source where it has them, otherwise the time of the import), `source` and `source_id`:

   | `source` | Folder | `source_id` |
   | --- | --- | --- |
   | `apple-notes` | `Apple Notes` | the note's `id` from `apple-notes-metadata.json` (ADR-0019), e.g. `x-coredata://…/ICNote/p1356`; for a file without a match `export:` followed by its path in the export, e.g. `export:Baking/Pizzateig.md` |
   | `simplenote` | `Simplenote` | the note's `id` in `notes.json` |
   | `html` | `HTML` | the file's path relative to the folder chosen for the import |
   | `markdown` | `Markdown` | the file's path relative to the folder chosen for the import |

   Paths in `source_id` use `/` and are NFC-normalized.
2. The note is written to `notes/<Folder>/` followed by the source's folders (for Apple Notes the note's folder in Notes), with a file name as in §3. Example: `notes/Apple Notes/Brot und Germgebäck/pizzateig.md`.
3. The text of the note is the source's text converted to Markdown. Changes beyond the conversion are limited to: line endings to LF, NFC normalization, attachments stored under §6 with their links rewritten (§5.5), a heading with the title the source gives the note (rule 7), and a final tag line (rule 5).
4. **Folder tags.** Each folder level of the source gives one tag level: its name, NFC-normalized, with every character that §5.3 does not allow in a tag name replaced by `-`, runs of `-` collapsed to one and `-` at either end removed. Levels that end up empty are left out. Nested folders give one tag with `/` between the levels: `Radio` › `QRP Kits` → `#Radio/QRP-Kits`. If the result is not recognized as a tag by §5.3 (e.g. `#2024` or `#a0b1c2`), the folder gives no tag.
5. Tags the source keeps outside the text (Simplenote `tags`) and folder tags are written on one final line, separated by spaces, after an empty line: `#Radio/QRP-Kits #ÖVSV`. A tag the text already carries (§5.3, compared after normalization) is not repeated. A source tag is normalized like a folder level (rule 4) and left out if the result is not a tag. A note that gets no such tags gets no final line.
6. **Notes in the source's trash** (Simplenote `trashedNotes`) are imported like the others and then moved to the trash (§8), with `original_path` where they were written. The log lists them as `added` with that path; a re-import finds them missing (§12.3 rule 3), and undo leaves them where they are (§12.4 rule 3). A note imported before that the source has since moved to its trash is treated as in §12.3 and is never moved to the trash by an import.
7. **Title.** If the source names the note outside its text (an HTML page's `<title>`, a Markdown file's front-matter `title`, a ChordPro file's `{title}`, §5.7 rule 9) and the text does not start with a level-1 heading, `# <title>` is put at the top of the body, after the front matter, followed by an empty line, so the note shows that title (§5.2). A front-matter `title` stays in the front matter. A leading level-1 heading wins over the source's name (ADR-0020).

### 12.2 Import log

Each import run that writes notes has one file `.notesapp/import-log/<run-id>.yaml`, where `<run-id>` is a ULID created when the run starts:

```yaml
format: 1
id: 01JB2C3D4E5F6G7H8J9K0K1M2N   # equals the file name
source: apple-notes              # as in §12.1
input: Schnipsle-Import          # name of the chosen file or folder, for display
started: 2026-10-29T10:15:00+01:00
finished: 2026-10-29T10:15:42+01:00   # missing while the run is still writing
undone: null                     # time of the undo, or null
notes:
  - source_id: x-coredata://1377F3B4-5733-4698-AF6B-FB430E3F3051/ICNote/p1356
    id: 01JB2C3E0F1G2H3J4K5K6M7N8P   # the note's id (§4)
    path: notes/Apple Notes/Brot und Germgebäck/bierkruste.md
    action: added                # added | updated
    source_modified: 2025-09-15T07:55:31Z   # the source's modification time, if it has one
    hash: 5f0c…e2                # SHA-256 (lowercase hex) of the file as written
  - source_id: '4f7a54ed11e9425db36b31f42f83db46'
    id: 01JB2C3E7Q8R9S0T1V2W3X4Y5Z
    path: notes/Simplenote/kein-netz-songtext.md
    action: updated
    source_modified: 2026-06-18T21:09:29.104Z
    hash: 9a41…07
    replaced: 20261029T091500Z   # the snapshot (§7) of the version this run replaced
```

1. `format`, `id`, `source`, `started` and `notes` are REQUIRED; in each entry `source_id`, `id`, `path`, `action` and `hash` are. Timestamps are RFC 3339 with offset; `replaced` names a snapshot in `.notesapp/history/<id>/` without its extension.
2. `notes` lists every note the run wrote, in the order written. Notes the run skipped are not listed (the dry-run report shows them).
3. A writer MUST create the log before writing the first note and MUST add each entry no later than the note is written; it MAY add entries in batches ahead of writing their notes. It sets `finished` when the run ends. A reader MUST therefore accept entries whose note does not exist or differs from `hash`, and MUST treat a log without `finished` like a finished one.
4. `path` is where the note was written, relative to the vault root. Notes are found by `id` (§3); the path is only a hint.
5. Unknown keys, at the top level and in entries, MUST be preserved. A log with `format` greater than the reader supports MUST NOT be changed or undone, and its entries MUST NOT be used for re-import. A log that is not YAML MUST NOT be overwritten.
6. Logs are kept until the user removes them. Removing a log only removes the ability to undo that run and to update its notes by re-import.

### 12.3 Re-import

1. A note from the source matches the latest entry with the same `source` and `source_id` in a log that is not undone (entries with `kept: true` in undone logs count, §12.4). Without a match the note is added.
2. With a match, the note in the vault (found by the entry's `id`) is replaced only if its current content hash equals the entry's `hash` **and** the source's modification time is later than the entry's `source_modified`. The replaced version MUST first be stored as a snapshot (§7), and the write MUST follow §10. The note keeps its `id`, `created` and file name; the run logs it as `updated` with `replaced`.
3. In every other case (changed in the vault, no newer source, note missing, or no modification time in the source) the note is skipped, and the report says why.

### 12.4 Undo

Undoing a run goes through its log in reverse order. For each entry, with the note found by `id`:

1. If the note's content hash differs from `hash`, the note is kept, and the entry gets `kept: true`.
2. Otherwise, an `added` note is moved to the trash (§8); an `updated` note gets the version in snapshot `replaced` back, after its current version is stored as a snapshot.
3. A missing note is left as it is.

Then `undone` is set to the time of the undo. An undone run cannot be undone again. Undo never deletes text: every version it removes is in the trash or the history.
