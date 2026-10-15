# Spike: vault folder access on iOS

- Branch: `spike/ios-vault`
- Started: 2026-10-14
- Decides: whether ADR-0003 (Tauri 2) holds for iOS, or a native Swift shell around the Rust core is needed
- Status: **concluded 2026-10-14: ADR-0003 confirmed for iOS** (C1–C6 passed on an iPhone with iOS 27; C7, C8 open, not decisive)

## Question

Can a Tauri 2 app on iOS let the user pick a folder in iCloud Drive, keep access to it across restarts, and read and safely write the Markdown notes in it, while the same folder is edited on a Mac?

## Findings before building

- Tauri's dialog plugin has **no folder picker on iOS or Android** (`FolderPickerNotImplemented`; its manifest says "Does not support folder picker"). It also does not keep security-scoped access for picked locations.
- Therefore the spike adds a small local plugin, `crates/tauri-plugin-vault-access`, with a Swift part (~130 lines) that:
  - shows `UIDocumentPickerViewController` for folders,
  - starts security-scoped access and stores a bookmark in `UserDefaults`,
  - restores the bookmark on the next launch (refreshing it if stale),
  - asks iCloud to download notes that exist only as placeholders (`.Name.md.icloud`).
- On desktop the app uses the dialog plugin and stores the folder path in the app's config folder.

## What was built

| Part                                                                                                                | Where                                       | Tested                                       |
| ------------------------------------------------------------------------------------------------------------------- | ------------------------------------------- | -------------------------------------------- |
| Vault access: list, read, atomic write with conflict check, create, iCloud placeholders                             | `crates/core/src/vault.rs`                  | 8 unit tests                                 |
| App commands: pick/restore vault, list, read, save, create, download                                                | `src-tauri/src/lib.rs`, `vault_location.rs` | desktop run (below)                          |
| iOS folder access plugin                                                                                            | `crates/tauri-plugin-vault-access`          | Rust side builds; **Swift not yet compiled** |
| Test UI: list, editor with autosave (1 s), change detection (poll every 3 s and on app activation), conflict banner | `ui/`                                       | desktop run                                  |

## Desktop result (Linux, 2026-10-14)

All passed with a test vault, driven automatically in a virtual display:

- [x] Last vault reopened on start; notes listed newest first, titles per format.md §5.2
- [x] iCloud placeholder shown with ☁ and not opened as text
- [x] Typing autosaves to disk after ~1 s
- [x] File changed externally while not editing → editor reloads ("Updated with changes from elsewhere")
- [x] File changed externally while editing → conflict banner, the other version is **not** overwritten
- [x] "Keep mine" writes the local version; no temporary files left in the folder

## Getting it to run on iOS 27 (2026-10-14)

Each step below stopped the first device build; all are fixed on this branch. They matter for anyone setting up the project.

| Symptom                                                                                                   | Cause                                                                             | Fix                                                                                                                                                    |
| --------------------------------------------------------------------------------------------------------- | --------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------ |
| Folder dialog missing on iOS                                                                              | Tauri's dialog plugin has no folder picker on mobile                              | Own plugin `tauri-plugin-vault-access`                                                                                                                 |
| Xcode: deployment target 14.0 not supported                                                               | Tauri's default iOS target is 14; Xcode 27 supports 15+                           | `bundle.iOS.minimumSystemVersion = "16.0"` (ADR-0002)                                                                                                  |
| Swift: `'v16' is unavailable`                                                                             | Plugin manifest used swift-tools-version 5.3                                      | swift-tools-version 5.9                                                                                                                                |
| Signing requires a development team                                                                       | Team not set; Xcode's setting is lost on regeneration                             | `APPLE_DEVELOPMENT_TEAM` environment variable                                                                                                          |
| "Build Rust Code": `pnpm: command not found`                                                              | Xcode runs build scripts without the shell's PATH                                 | Build phase sources `scripts/hook-env.sh` (in `project.yml` and `project.pbxproj`)                                                                     |
| Blank screen with `tauri ios dev` on the phone                                                            | Without `--host` the phone looks for the dev server on itself                     | `pnpm tauri ios dev --open --host`                                                                                                                     |
| `tauri ios run`: export fails (missing `.tmp…` plist)                                                     | Tauri CLI issue at the export step; the app itself was built                      | Install with `xcrun devicectl device install app`, or use the Xcode route                                                                              |
| App stops at launch (`EXC_BREAKPOINT` in `_UIApplicationEvaluateRuntimeIssueForNoSceneLifecycleAdoption`) | iOS 27 requires the UIScene life cycle; Tauri 2.11 (tao 0.35) does not provide it | Tauri 2.12.3 (tao 0.37.1) and `src-tauri/Info.ios.plist` with `UIApplicationSceneManifest` ([tao#1340](https://github.com/tauri-apps/tao/issues/1340)) |

Consequences:

- **Do not run `pnpm tauri ios init` again** without re-applying the PATH fix to the "Build Rust Code" phase; the regenerated `project.yml` would lose it. M1 moves this into a Tauri iOS template override so it survives.
- Minimum Rust version is 1.90 (required by Tauri 2.12).

## Test plan on the Mac and iPhone

Prerequisites: `scripts/bootstrap-macos.sh` has run; you are on the branch (`git switch spike/ios-vault`), then `pnpm install`.

### A. macOS desktop

```sh
pnpm tauri dev
```

1. "Open folder" → pick a test folder in iCloud Drive (e.g. `iCloud Drive/Schnipsle-Test`) with a few `.md` files.
2. Edit a note; check in Finder/TextEdit that the file changed.
3. Edit the same note in TextEdit while the app shows it (not editing) → the app reloads it within ~3 s.
4. Type in the app, and within a second save a change in TextEdit → conflict banner.
5. Quit and restart → the folder opens again without asking.

### B. iOS simulator

```sh
pnpm tauri ios dev           # pick an iPhone simulator from the list
```

The simulator's Files app has "On My iPhone" only; create a folder there with a note (Files → ⋯ → New Folder; drag a `.md` file from the Mac onto the simulator to add it). Repeat steps 1–5.

### C. Real iPhone with iCloud Drive (the actual question)

One-time setup: `APPLE_DEVELOPMENT_TEAM` in `~/.zshrc` and the phone in Developer Mode (see [dev-setup.md §6](../dev-setup.md#6-ios)).

```sh
pnpm tauri ios dev --open --host   # --host: the phone loads the UI from the Mac over Wi-Fi
```

In Xcode choose the phone as run destination and press ▶. Keep the Terminal command running while testing.

| #   | Test                                                             | Expected                                               | Result                                                                                                                                                  |
| --- | ---------------------------------------------------------------- | ------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------- |
| C1  | Open folder → pick the same iCloud Drive folder as on the Mac    | Notes listed                                           | pass (2026-10-14)                                                                                                                                       |
| C2  | Open and edit a note on the iPhone                               | Change appears on the Mac within seconds to a minute   | pass (2026-10-14)                                                                                                                                       |
| C3  | Edit a note on the Mac, keep it open on the iPhone (not editing) | iPhone reloads it after iCloud syncs                   | pass (2026-10-14)                                                                                                                                       |
| C4  | Note created on the Mac, not yet on the iPhone                   | Listed with ☁; tapping downloads it; opens on next tap | pass (2026-10-14)                                                                                                                                       |
| C5  | Force-quit the app and start again                               | Folder reopens without the picker                      | pass (2026-10-14)                                                                                                                                       |
| C6  | Restart the iPhone, start the app                                | Folder still reopens (bookmark survives)               | pass (2026-10-14)                                                                                                                                       |
| C7  | Edit the same note on both devices within a few seconds          | Conflict banner on one side; nothing lost              | fail (2026-10-15): nothing lost, but no banner; iCloud keeps the other side as an unresolved `NSFileVersion` conflict version the app does not read yet |
| C8  | Airplane mode, edit, then back online                            | Saved locally; syncs afterwards                        | pass (2026-10-15, M1 build with file events)                                                                                                            |

## Decision rule

- C1, C2, C5 and C6 pass → **ADR-0003 confirmed for iOS**; the plugin becomes part of M1, and the remaining issues go to the backlog.
- C1 or C5/C6 fail and cannot be fixed in the plugin → follow-up ADR: native Swift shell (SwiftUI) around the same Rust core via UniFFI.

## Known simplifications (to address in M1, not blockers for the spike)

- Changes are detected by polling every 3 s; M1 uses file events on desktop (`notify`) and `NSFilePresenter` on iOS.
- Writes do not yet use `NSFileCoordinator` (format.md §10 requires it on Apple platforms); the conflict check covers the common case.
- The hash check and the rename are not one atomic step; a change in that window of milliseconds is not detected.
- The bookmark lives in `UserDefaults`; M1 moves it to the app's Application Support folder next to other per-device state.
- One vault at a time; no front matter is added to notes yet.
