# Roadmap and status

- Updated: 2026-10-28
- Current milestone: **M4 – Import** (plan not written yet)

Each milestone ends in a usable build. Scope can move between milestones; the order should not.

| Milestone | Content | Status |
| --- | --- | --- |
| M0 – Foundations | Decisions (ADR-0001…0014), format spec, repository and CI, iOS feasibility spike | Done 2026-10-14 |
| M1 – Core editor | Open a vault, CodeMirror editor with live preview, images, autosave, safe writes, change detection | Done 2026-10-21 |
| M2 – Index and organization | SQLite/FTS5 index, tag folders with combination subfolders, full-text search | Done 2026-10-27 |
| M3 – Smart folders | Query language, saved searches | Done 2026-10-28 |
| M4 – Import | Apple Notes (macOS 26 Markdown export), Simplenote, HTML, Markdown folders | |
| M5 – Export | HTML, PDF, ODT built in; Pandoc formats on desktop | |
| M6 – Release 1.0 | Manual, packaging and signing, performance and data-safety tests | |
| Later | Note links, music rendering and playback (ABC), ChordPro, extras | |

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
