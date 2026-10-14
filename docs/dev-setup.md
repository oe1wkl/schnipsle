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
| `pnpm tauri ios init` | Once: generate the Xcode project in `src-tauri/gen/apple` (committed) |
| `pnpm tauri ios dev` | Run in the iOS simulator |
| `pnpm tauri ios dev --open` | Open in Xcode, choose your iPhone as the run destination |
| `cargo test --workspace` | Rust tests |
| `cargo clippy --workspace --all-targets -- -D warnings` | Rust lints, as in CI |
| `pnpm check` | TypeScript type check, ESLint, Prettier |
| `cargo deny check` | Dependency licences and security advisories |

Running on a real iPhone: in Xcode select the `schnipsle_iOS` target → Signing & Capabilities → choose your team (your Apple ID), then pick the phone as the run destination. The phone must have Developer Mode enabled (Settings → Privacy & Security → Developer Mode).

## 6. Commit conventions

- [Conventional Commits](https://www.conventionalcommits.org/) subjects: `feat(core): …`, `fix(ui): …`, `docs(adr): …`.
- Every commit is signed off: `git commit -s` (Developer Certificate of Origin, see [CONTRIBUTING.md](../CONTRIBUTING.md)).
- The commit-message hook checks both.
- The hooks load [`scripts/hook-env.sh`](../scripts/hook-env.sh) first, so they also work from Git apps (VS Code, Fork, Tower) that do not read `~/.zshrc`.

## 7. Linux (desirable platform)

Install the WebView and build dependencies (Debian/Ubuntu):

```sh
sudo apt install libwebkit2gtk-4.1-dev build-essential curl wget file libxdo-dev \
  libssl-dev libayatana-appindicator3-dev librsvg2-dev
```

Then install rustup, Node (per `.nvmrc`) and pnpm, and run `pnpm install && pnpm tauri dev`. The Rust core crates need none of the WebView libraries.

## 8. Android (later)

Android Studio, the NDK and a JDK are only needed once Android work starts; see the [Tauri prerequisites](https://v2.tauri.app/guides/prerequisites/).
