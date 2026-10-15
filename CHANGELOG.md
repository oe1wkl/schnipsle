# Changelog

All notable changes to this project are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses [Semantic Versioning](https://semver.org/).

## [Unreleased]

### Added

- Repository foundation: Cargo workspace (`schnipsle-core`, `-import`, `-export`, Tauri 2 shell), TypeScript UI, CI, dependency licence checks, macOS bootstrap script.
- Architecture decision records ADR-0001 to ADR-0015.
- Vault format specification, draft revision 0.4 (notes, front matter, tags, links, attachments, music in ABC, song sheets in ChordPro, history, trash, smart folders).
- Hashtag extraction in `schnipsle-core` following format.md §5.3, with the specification examples as tests.
- Vault access in `schnipsle-core`: list notes (with iCloud placeholders), read, atomic write with conflict detection, create (spike, see docs/spikes/ios-vault.md).
- `tauri-plugin-vault-access`: iOS folder picker with security-scoped bookmarks and iCloud download requests (spike).
- File coordination on macOS and iOS (ADR-0015): notes are read and saved through `NSFileCoordinator`, so iCloud Drive and other apps cannot change a note between the conflict check and the save. New crate `schnipsle-apple`; `schnipsle-core` gains a `Coordinator` trait.
- Changes made elsewhere now arrive as file events instead of being polled every 3 s: `notify` on desktop, an `NSFilePresenter` on macOS and iOS. The UI gets one `vault-changed` event per burst of changes.
- iCloud sync conflicts are shown: when two devices changed a note before syncing, a banner offers to keep this version or use the other one. The version not kept is stored in the note's history (`.notesapp/history/<id>/`, format.md §7) before iCloud is told the conflict is resolved, and the note gets `id`, `created` and `modified` in its front matter.
- Test UI: open a vault folder, edit notes with autosave, reload external changes, resolve conflicts (spike).

### Changed

- Safer saving: the conflict check now runs after the new text is flushed to disk, right before the rename, and creating a note can no longer replace one created at the same moment by another device or process.
