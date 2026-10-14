# Spike: vault folder access on iOS

- Branch: `spike/ios-vault`
- Started: 2026-10-14
- Decides: whether ADR-0003 (Tauri 2) holds for iOS, or a native Swift shell around the Rust core is needed
- Status: **built and tested on desktop (Linux); waiting for macOS and iOS runs**

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

| Part | Where | Tested |
| --- | --- | --- |
| Vault access: list, read, atomic write with conflict check, create, iCloud placeholders | `crates/core/src/vault.rs` | 8 unit tests |
| App commands: pick/restore vault, list, read, save, create, download | `src-tauri/src/lib.rs`, `vault_location.rs` | desktop run (below) |
| iOS folder access plugin | `crates/tauri-plugin-vault-access` | Rust side builds; **Swift not yet compiled** |
| Test UI: list, editor with autosave (1 s), change detection (poll every 3 s and on app activation), conflict banner | `ui/` | desktop run |

## Desktop result (Linux, 2026-10-14)

All passed with a test vault, driven automatically in a virtual display:

- [x] Last vault reopened on start; notes listed newest first, titles per format.md §5.2
- [x] iCloud placeholder shown with ☁ and not opened as text
- [x] Typing autosaves to disk after ~1 s
- [x] File changed externally while not editing → editor reloads ("Updated with changes from elsewhere")
- [x] File changed externally while editing → conflict banner, the other version is **not** overwritten
- [x] "Keep mine" writes the local version; no temporary files left in the folder

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
pnpm tauri ios init          # once; creates src-tauri/gen/apple (commit it afterwards)
pnpm tauri ios dev           # pick an iPhone simulator when asked
```

The simulator's Files app has "On My iPhone" only; create a folder there with a note (Files → ⋯ → New Folder; drag a `.md` file from the Mac onto the simulator to add it). Repeat steps 1–5.

### C. Real iPhone with iCloud Drive (the actual question)

```sh
pnpm tauri ios dev --open    # opens Xcode
```

In Xcode: target `schnipsle_iOS` → Signing & Capabilities → Team: your Apple ID; select your iPhone as the run destination and press Run. On the phone, enable Settings → Privacy & Security → Developer Mode if asked, and trust the developer certificate under Settings → General → VPN & Device Management.

| # | Test | Expected | Result |
| --- | --- | --- | --- |
| C1 | Open folder → pick the same iCloud Drive folder as on the Mac | Notes listed | |
| C2 | Open and edit a note on the iPhone | Change appears on the Mac within seconds to a minute | |
| C3 | Edit a note on the Mac, keep it open on the iPhone (not editing) | iPhone reloads it after iCloud syncs | |
| C4 | Note created on the Mac, not yet on the iPhone | Listed with ☁; tapping downloads it; opens on next tap | |
| C5 | Force-quit the app and start again | Folder reopens without the picker | |
| C6 | Restart the iPhone, start the app | Folder still reopens (bookmark survives) | |
| C7 | Edit the same note on both devices within a few seconds | Conflict banner on one side; nothing lost | |
| C8 | Airplane mode, edit, then back online | Saved locally; syncs afterwards | |

## Decision rule

- C1, C2, C5 and C6 pass → **ADR-0003 confirmed for iOS**; the plugin becomes part of M1, and the remaining issues go to the backlog.
- C1 or C5/C6 fail and cannot be fixed in the plugin → follow-up ADR: native Swift shell (SwiftUI) around the same Rust core via UniFFI.

## Known simplifications (to address in M1, not blockers for the spike)

- Changes are detected by polling every 3 s; M1 uses file events on desktop (`notify`) and `NSFilePresenter` on iOS.
- Writes do not yet use `NSFileCoordinator` (format.md §10 requires it on Apple platforms); the conflict check covers the common case.
- The hash check and the rename are not one atomic step; a change in that window of milliseconds is not detected.
- The bookmark lives in `UserDefaults`; M1 moves it to the app's Application Support folder next to other per-device state.
- One vault at a time; no front matter is added to notes yet.
