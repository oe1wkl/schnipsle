# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Repository foundation: Cargo workspace (`schnipsle-core`, `-import`, `-export`, Tauri 2 shell), TypeScript UI, CI, dependency licence checks, macOS bootstrap script.
- Architecture decision records ADR-0001 to ADR-0015.
- Vault format specification, draft revision 0.6 (notes, front matter, tags, links, attachments, music in ABC, song sheets in ChordPro, history, trash, smart folders).
- Hashtag extraction in `schnipsle-core` following format.md §5.3, with the specification examples as tests.
- Vault access in `schnipsle-core`: list notes (with iCloud placeholders), read, atomic write with conflict detection, create (spike, see docs/spikes/ios-vault.md).
- `tauri-plugin-vault-access`: iOS folder picker with security-scoped bookmarks and iCloud download requests (spike).
- File coordination on macOS and iOS (ADR-0015): notes are read and saved through `NSFileCoordinator`, so iCloud Drive and other apps cannot change a note between the conflict check and the save. New crate `schnipsle-apple`; `schnipsle-core` gains a `Coordinator` trait.
- Changes made elsewhere now arrive as file events instead of being polled every 3 s: `notify` on desktop, an `NSFilePresenter` on macOS and iOS. The UI gets one `vault-changed` event per burst of changes.
- iCloud sync conflicts are shown: when two devices changed a note before syncing, a banner offers to keep this version or use the other one. The version not kept is stored in the note's history (`.notesapp/history/<id>/`, format.md §7) before iCloud is told the conflict is resolved, and the note gets `id`, `created` and `modified` in its front matter.
- Every save keeps the note's front matter up to date (format.md §4): `id` and `created` are added the first time the app writes a note, `modified` on each change. Keys added by other tools are preserved, and a note is never rewritten just by opening it.
- Copies of a note with the same `id` (format.md §4), such as the `Name 2.md` copies iCloud Drive makes in a sync conflict, are shown in the conflict banner: keep this version or the copy's (the other text goes into the note's history and the copy is removed), or keep both as separate notes, in which case the later one gets a new `id`. For now only copies in the same folder are found.
- New note editor (CodeMirror 6, ADR-0007) with inline live preview: headings, emphasis, strikethrough, inline code and links are shown formatted, their Markdown markers appear only on the line being edited, and task boxes can be ticked. A button switches to plain Markdown source; the choice is remembered. The front matter is shown muted, and front matter written on save no longer moves the cursor or enters the undo history.
- Note list: sort by recently changed or by title (ignoring case and accents). New notes are named after their title, and when you leave a note whose title changed, its file is renamed to match (format.md §3); renames never replace another file.
- Trash (format.md §8): Delete moves a note unchanged to `.notesapp/trash/`, with Undo; a Trash view lists deleted notes and restores them. Notes without `id` and repeated ids get their own trash names (format.md 0.5).
- Vaults follow the layout of format.md §1: a vault is a folder with `.notesapp/vault.yaml`, and its notes live in `notes/`. Picking a folder without `vault.yaml` asks first and then moves everything visible in it into `notes/` (relative links keep working; hidden files such as `.notesapp/` stay), so existing note folders, including the M1 test vault, are converted only with consent. A vault written by a newer format version, or with a damaged `vault.yaml`, opens read-only with an explanation. Picking a folder inside a vault is refused.
- Test UI: open a vault folder, edit notes with autosave, reload external changes, resolve conflicts (spike).

### Changed

- Trash records store `original_path` relative to the vault root (`notes/…`, format.md 0.6); records written by earlier M1 builds are restored into `notes/` under their file name.
- Safer saving: the conflict check now runs after the new text is flushed to disk, right before the rename, and creating a note can no longer replace one created at the same moment by another device or process.
