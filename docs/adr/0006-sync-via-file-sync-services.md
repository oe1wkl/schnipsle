# ADR-0006: Sync through file-sync services, no own sync in v1

- Status: Accepted
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: ADR-0002, ADR-0004, N-02

## Context

With macOS and iOS as musts, notes must reach both devices. Building a sync service (server, accounts, conflict resolution, encryption) is a project of its own and would add a server to an otherwise local-first app.

## Decision

- The app does **not** implement sync in v1. Because the vault is plain files (ADR-0004), existing file-sync services do it:
  - Apple devices: iCloud Drive (the app's iCloud container or any folder picked in Files).
  - Linux and Android, later: Syncthing, Nextcloud or similar on the same folder.
- The app is **sync-aware**:
  - Uses `NSFileCoordinator` / `NSFilePresenter` on Apple platforms and a file watcher elsewhere.
  - Detects conflicts (iCloud conflict versions, Syncthing `.sync-conflict-*` files, or a changed `modified` timestamp plus content hash since last read) and never overwrites silently.
  - Shows both versions side by side; the user picks one or merges; the other is kept in the version history (ADR-0011).
  - Handles not-yet-downloaded iCloud files (placeholders) by requesting download and showing a state.

## Consequences

- Positive: no server, no accounts, no data on our infrastructure; users choose their sync.
- Negative: sync quality depends on the service; real-time co-editing is impossible.
- Follow-up: a conflict test suite with simulated concurrent edits; documentation per sync service in the user manual.
