# ADR-0004: Plain Markdown files as the source of truth

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: N-01, N-02, N-04, [format.md](../format.md)

## Context

Notes are long-lived. Users must be able to read, back up, sync and migrate them without our app. Proprietary containers (as in Apple Notes) are exactly the lock-in we are helping people escape.

## Decision

- A **vault** is an ordinary folder. Each note is one UTF-8 `.md` file with a YAML front-matter block, as specified in [`docs/format.md`](../format.md).
- Images and other attachments are plain files, stored content-addressed by SHA-256.
- All app-specific state lives in `.notesapp/` inside the vault as human-readable files (YAML) or as derived caches.
- A note's identity is a ULID in its front matter, not its file name or path.
- Everything derived (index, tag tree, thumbnails) can be deleted and rebuilt from the files.

## Consequences

- Positive: no lock-in; any editor and any backup or sync tool works; Git export trivial.
- Negative: we must cope with external edits (file watcher, conflict detection) and cannot rely on database transactions across notes.
- Rules that follow: atomic writes (temp file + rename), file coordination on Apple platforms, a versioned format spec, migrations for format changes.
