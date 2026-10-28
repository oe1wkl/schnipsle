# Schnipsle – High-Level Design

- Snapshot: end of M0, 2026-10-14
- Living version: see [README.md](README.md)
- Where this document and an ADR disagree, the ADR wins.

## 1. Purpose and scope

A local-first note editor that stores every note as a plain Markdown file and organizes notes through hashtags and saved searches instead of physical folders. Notes come in from Apple Notes, Simplenote and HTML, and go out as HTML, PDF and OpenDocument.

Goals:

- Markdown as the single source of truth: notes stay usable without the app.
- Painless migration from Apple Notes and Simplenote, including images.
- Organization by hashtags and saved searches, shown as virtual folders.
- Nothing is lost: version history and a searchable trash.
- Music in notes: scores (ABC) and song sheets with chords (ChordPro).

Non-goals for version 1: real-time collaboration, a hosted sync service, WYSIWYG-only editing that hides the Markdown.

| Platform           | Priority     |
| ------------------ | ------------ |
| macOS              | Must         |
| iOS / iPadOS (16+) | Must         |
| Linux              | Desirable    |
| Android            | Desirable    |
| Windows            | Nice to have |

Open source on GitHub, Apache-2.0 (ADR-0002, ADR-0005).

## 2. Requirements

Priority: M = must (v1), S = should, C = could (later).

| ID   | Requirement                                                                                                     | Prio |
| ---- | --------------------------------------------------------------------------------------------------------------- | ---- |
| F-01 | Edit notes in Markdown (CommonMark + GFM) with syntax highlighting and live preview                             | M    |
| F-02 | Insert images by paste, drag-and-drop or picker; store them as files                                            | M    |
| F-03 | Import Apple Notes, including images                                                                            | M    |
| F-04 | Import Simplenote export, including tags                                                                        | M    |
| F-05 | Import HTML files and plain Markdown folders                                                                    | S    |
| F-06 | Export a note or a selection to HTML, PDF and ODT                                                               | M    |
| F-07 | Hashtags create virtual folders; a note with several tags appears in several folders                            | M    |
| F-08 | Combinations of tags appear as virtual subfolders                                                               | M    |
| F-09 | Full-text search with tag, date and attribute filters                                                           | M    |
| F-10 | Save a search as a smart folder                                                                                 | M    |
| F-11 | Rename or merge a tag across all notes                                                                          | S    |
| F-12 | Links between notes and a backlinks panel                                                                       | S    |
| F-13 | DOCX/EPUB export; Evernote, Google Keep, Obsidian/Bear import                                                   | C    |
| F-14 | Version history per note: browse, compare, restore                                                              | M    |
| F-15 | Deleted notes go to a trash, stay searchable, restorable with history                                           | M    |
| F-16 | Music notation in ABC, rendered in editor and exports; titles and lyrics searchable                             | M    |
| F-17 | Playback of ABC music with tempo and per-voice mute/solo                                                        | S    |
| F-18 | Song sheets in ChordPro: chords over lyrics, transposition (English and German names), lyrics-only view, search | M    |

| ID   | Non-functional requirement                                                   |
| ---- | ---------------------------------------------------------------------------- |
| N-01 | Data is plain files readable without the app                                 |
| N-02 | Works fully offline; no account required                                     |
| N-03 | Search under 200 ms for 10,000 notes (tested with 20,000)                    |
| N-04 | No data loss: atomic writes, autosave, trash, history                        |
| N-05 | macOS and iOS; Linux and Android possible without redesign; Windows optional |
| N-06 | UI localizable (English, German first) and keyboard-accessible               |
| N-07 | Open-source licence and reproducible builds                                  |

## 3. Data model and storage

A vault is an ordinary folder chosen by the user; each note is one `.md` file with YAML front matter. Everything derived is a rebuildable cache. Full specification: [format.md](../format.md); decision: ADR-0004.

```text
<vault>/
  notes/                  note files (subfolders allowed)
  attachments/ab/<sha256>.<ext>
  .notesapp/
    vault.yaml  settings.yaml  smart-folders.yaml
    history/<note-id>/<timestamp>.md.zst
    trash/<note-id>.md
    import-log/
# per device, outside the vault: index.sqlite (ADR-0009)
```

- A folder without `.notesapp/vault.yaml` is not a vault. When the user picks one, the app asks first, then moves everything visible in it into `notes/` (so relative links keep working) and writes `vault.yaml` last; hidden files stay, nothing is replaced, and an interrupted move is offered again. A vault of a newer format, or with a damaged `vault.yaml`, opens read-only (format.md §2). Notes that turn up outside `notes/` later (e.g. from an older build) are not listed; the app offers to move them in.
- Identity is a ULID in front matter; file names are cosmetic.
- Tags are inline only (format.md §5.3, ADR-0008); `#a/b` is hierarchical.
- Writes are atomic and refuse to overwrite a note changed elsewhere (format.md §10).

## 4. Architecture

Three layers: a thin UI, a Rust core that owns all file access and logic, and storage where the Markdown files are authoritative (ADR-0003).

```text
UI (TypeScript, CodeMirror 6, WebView)
   │  Tauri commands and events
App shell (src-tauri, Tauri 2)  ── platform glue: dialog plugin (desktop),
   │                                tauri-plugin-vault-access (iOS, Swift),
   │                                schnipsle-apple (file coordination and presenter, ADR-0015)
Core (Rust crates: core, import, export)
   │
Vault on disk (source of truth)      Index cache (SQLite FTS5, per device)
```

The UI never touches files. Platform specifics (folder access, file coordination, export engines) sit behind Rust traits or app commands; file coordination, for example, is the core's `Coordinator` trait, implemented for Apple platforms in `schnipsle-apple`.

## 5. Technology

| Concern           | Choice                                                               | Decision           |
| ----------------- | -------------------------------------------------------------------- | ------------------ |
| App shell         | Tauri ≥ 2.12 (macOS, iOS; Linux/Android later)                       | ADR-0003           |
| Core              | Rust, edition 2024, MSRV 1.90                                        | ADR-0003           |
| Editor            | CodeMirror 6, inline live preview + source toggle                    | ADR-0007           |
| Markdown parser   | comrak (CommonMark + GFM)                                            | ADR-0003           |
| Search            | SQLite FTS5 via rusqlite, per-device cache                           | ADR-0009           |
| Export            | Built-in HTML/PDF/ODT on all platforms; Pandoc optional on desktop   | ADR-0010           |
| Music             | ABC via abcjs (MIT); ChordPro with own parser                        | ADR-0013, ADR-0014 |
| iOS folder access | Own plugin: folder picker, security-scoped bookmark, iCloud download | spike, ADR-0003    |

## 6. Import and export

Importers are adapters producing one intermediate form; a shared pipeline cleans up, hashes attachments, maps tags, detects duplicates, shows a dry-run report, writes atomically and keeps an undo log (ADR-0012). Apple Notes uses the built-in Markdown export of macOS 26. Imported notes go to `notes/<Source>/` with the source's folders, which also become tags (`#<Folder>`); a re-import updates only notes unchanged since, and undo trashes only unedited ones (ADR-0019). Import runs on the desktop.

Export: one `Exporter` interface; built-in HTML (with CSS theme), PDF (via the system WebView) and ODT (own writer) everywhere; Pandoc adds DOCX/EPUB on desktop when installed (ADR-0010).

## 7. Organization: virtual folders

- **Tag folders:** each tag is a folder; inside tag A, each co-occurring tag B is a subfolder A › B (notes with both). Paths are intersections; depth limited (default 3). Hierarchical tags `#a/b` nest additionally (ADR-0008).
- **Smart folders:** named, saved queries; nested for grouping.
- **Query language (draft):** words (prefix match), `"phrase"`, space = AND, `OR`, `-term`, `#tag`/`tag:x`, `created:`/`modified:` with dates and ranges, `has:image|task|link|chord`, `is:pinned|untagged`, `in:trash`, parentheses. Parsed to an AST and compiled to SQL; tag folders use the same engine.

## 8. Version history and trash

Snapshots on typing pause, close, delete, import and external change; retention all for 7 days, daily for 3 months, weekly after; trash never emptied automatically. Both live inside the vault so they sync (ADR-0011, format.md §7–8).

## 9. Music

- ABC in ` ```abc ` blocks, rendered as score, playable, titles/lyrics indexed (format.md §5.6, ADR-0013).
- ChordPro in ` ```chordpro ` blocks: chords over syllables, transposition, German note names option, lyrics-only view, chord filter (format.md §5.7, ADR-0014).

## 10. Development

Development environment, conventions and CI: [dev-setup.md](../dev-setup.md), [CONTRIBUTING.md](../../CONTRIBUTING.md). Decisions: [adr/](../adr/README.md). Plan and status: [roadmap.md](../roadmap.md).

## 11. Decisions resolved during M0

| Topic      | Decision                                                                                      |
| ---------- | --------------------------------------------------------------------------------------------- |
| Platforms  | macOS, iOS must; Linux, Android desirable; Windows nice to have                               |
| Licence    | Apache-2.0, DCO sign-off                                                                      |
| Sync       | None of its own; iCloud Drive / Syncthing / Nextcloud on the vault folder; conflicts detected |
| Editing    | Inline live preview by default, source toggle                                                 |
| Tags       | Inline only; both combination and hierarchical folders                                        |
| Volume     | Designed for at least 10,000 notes                                                            |
| Versioning | Per-note history and searchable trash are must-haves                                          |
| Music      | ABC for scores, ChordPro for song sheets                                                      |
| iOS        | Minimum iOS 16; Tauri ≥ 2.12 required for iOS 27                                              |
