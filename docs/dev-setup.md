# Development setup (macOS)

Everything needed to build Schnipsle for macOS and iOS on an Apple-silicon or Intel Mac. Linux and Android notes follow at the end.

## 1. Before you start

- macOS with at least 40 GB free disk space (Xcode and the iOS simulator are large).
- An Apple ID. A free account is enough to run the app on your own iPhone; publishing on the App Store needs the paid Apple Developer Program.
- Keep the source tree **outside iCloud Drive** (e.g. `~/Developer/schnipsle`). iCloud syncing build folders and `.git` causes corrupted builds. Test vaults may live in iCloud.

## 2. Install Xcode (manual, once)

1. Install **Xcode** from the App Store (not only the Command Line Tools: iOS builds need full Xcode).
2. Open Xcode once, accept the licence, and install the **iOS** platform when asked (Xcode → Settings → Components).
3. In Terminal:

   ```sh
   sudo xcode-select -s /Applications/Xcode.app/Contents/Developer
   ```

## 3. Get the code

```sh
mkdir -p ~/Developer && cd ~/Developer
git clone https://github.com/oe1wkl/schnipsle.git
cd schnipsle
```

## 4. Run the bootstrap script

```sh
scripts/bootstrap-macos.sh
```

It installs Homebrew (if missing) and everything in [`Brewfile`](../Brewfile), the pinned Rust toolchain with the Apple targets from [`rust-toolchain.toml`](../rust-toolchain.toml), the pinned Node version from [`.nvmrc`](../.nvmrc), the JavaScript dependencies and the git hooks. At the end it runs all checks. The script can be run again at any time, e.g. after the pinned versions change.

Then add the two lines it prints to `~/.zshrc` and open a new terminal.

| Tool | Version source | Purpose |
| --- | --- | --- |
| Xcode | App Store | macOS and iOS SDKs, simulator, signing |
| rustup + Rust | `rust-toolchain.toml` | Rust core and Tauri shell |
| fnm + Node.js | `.nvmrc` | UI build tooling |
| pnpm | `package.json` → `packageManager` | JavaScript dependencies |
| Tauri CLI | `package.json` (dev dependency) | `pnpm tauri …` |
| CocoaPods | Brewfile | iOS dependencies used by Tauri |
| lefthook | Brewfile, `lefthook.yml` | pre-commit and commit-message checks |
| cargo-deny | Brewfile, `deny.toml` | licence and advisory checks |
| Pandoc, Typst | Brewfile | optional desktop exporter (ADR-0010) |
| gh | Brewfile | GitHub from the command line |

## 5. Everyday commands

| Command | What it does |
| --- | --- |
| `pnpm tauri dev` | Run the desktop app with hot reload |
| `pnpm dev`, then <http://localhost:1420/dev/editor.html> | Editor playground in a browser, with a sample note and no Tauri |
| `pnpm dev`, then <http://localhost:1420/dev/import.html> | Import dialog with a stand-in backend and a sample report for Apple Notes, Simplenote, HTML and Markdown (`?metadata=no` starts without the Notes metadata) |
| `pnpm tauri ios dev` | Run in the iOS simulator (pick one from the list) |
| `pnpm tauri ios dev --open --host` | Build for your iPhone and open Xcode; see §6 |
| `cargo test --workspace` | Rust tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | Rust lints, as in CI |
| `pnpm check` | TypeScript type check, ESLint, Prettier |
| `cargo deny check` | Dependency licences and security advisories |

## 6. iOS

The Xcode project lives in `src-tauri/gen/apple` and is committed. Its "Build Rust Code" phase loads `scripts/hook-env.sh`, because Xcode runs build scripts without your shell's PATH (see `docs/spikes/ios-vault.md`). That change lives in the project template `src-tauri/templates/ios/project.yml` (`bundle.iOS.template`), so it survives `pnpm tauri ios init`. Regenerate only when needed, from the repository root, and review the diff before committing: `ios init` writes your `DEVELOPMENT_TEAM` into `project.yml` (remove it; the team comes from `APPLE_DEVELOPMENT_TEAM`), and it drops the `UIApplicationSceneManifest` from `Info.plist`, which `src-tauri/Info.ios.plist` provides. After a Tauri CLI update, take the new CLI's template and re-apply the PATH change (see the comment at the top of the template).

One-time setup for a real iPhone:

1. Xcode → Settings → Accounts → add your Apple ID (a free account is enough for your own devices).
2. Find your team ID: open `src-tauri/gen/apple/schnipsle.xcodeproj`, target `schnipsle_iOS` → Signing & Capabilities → choose your team; the ID then appears as `DEVELOPMENT_TEAM` in `project.pbxproj`. Put it in `~/.zshrc`, so it survives project regeneration and stays out of the repository:

   ```sh
   echo 'export APPLE_DEVELOPMENT_TEAM=<your team id>' >> ~/.zshrc
   ```

3. On the iPhone: Settings → Privacy & Security → Developer Mode → on; after the first install, trust your certificate under Settings → General → VPN & Device Management.

Running on the iPhone:

```sh
pnpm tauri ios dev --open --host
```

In Xcode, choose the phone as run destination and press ▶. Keep the Terminal command running: the phone loads the UI from the Mac over Wi-Fi (`--host`), and Xcode's build phase talks to it. Phone and Mac must be on the same network.

Troubleshooting:

| Message | Remedy |
| --- | --- |
| `Port 1420 is already in use` | An old dev server is still running: `lsof -nP -iTCP:1420 -sTCP:LISTEN`, then `kill <PID>` |
| `Signing for "schnipsle_iOS" requires a development team` | `APPLE_DEVELOPMENT_TEAM` not set in this shell (`source ~/.zshrc`) |
| App shows a blank screen | Started without `--host`, or the build is older than Tauri 2.12 (iOS 27 needs the UIScene life cycle) |
| Xcode build failed, no details | Xcode → Report navigator (⌘9) → latest build; the red line names the cause |

Minimum iOS version: 16 (ADR-0002).

## 7. Commit conventions

- [Conventional Commits](https://www.conventionalcommits.org/) subjects: `feat(core): …`, `fix(ui): …`, `docs(adr): …`.
- Every commit is signed off: `git commit -s` (Developer Certificate of Origin, see [CONTRIBUTING.md](../CONTRIBUTING.md)).
- The commit-message hook checks both.
- The hooks load [`scripts/hook-env.sh`](../scripts/hook-env.sh) first, so they also work from Git apps (VS Code, Fork, Tower) that do not read `~/.zshrc`.

## 8. Linux (desirable platform)

Install the WebView and build dependencies (Debian/Ubuntu):

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

Then install rustup, Node (per `.nvmrc`) and pnpm, and run `pnpm install && pnpm tauri dev`. The Rust core crates need none of the WebView libraries.

## 9. Android (later)

Android Studio, the NDK and a JDK are only needed once Android work starts; see the [Tauri prerequisites](https://v2.tauri.app/guides/prerequisites/).
