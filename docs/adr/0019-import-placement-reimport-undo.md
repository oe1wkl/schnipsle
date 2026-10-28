# ADR-0019: Where imported notes go, re-import and undo

- Status: Accepted
- Date: 2026-10-28
- Deciders: Willi Kraml
- Related: F-03, F-04, F-05, ADR-0005, ADR-0008, ADR-0011, ADR-0012 (amended), [format.md §1](../format.md#1-vault)

## Context

ADR-0012 set the import pipeline but left open where imported notes are stored, what a second import of the same source does, how far "undo import" goes and which platforms import. It also chose `#notes/<Folder>` as the tags for source folders. Planning M4 settled these points.

A real Apple Notes export (macOS 27, 137 notes in 3 folders) showed what the Markdown export leaves out: it has no note IDs and no dates (every file carries the export time), it shortens long titles in file names with `…`, and of several notes with the same title in one folder it writes only one. Smart folders look like ordinary folders. Notes' scripting interface has what is missing: per note `id`, `creation date`, `modification date`, its folder and its text, but it cannot save attachments, which the Markdown export does.

## Decision

- **Placement:** imported notes go to `notes/<Source>/`, followed by the source's own folders, e.g. `notes/Apple Notes/Radio/Tuner.md`. Names follow format.md §3, and a name that is taken gets `-2`, `-3`, ….
- **Folder tags** (amends ADR-0012): a source folder becomes the tag `#<Folder>` directly, without the `notes/` prefix, so it merges with tags the user already has. Nested folders become hierarchy levels (`#Radio/QRP`). Characters a tag cannot hold (format.md §5.3), such as spaces, become `-`; runs of `-` collapse, and a `-` at either end is dropped. A folder name without a letter gives no tag. As in ADR-0012, tags added by an import go on a final line, and the original text stays as it was.
- **Re-import:** a note with the same `source` and `source_id` as an earlier import is replaced only if the source has a newer version **and** the note is unchanged since that import (its hash equals the one in the import log). Its previous version goes into the history first (ADR-0011). Otherwise it is left alone and listed in the report.
- **Undo** of an import run, using its log:
  - imported notes that are unchanged since then go to the trash, so they can be restored with their history;
  - notes edited since then are kept and listed;
  - notes the run replaced get their previous version back from the history.
- **Apple Notes = Markdown export plus a metadata file.** `scripts/apple-notes-metadata.js` (JavaScript for Automation, reads Notes only) writes `apple-notes-metadata.json` into the export folder: `format` (1), `exported`, and per note `id`, `account`, `folder` (path from the top level), `smartFolders`, `name`, `created`, `modified` (ISO 8601, UTC), `locked`, `shared` and `snippet` (the first 200 characters of its text, white space collapsed). Notes shown by a smart folder are listed once, in their real folder.
  - Matching: an exported file belongs to the note whose `name` equals its first line, or its file name, after removing Markdown marks and the characters a file name drops (`:`, `/`), ignoring case and runs of white space; a file name ending in `…` matches the start of the name. If several notes match, the one whose snippet continues like the file's text wins.
  - A matched note gets `source_id` = the note's `id` and its `created`/`modified`. A file without a clear match keeps the export time, gets its path in the export as `source_id` and is listed in the report; so is a note the metadata names twice in one folder (the export holds only one of them).
  - Placement and folder tags follow the folders of the export, which the user names when exporting; the metadata's folders only help matching.
  - Without the metadata file the import still runs, as for unmatched files, and the report says what is missing.
- **Desktop only in M4.** The Apple Notes export exists only on the Mac anyway, and the iPhone gets imported notes through sync.
- **HTML** is converted by a built-in, permissively licensed Rust converter on every platform. Pandoc (ADR-0005, external program only) may follow later.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| `notes/<Source>/` plus source folders (chosen) | An import stays together; the source's structure is visible in Finder | Folders and tags say the same thing twice |
| Flat in `notes/` | Organization only through tags, as intended | Many name collisions, an import hard to tell apart in Finder |
| One folder per import run | Clear what came in when | Source folders survive only as tags |
| `#<Folder>` (chosen) | Merges with existing tags | Can mix imported notes into tags the user meant otherwise |
| `#notes/<Folder>` (ADR-0012) | Imported folders grouped under one tag | An extra level in every tag |
| Re-import updates unchanged notes (chosen) | A second export brings changes in; nothing edited here is lost | Needs the hash per note in the import log |
| Re-import always skips | Simplest | Later changes in the source never arrive |
| Apple Notes: Markdown export plus metadata file (chosen) | Dates and stable IDs back; attachments from the export | A second step when exporting; matching by title can fail |
| Apple Notes: Markdown export only | One step | All notes dated by the export; re-import only by file path |
| Apple Notes: script only (HTML text) | One step, IDs and dates | No attachment files; HTML to convert |

## Consequences

- Positive: imports can be repeated as the source changes, without duplicates and without overwriting work done in Schnipsle. Undo never removes text for good.
- Negative: the import log becomes part of the vault format: it records `source`, `source_id`, path and hash per note, so its format must be specified in format.md. A folder tag can clash with an existing tag of the same name.
- Follow-up work: format.md gets an import log section (M4, package 1), with fixtures. HLD §6 and ADR-0012 point to this ADR. The Apple Notes adapter reads `apple-notes-metadata.json`; the import UI explains the two export steps.
