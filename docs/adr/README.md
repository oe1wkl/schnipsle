# Architecture Decision Records

This folder records every significant architectural decision of the project, one file per decision, in the [MADR](https://adr.github.io/madr/) style.

## Rules

- File name: `NNNN-short-title.md`, numbered consecutively, never renumbered.
- An accepted ADR is not edited in substance. A changed decision gets a **new** ADR that supersedes the old one; the old one is marked `Superseded by ADR-NNNN`.
- Status values: `Proposed`, `Accepted`, `Deprecated`, `Superseded by ADR-NNNN`.
- Every pull request that changes the file format, the tech stack or a public interface references an ADR.
- New ADRs start from [`template.md`](template.md).

## Index

| ADR | Title | Status | Date |
| --- | --- | --- | --- |
| [0001](0001-record-architecture-decisions.md) | Record architecture decisions | Accepted | 2026-10-13 |
| [0002](0002-target-platforms.md) | Target platforms and priorities | Accepted | 2026-10-13 |
| [0003](0003-application-stack-tauri.md) | Application stack: Tauri 2, Rust core, TypeScript UI | Accepted | 2026-10-13 |
| [0004](0004-plain-markdown-files-as-source-of-truth.md) | Plain Markdown files as the source of truth | Accepted | 2026-10-13 |
| [0005](0005-license-apache-2.md) | License: Apache-2.0 | Accepted | 2026-10-13 |
| [0006](0006-sync-via-file-sync-services.md) | Sync through file-sync services, no own sync in v1 | Accepted | 2026-10-13 |
| [0007](0007-editing-mode-inline-live-preview.md) | Editing mode: inline live preview with source toggle | Accepted | 2026-10-13 |
| [0008](0008-tag-syntax-and-virtual-folders.md) | Tag syntax, tag hierarchy and combination folders | Accepted | 2026-10-13 |
| [0009](0009-search-index-sqlite-fts5.md) | Search index: SQLite FTS5 as a rebuildable cache | Accepted | 2026-10-13 |
| [0010](0010-export-engines.md) | Export engines: Pandoc on desktop, built-in on iOS | Accepted | 2026-10-13 |
| [0011](0011-version-history-and-trash.md) | Version history and trash inside the vault | Accepted | 2026-10-13 |
| [0012](0012-import-pipeline.md) | Import through adapters and a common pipeline | Accepted | 2026-10-13 |
| [0013](0013-music-notation-abc.md) | Music notation as ABC in fenced code blocks | Accepted | 2026-10-14 |
| [0014](0014-lyrics-with-chords-chordpro.md) | Lyrics with chords as ChordPro in fenced code blocks | Accepted | 2026-10-14 |
| [0015](0015-apple-file-coordination-objc2.md) | Apple file coordination in a Rust crate using objc2 | Accepted | 2026-10-15 |
| [0016](0016-rewrite-relative-links-when-notes-move.md) | Rewrite relative links when a note moves to another folder | Accepted | 2026-10-17 |
| [0017](0017-history-retention-settings.md) | History retention settings in `settings.yaml` | Accepted | 2026-10-21 |
| [0018](0018-smart-folders-file.md) | Reading and writing `smart-folders.yaml` | Accepted | 2026-10-28 |
| [0019](0019-import-placement-reimport-undo.md) | Where imported notes go, re-import and undo | Accepted | 2026-10-28 |
| [0020](0020-imported-notes-show-their-title.md) | Imported notes show the title they are named by | Accepted | 2026-10-31 |
| [0021](0021-what-an-exported-note-looks-like.md) | What an exported note looks like | Accepted | 2026-11-01 |
| [0022](0022-pdf-through-the-print-path.md) | PDF through the print path on macOS | Accepted | 2026-11-02 |
| [0023](0023-links-between-pdf-files.md) | Links between separate PDF files become text | Accepted | 2026-11-03 |
| [0024](0024-pdf-dates-in-the-file.md) | PDF dates are corrected in the file | Accepted | 2026-11-04 |
| [0025](0025-docx-and-epub-through-pandoc.md) | DOCX and EPUB through Pandoc | Accepted | 2026-11-05 |
| [0026](0026-a-heading-of-any-level-gives-the-title.md) | A first heading of any level gives the title | Accepted | 2026-11-05 |
| [0027](0027-pictures-converted-for-an-export.md) | Pictures few programs show are converted for an export | Accepted | 2026-11-05 |
| [0028](0028-turned-pictures-upright-in-docx-and-odt.md) | Turned pictures are upright in DOCX and ODT | Accepted | 2026-11-06 |
| [0029](0029-table-of-contents-and-page-numbers.md) | Table of contents and page numbers | Accepted | 2026-11-06 |
| [0030](0030-rendering-and-transposing-song-sheets.md) | Rendering and transposing song sheets | Accepted | 2026-11-07 |
| [0031](0031-song-sheets-in-the-editor.md) | Song sheets in the editor | Accepted | 2026-11-08 |
| [0032](0032-scores-in-the-editor.md) | Scores in the editor | Accepted | 2026-11-08 |
| [0033](0033-scores-in-exports.md) | Scores in exports, engraved by the app's web view | Accepted | 2026-11-09 |
| [0034](0034-song-sheets-in-exports.md) | Song sheets and ABC sources in exports | Accepted | 2026-11-09 |
| [0035](0035-playing-scores.md) | Playing scores | Accepted | 2026-11-10 |
| [0036](0036-installing-and-updating.md) | Installing and updating | Accepted | 2026-11-13 |
| [0037](0037-pdf-on-ios.md) | PDF on iOS, with the links put back | Accepted | 2026-11-14 |
| [0038](0038-comparing-versions.md) | Comparing versions in the history | Accepted | 2026-11-17 |
| [0039](0039-settings-what-is-set-where.md) | Settings: what is set where | Accepted | 2026-11-17 |
| [0040](0040-renaming-and-merging-tags.md) | Renaming and merging tags | Accepted | 2026-11-18 |
| [0041](0041-ui-strings-and-access.md) | UI strings in one place, and access by keyboard and VoiceOver | Accepted | 2026-11-18 |
| [0042](0042-nothing-dropped-when-text-is-replaced.md) | Nothing dropped when a version is replaced or a note is left | Accepted | 2026-11-19 |
| [0043](0043-tags-added-to-every-imported-note.md) | Tags added to every note of an import | Accepted | 2026-11-19 |
| [0044](0044-a-note-keeps-its-id-when-another-program-drops-it.md) | A note keeps its `id` when another program drops it | Accepted | 2026-11-21 |
| [0045](0045-measuring-performance-on-devices.md) | Measuring performance on devices | Accepted | 2026-11-23 |
| [0046](0046-unchanged-files-are-not-read-again.md) | Unchanged files are not read again to list notes or find copies | Accepted | 2026-11-23 |
| [0047](0047-folders-are-listed-in-bulk-on-apple-platforms.md) | Folders are listed in bulk on Apple platforms | Accepted | 2026-11-24 |
| [0048](0048-a-default-vault-with-the-manual-as-notes.md) | A default vault, with the manual as notes | Accepted | 2026-11-25 |
| [0049](0049-book-notes.md) | Book notes: a title page and an order for exported notes | Accepted | 2026-11-25 |
| [0050](0050-the-default-vault-on-a-second-device-and-on-the-mac.md) | The default vault on a second device and on the Mac | Accepted | 2026-11-26 |
| [0051](0051-the-manual-links-its-notes-by-relative-links.md) | The manual links its notes by relative links, and is built into the app | Accepted | 2026-11-27 |
