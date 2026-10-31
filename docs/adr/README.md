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
