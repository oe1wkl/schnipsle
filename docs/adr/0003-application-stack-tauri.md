# ADR-0003: Application stack: Tauri 2, Rust core, TypeScript UI

- Status: Accepted, conditional on the M0 iOS feasibility spike
- Date: 2026-10-13
- Deciders: Willi Kraml
- Related: ADR-0002, N-03, N-05

## Context

We need one code base for macOS and iOS (must), with Linux and Android possible later. The core features are Markdown editing, fast full-text search over at least 10,000 notes, robust file handling, import and export.

## Decision

- **Shell:** Tauri 2 (desktop and mobile targets).
- **Core:** Rust library crates (`crates/core`, `crates/import`, `crates/export`) without any UI dependency. All file access, parsing, indexing and queries happen here.
- **UI:** TypeScript (strict) in the system WebView, editor based on CodeMirror 6.
- **Boundary:** the UI talks to the core only through Tauri commands and events; the UI never reads or writes vault files.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Tauri 2 | macOS, iOS, Linux, Android, Windows from one code base; small binaries; Rust core is fast and memory-safe; CodeMirror 6 is the strongest Markdown editor component | Two languages; Tauri mobile is younger than desktop; WebView differences |
| Electron | One language, uniform rendering | No iOS; large binaries |
| Swift / SwiftUI | Best Apple integration | Rules out Linux and Android |
| Qt 6 / C++ | Familiar language, native widgets | No good Markdown editor widget; iOS support with QML is laborious |
| Flutter | iOS + Android + desktop | No mature Markdown source editor; Dart-only ecosystem for parsing/indexing |

## Consequences

- Positive: all musts and desirables reachable; the core is testable without UI and reusable by a native shell if ever needed.
- Negative: contributors need Rust and TypeScript.
- Risk: Tauri on iOS. **Mitigation:** M0 spike — a Tauri app that opens a vault folder in iCloud Drive on a real iPhone, edits a note with CodeMirror, and sees changes made on the Mac. If the spike fails, a follow-up ADR evaluates a native Swift shell on top of the same Rust core (via UniFFI).

## Amendments

- 2026-10-14: iOS 27 requires the UIScene life cycle; **Tauri ≥ 2.12 (tao ≥ 0.37)** is needed, together with `UIApplicationSceneManifest` in `src-tauri/Info.ios.plist`. Earlier versions stop at launch. Details: `docs/spikes/ios-vault.md`.
