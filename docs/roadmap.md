# Roadmap and status

- Updated: 2026-11-06
- Current milestone: **M6 – Music**

Each milestone ends in a usable build. Scope can move between milestones; the order should not.

| Milestone | Content | Status |
| --- | --- | --- |
| M0 – Foundations | Decisions (ADR-0001…0014), format spec, repository and CI, iOS feasibility spike | Done 2026-10-14 |
| M1 – Core editor | Open a vault, CodeMirror editor with live preview, images, autosave, safe writes, change detection | Done 2026-10-21 |
| M2 – Index and organization | SQLite/FTS5 index, tag folders with combination subfolders, full-text search | Done 2026-10-27 |
| M3 – Smart folders | Query language, saved searches | Done 2026-10-28 |
| M4 – Import | Apple Notes (macOS 26 Markdown export), Simplenote, HTML, Markdown folders | Done 2026-11-01 |
| M5 – Export | HTML, PDF, ODT built in; Pandoc formats on desktop (desktop first) | Done 2026-11-06 |
| M6 – Music | ABC scores rendered and played, ChordPro song sheets rendered and transposed, both in exports | |
| M7 – Release 1.0 | Export on iOS, manual, packaging and signing, performance and data-safety tests | |
| Later | Note links, extras | |

## M0 results

- Tauri 2 confirmed for iOS by the spike (`docs/spikes/ios-vault.md`); ADR-0003 unconditional.
- Working: folder picker and persistent access on iOS, list/read/write with conflict detection, test UI on macOS, Linux and iPhone.
- C8 (offline editing) passed on 2026-10-15 and C7 (simultaneous edits on two devices) on 2026-10-16, both with the M1 build. In C7, iCloud sometimes keeps the other side as a visible copy (`Name 2.md`) instead of a conflict version; such copies are shown as conflicts once the note has an `id`.

## M1 results

- Vaults follow format.md §1–2 (`vault.yaml`, notes in `notes/`); existing folders are converted only after asking, notes left outside `notes/` are found and moved in on request.
- Editor: CodeMirror 6 with live preview (headings, emphasis, code, links, lists, tasks, quotes, images, tables) and a source toggle; images by paste, drop or picker, stored content-addressed in `attachments/`.
- Data safety: atomic, coordinated writes with conflict checks; iCloud conflict versions and same-id copies resolved without losing text; trash; history snapshots with retention and a history view to read, restore or copy versions; relative links kept working when notes move (ADR-0016).
- Format spec at draft revision 0.8; ADR-0016 and ADR-0017 added. Tested on macOS and on the iPhone (iOS 27).
- Moved to later milestones: loading the editor on demand (bundle ~530 kB), inline formatting inside table cells, a settings view.

## M6 backlog

Draft, to be agreed. Goal: music in notes as the HLD asks for 1.0: ABC scores engraved in the editor and in exports, played back for practice (F-16, F-17; ADR-0013), and ChordPro song sheets with chords over their syllables, transposed, in German or English names, lyrics only on request (F-18; ADR-0014). Both follow format.md §5.6 and §5.7; the file is never rewritten except by an explicit "Apply transposition". Order: core first (indexing, ChordPro model), editor, playback, exports, then a check with real songs on the Mac and the iPhone.

To decide first:

- [x] How exports get the engraved ABC (spike `docs/spikes/abc-export.md`, ADR-0033): the app's web view engraves the scores before the export with the editor's code and hands the SVGs to the export command; inline SVG in HTML and PDF, a picture in ODT, DOCX and EPUB; a block without a score stays source and is reported
- [x] Playback (F-17, a should) is part of M6 (2026-11-06)

Work:

- [x] ABC indexing (format.md §5.6 rule 5; `abc.rs`, golden files in `tests/fixtures/format/5.6-index/`: a score with two voices, `#` in key, chords and comments, and a fragment without `X:`): `T:`, `C:`, `w:` and `W:` into the full-text index, the notes, other fields and `%` comments not; syllables joined into words, also over a line end, `+:` continues a field, ABC accents and HTML entities read as letters; no tags from the block; index schema 6
- [x] ChordPro renderer in `schnipsle-core` (ADR-0014, ADR-0030; `chordpro/sheet.rs`, `chordpro/chord.rs`, golden files in `tests/fixtures/format/5.7-render/`): a sheet of the song's details and sections with each chord over its text, chord lines, comments, tabs and unknown directives; `{chorus}` repeated in the key of its place; transposition of `transpose=`, the viewer and `{transpose}` added up, spelled for the target key in English and German names; `nochords` and lyrics only; `chord_notation` as the vault default (format.md 0.15); `apply_transposition` rewrites only chords and `{key}`
- [x] ChordPro in the editor (ADR-0031; `ui/src/song.ts`, commands in `src-tauri/src/songs.rs`): chords over syllables in the live preview, the source while the cursor is in the block; transpose control, chord names shown (read as written, ADR-0031) and lyrics only per block as view settings while the note is open; "Apply transposition" saves, stores a history snapshot and rewrites the block (§5.7 rule 5), undoable; checked in the editor playground with the golden sheets, not yet in the app
- [x] ABC in the editor (ADR-0032; `ui/src/score.ts`): abcjs 6.7 (MIT) engraves the block in the live preview, every tune, in the text colour; the source while the cursor is in it, and a click on a note puts the cursor there; any abcjs warning keeps the block as source with the warnings listed, each a link to its place; `+:` continuations, HTML entities and `\u` escapes prepared for abcjs with every character in its place; `transpose=` and `nosource`; abcjs a chunk of its own (509 kB), loaded only when a note has music; checked in the editor playground, not yet in the app
- [x] Playback (F-17, ADR-0035): a bar over each score with play and stop per tune, tempo in percent, transposition of score and sound, mute and solo per voice and the chord accompaniment on or off; the FluidR3 piano in the app (`ui/public/sounds/`, 2.1 MB, CC BY 3.0), so it plays offline; checked in the editor playground; not yet in the app, and not yet on the iPhone with the silent switch on
- [x] Music in exports (ADR-0033, ADR-0034): scores engraved by the app's web view before the export (`export_scores`), inline SVG in HTML and PDF, pictures in ODT, DOCX and EPUB, a block that cannot be engraved kept as source and reported (`scoreNotEngraved`); song sheets from the core model, as in the editor in HTML, PDF and EPUB, a monospace line of chords over each line of lyrics in ODT and DOCX; the ABC source under each score as an option; golden files with a score engraved by abcjs 6.7.2 (`tests/fixtures/export/scores/`); checked with LibreOffice, Pandoc and EPUBCheck and in the export playground, not yet in the app or in Word
- [x] Snippet menu for ABC (ADR-0013; `ui/src/snippets.ts`): Music in the bar under the note; outside a score three templates (melody with chords, two voices, choir SATB) after the paragraph, title selected; in a score the key (major, minor, modes) and metre of the tune at the cursor (the `K:` and `M:` lines; a clef stays), another voice with a clef and one bar's rest (`V:1` added where the tune had none, the voice added to `%%score`), a `w:` line, a chord symbol and another tune; one undo step each; a link to the ABC standard. Manual page `docs/manual/music.md`: scores, the menu, ABC in brief, options, playback, song sheets, search, exports. Checked in the editor playground (templates and inserted voices engrave without warnings, phone width), not yet in the app
- [ ] Check with real songs: a choir piece with several voices, song sheets with German names, on the Mac and the iPhone (iOS 27); typing in a long score stays smooth

Not in M6: chord diagrams (`{define}`) and multi-column song sheets (later extension of format.md); MusicXML, MIDI or LilyPond rendered inline (§5.6 rule 7).

## M7 backlog

Draft, to be agreed. Goal: a 1.0 people can install and trust on the Mac and the iPhone, with everything the HLD marks as a must (M) or settled as out of 1.0. Desktop builds are signed and notarized, the iPhone app goes through TestFlight to the App Store. Order: decisions, export on iOS, what the requirements still lack, data safety and performance on devices, then manual, packaging and release.

To decide first:

- [ ] Ways to install (ADR): Developer ID with notarization from GitHub, the Mac App Store (sandbox, see ADR-0022), or both; the App Store for iOS. Updates on the desktop: Tauri's updater or none
- [ ] German in 1.0 (N-06: English and German first), or English only with the strings ready for it
- [ ] What else joins 1.0: import on the iPhone (Markdown folders and the Simplenote zip from Files); a settings view (history retention, Pandoc, later the reading view); renaming and merging tags (F-11)

Work:

- [ ] PDF on iOS: a spike of `UIPrintPageRenderer` with the web view's print formatter (`docs/spikes/pdf-print.md`), then an ADR for the iOS path of ADR-0022, page numbers and table of contents included (ADR-0029)
- [ ] Export on iOS (F-06): HTML, PDF and ODT from the note or the notes in the list, handed to the share sheet or saved to Files; the report as on the desktop. DOCX and EPUB stay desktop only (ADR-0025)
- [ ] History: compare two versions of a note, or a version with the note (F-14 "browse, compare, restore"; browsing and restoring exist)
- [ ] Localisation and access (N-06), as decided: UI strings in one place, German if chosen; every view usable by keyboard on the Mac and with VoiceOver on both
- [ ] Data-safety tests (N-04): a long run of random edits, renames, deletes, restores and outside changes against a model of the vault, which must never lose text; the app killed while saving; a full disk and a read-only vault; C7 and C8 (two devices, offline) again with the release build
- [ ] Performance on devices (N-03): 10,000 notes on the oldest iPhone we support and on an Intel Mac if we build for it; start time, memory, search and export times written down; the editor loaded on demand if start time needs it (bundle ~530 kB)
- [ ] Manual (`docs/manual/`): getting started, vaults and their files, tags and smart folders, search (exists), music (exists), import, export, history and trash, sync with iCloud; reachable from the Help menu
- [ ] Licences: the legal check of how Pandoc is called (ADR-0005, ADR-0025); a list of third-party licences in the app and next to the download
- [ ] macOS packaging: version 1.0.0 everywhere, hardened runtime with the Apple Events entitlement for the Notes import, Developer ID signing and notarization in a CI release job, a DMG; Apple silicon only or universal
- [ ] iOS packaging: App Store Connect record, privacy manifest (`PrivacyInfo.xcprivacy`) with the reasons for the APIs used, export compliance, icon and screenshots, TestFlight; the iOS `Info.plist` without the desktop-only `NSAppleEventsUsageDescription` the build adds
- [ ] Reproducible builds (N-07): what can be rebuilt bit for bit and what cannot (signing, Xcode), documented in `docs/dev-setup.md`
- [ ] Beta: a few weeks of everyday use on the Mac and the iPhone, with others on TestFlight; fixes
- [ ] Release: CHANGELOG section 1.0.0, tag, GitHub release with the DMG and notes, App Store submission

Not in M7, unless decided otherwise above: Linux and Windows builds (N-05 keeps them possible), Android, note links and backlinks (F-12), the importers of F-13, fonts as a choice.

## M5 results

- Export on the desktop (File › Export…) of the open note or the notes in the list: HTML, PDF (macOS) and ODT built in, DOCX and EPUB when Pandoc is installed (ADR-0010, ADR-0021, ADR-0025). One document, or one file per note; A4 or Letter; with or without the final tag line; optionally a table of contents and page numbers (ADR-0029).
- One tree all formats share, so a note looks alike everywhere: front matter into the document's properties, pictures inside the file, real footnotes, links to notes only to notes exported too, raw HTML through an allowlist. PDF through the print path of `WebKit` (ADR-0022) with its dates corrected (ADR-0024); ODT from a writer of our own.
- Pictures that most programs do not show (HEIC, HEIF, TIFF) are converted (ADR-0027), and photos turned only by their orientation are upright in DOCX and ODT (ADR-0028). A first heading of any level gives the title (ADR-0026).
- Nothing is ever replaced: a taken name gets a number, an export into the vault is refused, and the report says what differs from the notes.
- Checked with all 538 notes of the M4 import in every format, no text lost, and by hand in LibreOffice, WordCraft and Books. Word itself is not checked.
- Moved on: export on iOS (M7, with page numbers for printing there); rendered music and song sheets in exports (M6, music); fonts as a choice (below); a table of contents of the headings of one long note, if wanted; exports on Linux and Windows.

## M5 backlog

Goal: export a note, or the notes of a tag folder, search or smart folder, to HTML, PDF and ODT (F-06), plus DOCX and EPUB through Pandoc when it is installed (F-13, desktop). Decisions: ADR-0010 (built-in engines, Pandoc optional and never bundled). Several notes become, by choice in the dialog, one document with a section per note (a page break between notes in PDF and ODT) or one file per note in a folder. The final tag line of a note is left out by default; tags inside the text stay as text. Desktop first; export on iOS (share sheet) moves to M7 (then called M6), since F-06 is a must for 1.0. Order: export rules and fixtures, HTML, PDF spike, PDF, ODT, export UI, Pandoc, check with real notes.

- [x] Export rules and test data: ADR-0021 (front matter into the document's metadata, a single note as written, sections with their titles in one document, the final tag line left out unless kept, pictures embedded or copied, other attachments only in a folder export, links to notes only to notes exported too, music and math as source until the music milestone, raw HTML through an allowlist, no notes from the trash, a report at the end); the sample vault `tests/fixtures/export/vault/` (six notes, three attachments, one note in the trash) with a README of what each note covers, checked by `crates/export/tests/sample_vault.rs`
- [x] HTML exporter in `schnipsle-export`: the rules of ADR-0021 in one step all formats share (`document.rs`, `raw_html.rs`), comrak's HTML with a CSS theme close to the editor's preview; a single file with pictures as `data:` URLs, one document in a folder with an `attachments/` copy, or a document per note; a folder export never replaces what is there; golden files in `tests/fixtures/export/html/expected/`
- [x] PDF spike: `createPDF` makes one long page; the print path (`runOperationModalForWindow:…` with no panels, to a file) gives real pages, A4/Letter, margins and CSS page breaks, and keeps text and links; `runOperation` is unusable, and success does not prove a file. Findings in `docs/spikes/pdf-print.md`, the decision in ADR-0022, the program in `crates/apple/examples/pdf_spike.rs`
- [x] PDF exporter (macOS): `PdfExporter` in `schnipsle-export` has the page of the HTML exporter printed through a `Printer`, `print_pdf` in `schnipsle-apple` is the print path of ADR-0022, the exporter corrects the dates in the finished file (ADR-0024), and the command `export_notes` in the app shell runs it on the main thread. A4 or Letter, 20 mm margins; wide content wraps instead of shrinking the document; golden files in `tests/fixtures/export/pdf/expected/`, and `crates/apple/tests/pdf.rs` prints for real. Checked inside the running app: the window keeps its focus. Links between separate PDF files become text (ADR-0023). Linux and Windows later
- [x] ODT writer (ADR-0010): `OdtExporter` in `schnipsle-export` writes `content.xml`, `styles.xml`, `meta.xml`, the manifest and the pictures, zipped from the tree all formats share: headings, paragraphs, emphasis, lists, tasks, tables with their alignment, code, quotes, real footnotes, pictures, links, a new page per note, A4 or Letter. Links between files of a folder export stay links (rule 7 of ADR-0021, unlike PDF), and an export does not differ by when it ran. Golden files in `tests/fixtures/export/odt/expected/`; LibreOffice converts every file in a CI job of its own, and the samples passed the OASIS schema of ODF 1.2 in a check by hand. Word comes with the check with real notes; Pages does not open ODT
- [x] Export UI (desktop; `ui/src/export.ts`, `src-tauri/src/exporting.rs`): File › Export… with what (this note, or the notes shown in the list, never hits from the trash), format, one document or one file per note, page size for PDF and ODT, keep the tag line; then a save dialog or folder picker, which suggests a name from the title. The open note is saved first, and a change that cannot be saved is said in the dialog. An export into the vault is refused, as its copies would sync next to the notes. The report lists what is different from the notes, each note openable from it, and names the files that got a number because their name was taken (a folder export never replaces anything); the result opens in Finder on request. The choices are remembered. A single note in a folder is offered for HTML only, the one format that copies attachments; one document of several notes in a folder (`Layout::Folder`) stays unused by the dialog. No progress per note yet. Playground without Tauri: `/dev/export.html`
- [x] Pandoc (desktop, F-13; ADR-0025): `PandocExporter` in `schnipsle-export` writes the shared tree as Markdown, with its pictures as files in a temporary folder, for a Pandoc found on `PATH` or in the usual Homebrew folders. DOCX and EPUB are offered only then, with its version shown; never bundled (ADR-0005, ADR-0010). In DOCX title, date of creation and paper are set in the finished package, a new page per note; in EPUB a chapter per note. Links between files stay links in DOCX and become text in EPUB. Pandoc's warnings go into the report. Golden files of the Markdown in `tests/fixtures/export/pandoc/expected/`; the real Pandoc writes DOCX and EPUB in a CI job of its own
- [x] Check with real notes: all 538 notes of the M4 import in every format, as one document, as recipes alone and one file per note, in under 40 s each. No text lost (words of each note compared with each export), LibreOffice opens every DOCX and ODT file, PDFKit every PDF, calibre the EPUB. Found and fixed: HEIC and TIFF pictures missing or broken outside Apple's programs, now converted (ADR-0027); titles with `##` in file names and metadata, now a heading of any level gives the title (ADR-0026); pictures taller than the page cut off in ODT. The notes hold no music or song blocks, so those are covered by the sample vault only
- [x] Open the exports in office programs and e-book readers: DOCX and ODT in LibreOffice and WordCraft, EPUB in Books (2026-11-06). Found and fixed: Pandoc's raw message about SVG pictures, now said plainly; photos turned only by their orientation lay on their side in DOCX and ODT, now turned upright (ADR-0028); no format had a table of contents or page numbers, now two choices of the dialog (ADR-0029); in Books pictures taller than a page and contents links landing a page early, now a stylesheet of ours after Pandoc's. Word itself is not checked

Fonts, for later: notes carry no fonts, and that stays so (code is always monospaced). How they look is the reader's business, so two settings may come:

- Reading view in the app: serif or sans-serif, for headings and for text apart, and larger or smaller text. A setting of the device, never in the vault.
- Export: the same serif or sans-serif choice, with basic fonts per format. PDF embeds what it uses, so it can take the Mac's own (San Francisco, New York, SF Mono). ODT and DOCX carry no fonts, so they name ones nearly every computer has (Arial, Times New Roman, Courier New; LibreOffice on Linux puts in the Liberation fonts of the same widths); fonts are not embedded, for their licences and size. EPUB names only the family, as readers let their users choose. Until then DOCX names Pandoc's Aptos and Consolas, which only Microsoft Office brings; ODT has Arial and Courier New. This needs an ADR.

Not in M5: export on iOS (M7, then called M6); rendered music and song sheets in exports (music milestone; until then they export as code blocks); multi-select in the note list.

## M4 results

- Import from Apple Notes (Markdown export plus `apple-notes-metadata.json` for ids, dates and folders), Simplenote (zip), HTML pages and Markdown folders, ChordPro files included (ADR-0012, ADR-0019, ADR-0020; format.md 0.13 §12).
- One pipeline for all sources: a dry run that writes nothing, then atomic, coordinated writes with an import log; re-import updates only notes unchanged here and newer in the source; File › Imports… undoes an import without deleting anything for good.
- Imported notes land in `notes/<Source>/` with their folders as tags; attachments content-addressed; titles shown as a level-1 heading.
- ⌘-click opens links in notes.
- Checked with the full real exports (about 760 notes) on macOS and, after sync, on the iPhone (iOS 27).
- Moved on: import on the iPhone; links between imported notes (with F-12); Pandoc for HTML; Evernote, Google Keep, Obsidian and Bear (F-13).

## M4 backlog

Goal: bring notes over from Apple Notes and Simplenote (F-03, F-04), plus HTML files and Markdown folders (F-05), without losing text and repeatably. Decisions: ADR-0012, ADR-0019 (notes in `notes/<Source>/` with the source's folders, folders as `#<Folder>` tags, re-import updates only notes unchanged since, undo trashes only unedited notes, desktop only, built-in HTML converter). Order: spec and fixtures, pipeline, Apple Notes, import UI, Simplenote, HTML, Markdown folders, undo.

- [x] Spec and test data: format.md §12 (revision 0.10): imported notes (`source`/`source_id` per source, placement, folder and source tags on a final line), the import log `.notesapp/import-log/<run-id>.yaml`, re-import and undo; conformance fixtures in `tests/fixtures/format/12-import-log/`; anonymized samples modelled on your exports in `tests/fixtures/import/apple-notes/` (10 files, metadata for 13 notes) and `tests/fixtures/import/simplenote/` (zip and `notes.json`, 9 notes)
- [x] Pipeline in `schnipsle-import` (ADR-0012, format.md §12): adapters hand over a `Batch` of `ImportedNote`s; `plan` is the dry run (new, updated, skipped with reason, problems per note, notices) and writes nothing; `apply` stores attachments (§6, links rewritten), writes each note as planned (front matter with `id`, dates from the source, `source`/`source_id`; folder and source tags on a final line; NFC and LF), atomically and coordinated, with a history snapshot before each update, and the import log ahead of the notes it lists. Re-import follows §12.3 (unchanged here and newer in the source); a note changed between dry run and import is left alone. Core gained exact-text note writes, `free_note_path`, `find_note` and `import_log`. Golden-file tests in `tests/fixtures/import/pipeline/` (`SCHNIPSLE_BLESS=1` rewrites them). Source-specific clean-up comes with each adapter; the search index update for the written notes comes with the import UI
- [x] Apple Notes metadata script: `scripts/apple-notes-metadata.js` writes `apple-notes-metadata.json` (id, folder, smart folders, dates, snippet) into the export folder (ADR-0019); tested on your Notes (679 notes, 17 s); a prototype matched 132 of 137 exported files without doubt
- [x] Apple Notes adapter (`apple_notes.rs`; golden files in `tests/fixtures/import/apple-notes/expected/`): the Markdown export as it really is (title in the first line, with or without `#`; line breaks as two trailing spaces; stray U+2028; `++underline++`; `Attachments/<UUID>.<ext>` linked as images, PDFs too, with recognized text as alt text; tables; checklists) matched to `apple-notes-metadata.json` (ADR-0019) for `source_id` and dates; the note's real Notes folder (from the metadata) gives placement and `#<Folder>`, the export folder only for unmatched files; unmatched files, notes the export dropped (same title in one folder) and missing metadata are listed in the report. Two spaces at a line end are kept only where they break a line inside a paragraph; U+2028 at a line end is dropped, inside a line it becomes a line break; PDFs and other non-pictures become links; `++underline++` stays as written (Markdown has no underline). A damaged or newer metadata file stops the import rather than importing with export times, as a later import with it would add every note again. The pipeline now also reports lines like `#define X` that become tags
- [x] Import UI (desktop; `ui/src/import.ts`, `src-tauri/src/importing.rs`): File › Import… with the source, a file or folder picker (for Apple Notes with the two export steps explained, and the metadata script run on request), the dry-run report, then Import with progress and a result summary listing new, updated and skipped notes, each one openable. Import writes exactly the plan of the dry run the user saw (kept in the app, never planned afresh) and updates the search index for the notes written. The metadata script is built into the app and run with `osascript`; `src-tauri/Info.plist` explains to macOS why Schnipsle asks to control Notes. Signing with the hardened runtime (M7) needs the entitlement `com.apple.security.automation.apple-events` as well. Sources other than Apple Notes appear as their adapters arrive. Playground without Tauri: `/dev/import.html`
- [x] Simplenote adapter (`simplenote.rs`; golden files in `tests/fixtures/import/simplenote/expected/`): reads `source/notes.json` from the zip Simplenote exports (crate `zip`, reading only, deflate through zlib-rs), from `notes.json` itself or from the unpacked folder; the zip's `.txt` files, some with names that are not UTF-8, are ignored. Title from the first line; CRLF to LF and NFC; tags as hashtags on a final line unless already in the text; `id`, `creationDate`, `lastModified`, `pinned` kept; the text stays as written whether or not Simplenote marked it as Markdown. Trashed notes are imported and moved to the trash (format.md 0.11 §12.1 rule 6); a note imported before and trashed in Simplenote since is updated as usual, never trashed, and the report says so. File › Import… offers Simplenote with a zip file picker
- [x] HTML adapter (`html.rs`; golden files in `tests/fixtures/import/html/expected/`): converter `htmd` (Apache-2.0, on html5ever) with rules of its own for `<title>`, `data:` pictures, strikethrough, checkboxes and `---`; a single `.html`/`.htm` file or a folder at any depth, skipping hidden entries and the `<page>_files` folders browsers save pictures in; folders below the chosen one become the note's folders and tags. The title is the page's leading level-1 heading, otherwise its `<title>`, which then becomes that heading so the note shows the title it is named by (format.md §5.2). Pictures embedded as `data:` URLs and local files that pictures and links point to inside the chosen folder go into `attachments/`; links to other pages, the web and missing files stay as written, missing ones reported. Text that is not UTF-8 is read as Windows-1252 unless the page declares another encoding, which is reported and not imported. Dates are the file's modification time, so a re-import updates changed pages. File › Import… offers HTML with a folder and a file picker
- [x] Markdown folders adapter (`markdown.rs`; golden files in `tests/fixtures/import/markdown/expected/`): `.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn` files from a folder at any depth or a single file, hidden entries (`.obsidian`, `.git`) left out; the text kept as written, front matter of other tools included (§4), named after its leading heading, otherwise a front-matter `title`, which then becomes that heading (§12.1 rule 7, ADR-0020), otherwise the file; relative links to pictures and files inside the chosen folder moved into `attachments/` and rewritten (shared with the HTML adapter in `files.rs`), missing ones reported; text that is not UTF-8 read as Windows-1252 and reported. ChordPro files (`.cho`, `.crd`, `.chopro`, `.chordpro`) become a note with the `{title}` (or the file name) as a level-1 heading and the file in one `chordpro` block, fenced longer than any backtick run in it (§5.7 rule 9). Dates from the file times; a front-matter `created` is kept. The pipeline now replaces an `id` that a source's front matter brings along (§12.1 rule 1) and reports it, and an import from inside the open vault, or from a folder that contains it, is refused. File › Import… offers Markdown with a folder and a file picker
- [x] Open links from the editor (found while trying the Markdown import): ⌘-click (Ctrl-click) on a link in a note; `Vault::link_target` in core sorts it into web page, note or vault file (never outside the vault, no hidden files, only `http`, `https` and `mailto`), the `open_link` command hands web pages and files to the system with the `open` crate (MIT, desktop only) and returns notes for the UI to open. Backlinks and `[[…]]` links stay with F-12
- [x] Undo import (ADR-0019, format.md §12.4; `undo.rs` in `schnipsle-import`, tests in `crates/import/tests/undo.rs`): `runs` lists the import logs newest first (damaged or newer ones with why they cannot be undone), `plan_undo` is the dry run, `undo` goes through the log in reverse: added notes unchanged since go to the trash, updated ones get the snapshot `replaced` back exactly as it was (after their current text is stored), edited ones get `kept: true`, missing ones and ones in the trash are left (reported as such, also by a re-import: `SkipReason::InTrash`, `UndoAction::InTrash`, found with `Vault::find_trashed`); then `undone` is set. A note the undo cannot change (no snapshot, an I/O error) is kept like an edited one and reported (format.md 0.13 §12.4 rule 4). The index is updated for the notes moved and restored. File › Imports… (`ui/src/imports.ts`) lists the runs with Undo… per run, shows the dry run, then the result; playground `/dev/import.html?imports`
- [x] Check with your full exports: dry run, import, re-import of a newer export, undo; then a look on the iPhone after sync. In a throwaway vault from the library: Apple Notes 135 notes (2 duplicates and 6 notes the export dropped, as known), Simplenote 629 (166 from its trash straight into the trash); the same export again adds and updates nothing; undoing both leaves no notes and 764 in the trash; about 6 s for 600 notes on a local disk. In the app: a newer export of one Notes folder updates only the changed note; the vault reads and edits fine on the iPhone after iCloud sync. Fixed on the way: notes in the trash are reported as such by re-import and undo; Apple Notes with one title but tags in the text are told apart (the metadata snippet shows tags as U+FFFC); search snippets come from the text, so a title hit no longer looks like a second result

Not in M4: import on the iPhone; Pandoc for HTML; Evernote, Google Keep, Obsidian and Bear (F-13); rewriting links between imported notes or pages, which keep the paths they had in the source and so break where a note got another name or folder (comes with note links, F-12).

## M3 results

- Query language complete (`docs/manual/query-language.md`): dates absolute and relative in local time, `has:`, `is:`, `in:trash`, `tag:`, `chord:`; one parser for the search field and smart folders.
- ChordPro song sheets searchable by lyrics, title and people, and by chord (`has:chord`, `chord:Am`); the parser is ready for rendering in the music milestone.
- Smart folders (F-10; format.md 0.9 §9, ADR-0018): saved, nested searches in `.notesapp/smart-folders.yaml`, edited as operations so changes from two devices both survive; four sort orders.
- Pinning: `pinned` in the front matter without touching `modified`; pinned notes first in every list.
- UI: smart folder tree above Tags (in the Tags view on the phone), "Save" for a search, a form to edit and delete, reorder and nest by drag and drop or keyboard. Tested on macOS and on the iPhone (iOS 27).
- Index schema 4 (rebuilds itself once).
- Known: "Recently changed" in the note list uses file times, so unpinning moves a note up there; smart folders sorted by `modified` use the front matter and are not affected.
- Moved on: ChordPro rendering and transposition (music milestone), renaming and merging tags (F-11), a setting for the combination depth, loading the editor on demand.

## M3 backlog

Goal: saved searches (F-10) on a full query language. Order: query language, ChordPro indexing, smart folders in the core, pinning, UI.

- [x] Query language complete (HLD §7; `dates.rs`, `query.rs`, `search.rs`): `created:`/`modified:` with `2026`, `2026-10`, `2026-10-23`, `>`, `<`, `>=`, `<=`, ranges `a..b`, `today`, `yesterday`, `Nd`/`Nw`/`Nm`/`Ny`; `has:image|task|link|chord`, `chord:Am`, `is:pinned|untagged`, `in:trash`, `tag:x`; index schema 2 for the fields it needs; reference in `docs/manual/query-language.md` (format.md §9)
- [x] ChordPro indexing (format.md §5.7 rule 8; `chordpro.rs`, golden files in `tests/fixtures/format/5.7-index/`): lyrics without chord brackets and the `title`, `subtitle`, `artist`, `composer`, `lyricist` directives in the full text, chord names in `note_chords` for `has:chord` and `chord:Am`; index schema 3; the parser is meant to carry rendering and transposition in the music milestone
- [x] Smart folders in `schnipsle-core` (format.md 0.9 §9, ADR-0018; `smart_folders.rs`, golden files in `tests/fixtures/format/9-smart-folders/`): read and write `.notesapp/smart-folders.yaml` atomically and coordinated, an edit applied again if the file changed meanwhile; unknown keys, values and entries kept (`yaml-rust2`); add, change, remove (subfolders move up), move and nest with cycle check; `Index::search_sorted` for the four `sort` orders (index schema 4)
- [x] Pinning (format.md §4): `Vault::set_pinned` changes only the `pinned` key (unpinning removes it) and keeps `modified`, with the usual conflict check; pinned notes first in the note list, tag folders, search and smart folders (`pinned` on `NoteEntry` and `SearchHit`); a Pin button next to History and a 📌 in the lists
- [x] UI (`ui/src/smart-folders.ts`, `src-tauri/src/smart_folders.rs`): "Smart folders" above Tags, nested; "Save" next to the search field; name, query, parent and order in a form, delete with confirmation; reorder and nest by dragging the ≡ handle (pointer events, so also by touch) or with the arrow keys on it; on the phone inside the Tags view. A change to `smart-folders.yaml` from elsewhere refreshes the tree (`VaultChanges.smartFolders`).

## M2 results

- Search index (ADR-0009): SQLite/FTS5 per device in the app's cache folder, synced in the background when a vault opens and kept current by change events and the app's own writes; trashed notes indexed.
- Tag folders (ADR-0008): hierarchy and combination subfolders with counts, in a tag tree above the note list (a Tags view on the phone).
- Full-text search: words (prefix), phrases, tags, exclusions, `OR`, parentheses; bm25 ranking with snippets.
- Copies with the same `id` found in every folder.
- Performance (N-03): with 20,000 generated worst-case notes all searches and tag queries 2–48 ms on a Mac (CI job in release mode); 5,000 notes on an iPhone 18 Pro instant after a first build of a few seconds.
- Moved on: saved searches and the remaining operators (M3), a setting for the combination depth, renaming and merging tags (F-11), splitting the open-note logic in `main.ts` further.

## M2 backlog

Goal: notes findable by tags and full text, under 200 ms for 10,000 notes on the iPhone (N-03). Order: index, sync, same-id copies, tags, search, performance, UI.

- [x] Index in `schnipsle-core` (ADR-0009, `index.rs`): `rusqlite` with bundled SQLite and FTS5; tables for notes (id, path, title, created, modified, hash, state), FTS5 over title and plain text (`unicode61`, diacritics removed), note–tag, note–link, note–attachment; schema version, rebuild on mismatch; one file per vault id in the per-device cache folder, never in the vault
- [x] Keep the index current (`index_sync.rs`, `src-tauri/src/indexing.rs`): background sync when a vault opens (path, modification time and size, hash only on difference), then change events and own saves; trashed notes indexed as `trash` (F-15)
- [x] Copies with the same `id` found vault-wide via the index (carried over from M1): `NotesById`, answered by the index in the app; candidates are checked on disk
- [x] Tag folders (ADR-0008, `tag_folders.rs`; shown in the UI with the last package): tags by normalized key with the most common spelling, hierarchy (`#a/b` counts for `#a`), combination subfolders A › B as intersections with counts, depth fixed at 3, "Untagged"
- [x] Search (`query.rs`, `search.rs`; shown in the UI with the last package): query parsed to an AST and compiled to parameterized SQL (shared with tag folders); M2 syntax: words (prefix), `"phrases"`, `-term`, `#tag`, `OR`, parentheses; `bm25` ranking and highlighted snippets
- [x] Performance: generator for 20,000 notes (`crates/core/tests/support/generate.rs`, also `--example generate_notes`), timing test in CI in release mode (N-03). On a Mac (M-series), worst case with very common words: searches 2–34 ms, tag folders ≤ 48 ms, building the index 5.4 s, a sync with nothing changed 38 ms
- [x] Check on the iPhone with a generated test vault: 5,000 notes on an iPhone 18 Pro (iOS 27), first index build a few seconds, search and tag folders instant
- [x] UI: split `ui/src/main.ts` into modules (`types.ts`, `dom.ts`, `images.ts`, `sidebar.ts`; the open-note logic stays in `main.ts`); search field and collapsible tag tree above the note list (hierarchy and combinations visually distinct, with counts); Tags as its own view on the phone

Not in M2: saved searches and the operators `created:`, `modified:`, `has:`, `is:`, `in:trash` (M3, same parser); a setting for the combination depth (when needed, with format.md and an ADR); renaming and merging tags (F-11, M3 or later).

## M1 backlog

Carried over from the spike:

- [x] Replace polling with file events: `notify` on desktop, `NSFilePresenter` on macOS and iOS (ADR-0015)
- [x] Use `NSFileCoordinator` for reads and writes on Apple platforms (format.md §10, ADR-0015: `schnipsle-apple`)
- [x] Make check-and-rename atomic enough: hash re-checked after the temp file is flushed, creates never replace (file coordination still open above)
- [x] Move the iOS bookmark from `UserDefaults` to Application Support (`vault.bookmark`, excluded from backups; an old one is moved over once)
- [x] Tauri iOS template override so the "Build Rust Code" PATH fix survives `ios init` (`src-tauri/templates/ios/project.yml`; checked by regenerating in a scratch copy)
- [x] Own menu bar instead of Tauri's default: Close Window only in File, next to New Note and Open Vault…; Window has Minimize, Zoom and Full Screen
- [x] Detect and show iCloud conflict versions (`NSFileVersion`, ADR-0006); C7 and C8 passed on the iPhone
- [x] Show iCloud duplicate copies (`Name 2.md`) as conflicts: two files with the same `id` (format.md §4); same folder for now, vault-wide with the index (M2)

Editor:

- [x] CodeMirror 6 with inline live preview and a source-mode toggle (ADR-0007): headings, emphasis, inline code, links, lists, tasks, quotes, images and tables (cells as plain text); the editor makes the bundle ~530 kB, load it on demand later
- [x] Vault layout (format.md §1–2): `.notesapp/vault.yaml` identifies a vault, notes live in `notes/`; a folder without `vault.yaml` is converted only after asking (its contents move into `notes/`); newer or damaged `vault.yaml` opens read-only
- [x] Notes left outside `notes/` (e.g. created by an older build on another device after the conversion) are detected, and the app offers to move them in
- [x] Note list with titles per format.md §5.2, sorting, new/rename/delete (delete → trash, format.md §8); rename follows the title when you leave a note, Trash view with restore
- [x] Front matter (`id`, `created`, `modified`) written on first save (format.md §4), `modified` updated on each change
- [x] Images: paste/drag-drop/picker → `attachments/` by SHA-256 (format.md §6), relative links, shown in live preview
- [x] Keep relative links working when a note lands at another folder depth (restore into `notes/` as fallback, moving notes in from the vault root): rewritten, old text in the history (ADR-0016)
- [x] Autosave with history snapshots (format.md §7, ADR-0011): after a 30 s typing pause, when leaving a note, before delete, and when another version replaces the open note or wins a conflict; duplicates skipped; retention (7 days all, 90 days daily, then weekly) runs when a vault opens
- [x] History view: list a note's snapshots, show one, restore it or copy it as a new note (ADR-0011)
- [x] Retention settings from `.notesapp/settings.yaml` (format.md §7, ADR-0017): `history_keep_all_days`, `history_keep_daily_days`
- [x] Phone layout: list and editor as separate views, with a back button; the bottom bar wraps

Housekeeping:

- [x] Merge `spike/ios-vault` into `main` via pull request (first PR run of the commit-message check)
- [x] Merge the Dependabot pull requests for GitHub Actions
