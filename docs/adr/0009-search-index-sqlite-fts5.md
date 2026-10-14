# ADR-0009: Search index: SQLite FTS5 as a rebuildable cache

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: F-09, F-10, N-03, ADR-0004

## Context

Search, tag folders and smart folders need answers in under 200 ms for at least 10,000 notes, on an iPhone as well as a Mac. The data must stay in plain files (ADR-0004).

## Decision

- An embedded **SQLite** database with the **FTS5** extension, accessed through `rusqlite` with the bundled SQLite build (same version on all platforms).
- Tables: notes (id, path, title, created, modified, hash, state), FTS5 over title and body text (Markdown stripped, `unicode61` tokenizer with diacritics removal), note–tag, note–link, note–attachment.
- The database is a **cache**: stored per device, **outside** the synced vault (app support/caches directory), keyed by vault id. It is never synced, because SQLite files in sync folders get corrupted.
- On start the core compares file hashes and modification times and updates only what changed; a full rebuild is always possible.
- The query language (HLD, "Smart folders") is parsed into an AST and compiled to parameterized SQL.

## Consequences

- Correction to the HLD vault layout: `index.sqlite` does **not** live in `.notesapp/` but in the per-device cache directory.
- Performance test with 20,000 generated notes in CI (N-03).
- Words in other languages: `unicode61` handles German umlauts; stemming is not used in v1.
