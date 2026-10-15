# Roadmap and status

- Updated: 2026-10-15
- Current milestone: **M1 – Core editor** (M0 complete and merged)

Each milestone ends in a usable build. Scope can move between milestones; the order should not.

| Milestone | Content | Status |
| --- | --- | --- |
| M0 – Foundations | Decisions (ADR-0001…0014), format spec, repository and CI, iOS feasibility spike | Done 2026-10-14 |
| M1 – Core editor | Open a vault, CodeMirror editor with live preview, images, autosave, safe writes, change detection | In progress |
| M2 – Index and organization | SQLite/FTS5 index, tag folders with combination subfolders, full-text search | |
| M3 – Smart folders | Query language, saved searches | |
| M4 – Import | Apple Notes (macOS 26 Markdown export), Simplenote, HTML, Markdown folders | |
| M5 – Export | HTML, PDF, ODT built in; Pandoc formats on desktop | |
| M6 – Release 1.0 | Manual, packaging and signing, performance and data-safety tests | |
| Later | Note links, music rendering and playback (ABC), ChordPro, extras | |

## M0 results

- Tauri 2 confirmed for iOS by the spike (`docs/spikes/ios-vault.md`); ADR-0003 unconditional.
- Working: folder picker and persistent access on iOS, list/read/write with conflict detection, test UI on macOS, Linux and iPhone.
- C8 (offline editing) passed on 2026-10-15 with the M1 build. C7 (simultaneous edits on two devices) failed on 2026-10-15: nothing is lost, but iCloud keeps the other side as a conflict version that the app does not show yet.

## M1 backlog

Carried over from the spike:

- [x] Replace polling with file events: `notify` on desktop, `NSFilePresenter` on macOS and iOS (ADR-0015)
- [x] Use `NSFileCoordinator` for reads and writes on Apple platforms (format.md §10, ADR-0015: `schnipsle-apple`)
- [x] Make check-and-rename atomic enough: hash re-checked after the temp file is flushed, creates never replace (file coordination still open above)
- [ ] Move the iOS bookmark from `UserDefaults` to Application Support
- [ ] Tauri iOS template override so the "Build Rust Code" PATH fix survives `ios init`
- [ ] Own menu bar instead of Tauri's default (duplicate Close Window in File and Window menus)
- [ ] Detect and show iCloud conflict versions (`NSFileVersion`, ADR-0006), then re-test C7 (C8 passed 2026-10-15)

Editor:

- [ ] CodeMirror 6 with inline live preview and a source-mode toggle (ADR-0007)
- [ ] Note list with titles per format.md §5.2, sorting, new/rename/delete (delete → trash, format.md §8)
- [ ] Front matter (`id`, `created`, `modified`) written on first save (format.md §4)
- [ ] Images: paste/drag-drop/picker → `attachments/` by SHA-256 (format.md §6), relative links
- [ ] Autosave with history snapshots (format.md §7, ADR-0011)
- [ ] Phone layout: list and editor as separate views

Housekeeping:

- [x] Merge `spike/ios-vault` into `main` via pull request (first PR run of the commit-message check)
- [x] Merge the Dependabot pull requests for GitHub Actions
