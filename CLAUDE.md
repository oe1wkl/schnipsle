# Schnipsle – instructions for Claude Code

Local-first Markdown note app for macOS and iOS (Linux and Android later): notes are plain `.md` files in a user-chosen folder ("vault"), organized by hashtags and saved searches instead of folders. Owner: Willi Kraml (OE1WKL). Open source, Apache-2.0, <https://github.com/oe1wkl/schnipsle>.

Work as a professional software developer: follow the decisions below, keep docs in step with code, and ask before changing a recorded decision.

## Read first

| File                       | What it holds                                                                                               |
| -------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `docs/roadmap.md`          | Current milestone, open tasks, what is next                                                                 |
| `docs/adr/`                | Architecture decisions (ADR-0001 … 0015). Do not contradict an accepted ADR; propose a new one              |
| `docs/format.md`           | Vault file format (normative): notes, tags §5.3, music §5.6, chords §5.7, history, trash, writing rules §10 |
| `docs/design/hld.md`       | High-level design snapshot (requirements F-xx / N-xx, architecture)                                         |
| `docs/spikes/ios-vault.md` | iOS findings, including the iOS 27 build pitfalls                                                           |
| `docs/dev-setup.md`        | Toolchain, commands, iOS signing and troubleshooting                                                        |

## Layout

```text
crates/core/                     schnipsle-core: vault access, tags, (later) index, query. No UI deps
crates/apple/                    schnipsle-apple: NSFileCoordinator and NSFilePresenter via objc2, ADR-0015
crates/import/, crates/export/   importer and exporter crates (empty until M4/M5)
crates/tauri-plugin-vault-access iOS folder picker + security-scoped bookmark (Swift in ios/)
src-tauri/                       Tauri 2 app shell: commands (lib.rs), platform glue (vault_location.rs)
src-tauri/gen/apple/             generated Xcode project, committed and hand-patched (see below)
ui/                              TypeScript front end (Vite), currently a spike UI
docs/                            ADRs, format spec, design, setup, spikes
tests/fixtures/                  golden files
```

## Commands

```sh
cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
cargo fmt --all
pnpm check                 # tsc, eslint, prettier --check
pnpm format                # prettier --write
cargo deny --all-features --workspace check
pnpm tauri dev             # desktop app
pnpm tauri ios dev         # iOS simulator
pnpm tauri ios dev --open --host   # real iPhone via Xcode (needs APPLE_DEVELOPMENT_TEAM)
```

Before reporting work as done, run tests, clippy, fmt and `pnpm check`; all must pass (CI enforces the same, plus cargo-deny and a macOS app build).

## Rules

- **Core first:** logic goes into `schnipsle-core` with unit tests; `src-tauri` only wraps it as commands. The UI never touches files.
- **Data safety:** writes are atomic and must not overwrite a note changed elsewhere (format.md §10). Never discard user text silently.
- **Format changes** need an update of `docs/format.md`, an ADR, and fixtures.
- **Rust:** edition 2024, MSRV 1.90 (toolchain pinned in `rust-toolchain.toml`), clippy pedantic, `unsafe` forbidden except in the Swift bridge crate and `schnipsle-apple` (ADR-0015: `// SAFETY:` on every block, safe public API), every file starts with `// SPDX-License-Identifier: Apache-2.0`.
- **TypeScript:** strict, ESLint strictTypeChecked, Prettier (width 100).
- **Licences:** no GPL/AGPL/LGPL/SSPL dependencies (App Store, ADR-0005); cargo-deny and `scripts/check-npm-licenses.mjs` check this. Pandoc may only be called as an external program on desktop, never bundled.
- **Commits:** Conventional Commits (`feat(core): …`), signed off with `git commit -s`; the commit-msg hook checks both. Do not push, open pull requests or merge without being asked.
- **Changelog:** user-visible changes go under "Unreleased" in `CHANGELOG.md`.

## Pitfalls already paid for

- **Do not run `pnpm tauri ios init`.** The committed Xcode project's "Build Rust Code" phase sources `scripts/hook-env.sh` (Xcode has no shell PATH); regeneration loses this.
- iOS 27 needs the UIScene life cycle: Tauri ≥ 2.12 and `src-tauri/Info.ios.plist`. Do not downgrade Tauri.
- Tauri's dialog plugin has no folder picker on iOS; use `tauri-plugin-vault-access`.
- Plugin calls that wait for iOS block: call them only from `async` commands, never on the main thread.
- Keep npm `@tauri-apps/*` and the Rust `tauri` crate on the same minor version, or the CLI refuses to build.
- Minimum iOS 16 (ADR-0002), Safari 16 target in `vite.config.ts`.
