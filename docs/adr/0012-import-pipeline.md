# ADR-0012: Import through adapters and a common pipeline

- Status: Accepted; folder tags, placement, re-import and undo amended by [ADR-0019](0019-import-placement-reimport-undo.md)
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: F-03, F-04, F-05, F-13

## Context

Several sources must be imported now (Apple Notes, Simplenote, HTML, Markdown folders) and more later (Evernote, Google Keep). Each has its own quirks, but the steps after reading the source are the same.

## Decision

- Each source is an **adapter** implementing `Importer`, producing a stream of `ImportedNote { title, body_markdown, created, modified, tags, attachments, source_ref }`.
- A shared **pipeline** does: Markdown clean-up → attachment hashing and link rewriting → tag mapping → duplicate detection → **dry-run report** → atomic write → index → import log (enables undo of a whole import).
- Mapping decisions:
  - Apple Notes (macOS 26+ Markdown export): folder names become tags with a configurable prefix, default `#notes/<Folder>` (now `#<Folder>`, ADR-0019); dates kept where the export provides them. To be verified against a real export in M4. (The export has no IDs or dates; a metadata file from a script fills them in, ADR-0019.)
  - Simplenote: JSON tags become hashtags appended at the end of the note if not already in the body; `creationDate`/`lastModified` kept; trashed notes go to the trash.
  - HTML: converted to Markdown by the built-in converter (all platforms) or Pandoc when available.
- Tags added by import are appended on a final line, so the original text stays untouched.
- `source` and `source_id` in front matter record provenance and prevent duplicate imports on re-run.

## Consequences

- New formats need only an adapter plus golden-file fixtures in `tests/fixtures/import/<source>/`.
- Test exports from real sources (anonymized) are needed before M4.
