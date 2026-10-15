# ADR-0015: Apple file coordination in a Rust crate using objc2

- Status: Accepted
- Date: 2026-10-15
- Deciders: Willi Kraml
- Related: N-04, ADR-0003, ADR-0006, [format.md §10](../format.md#10-writing-rules), [iOS spike](../spikes/ios-vault.md)

## Context

format.md §10 requires file coordination for every read and write on Apple platforms, and ADR-0006 requires the app to notice changes made by sync services and other apps. The spike does neither: it checks a content hash before renaming, which leaves a short window in which iCloud Drive can replace the file unseen, and it finds external changes by polling every 3 s.

Apple's mechanism is `NSFileCoordinator` (exclusive or shared access for the duration of a block, honoured by iCloud Drive and other coordinating apps) and `NSFilePresenter` (callbacks when coordinated changes happen to a file or folder). Both are Objective-C APIs.

Forces:

- **Core first:** the write logic (hash check, atomic replace, conflict handling) lives in `schnipsle-core` and must not be duplicated per platform.
- Coordination runs a caller-supplied block while access is held. The code that checks and replaces the file must run _inside_ that block, so it cannot sit on the far side of an asynchronous message boundary such as a Tauri plugin `invoke`.
- The workspace forbids `unsafe`. The only exception so far is `tauri-plugin-vault-access`, whose Swift bridge needs one foreign-function declaration.
- Tauri builds Swift plugin code only for iOS. macOS needs coordination too.
- Not every app coordinates: on macOS, editors such as Vim or VS Code write files without telling presenters.

## Decision

- **`schnipsle-core` defines a `Coordinator` trait** and accepts an implementation when a vault is opened. The default implementation, `Uncoordinated`, just calls the closure; Linux, Windows, Android and all tests use it.

  ```rust
  pub trait Coordinator: Send + Sync + std::fmt::Debug {
      /// Runs `f` with shared read access to `path`; `f` receives the path to use.
      fn read(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()>;
      /// Runs `f` with exclusive access for replacing `path`.
      fn write(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()>;
  }
  ```

  `Vault::write_note` keeps writing and flushing the temporary file first. It then performs the hash check and the rename inside `Coordinator::write`, so on Apple platforms no coordinating process can change the note between the check and the replace.

- **A new crate `crates/apple` (`schnipsle-apple`)** implements the trait with `NSFileCoordinator` through `objc2-foundation` (MIT, already in the dependency tree through Tauri). The same code serves macOS and iOS. On other targets the crate is empty, so the workspace still builds everywhere.
- **Change notification** is a second, separate job of the platform layer. Events are only hints that tell the app to re-read. The hash check stays the authority on whether a save is safe.
  - **iOS:** an `NSFilePresenter` on the vault folder, defined in `schnipsle-apple` with objc2's `define_class!`. Its callbacks run on its own serial `NSOperationQueue`, never the main queue.
  - **macOS:** the same presenter, so iCloud Drive and other coordinating apps can ask the app to save or relinquish the file. It is combined with `notify` (FSEvents) to catch editors that do not coordinate.
  - **Linux, Windows, Android:** `notify` alone.
- **`unsafe` is allowed in `schnipsle-apple` only**, under stricter lints than elsewhere: `unsafe_op_in_unsafe_fn`, `clippy::undocumented_unsafe_blocks` and `clippy::multiple_unsafe_ops_per_block` are denied, every `unsafe` block has a `// SAFETY:` comment, and the crate's public API is entirely safe. All other crates keep `unsafe_code = "forbid"`.
- **Threading:** coordination blocks the calling thread until access is granted, and may wait for a presenter that needs the main thread. Vault operations that coordinate are therefore called only from `async` Tauri commands, never on the main thread. This is the rule CLAUDE.md already sets for plugin calls that wait.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Rust crate with objc2 (chosen) | Logic stays in core and is tested once; one implementation for macOS and iOS; no second language in the write path; objc2 is already a transitive dependency | A second crate with `unsafe`; objc2 is pre-1.0 and its API still changes |
| Swift in `tauri-plugin-vault-access` | Idiomatic Apple code; no `unsafe` in Rust beyond the existing bridge | Hash check and atomic replace duplicated in Swift; plugin `invoke` is asynchronous, so Rust code cannot run inside the coordination block; Tauri builds the Swift part for iOS only, so macOS needs yet another path |
| Swift package called through swift-rs callbacks | Coordination code in Swift; Rust closure runs inside the block | Two languages and an FFI callback in the most sensitive code path; a Swift build step for the desktop app; still needs `unsafe` |
| No coordination (spike status) | Simplest | Violates format.md §10; iCloud Drive can replace a note between check and rename, or the app can read a half-downloaded file |

## Consequences

- Positive: the conflict check and the replace are atomic with respect to iCloud Drive and other coordinating apps. Polling disappears. iCloud can ask the app to save before it syncs. Core logic stays single-sourced and unit-tested on every platform.
- Negative: there is more `unsafe` code to review, kept small and in one crate. objc2 upgrades may need code changes; it is pinned through the workspace and moves together with Tauri's version. Coordination tests only run on macOS, which CI covers (`macos-latest`).
- Follow-up work:
  - CLAUDE.md: name `schnipsle-apple` as the second exception to the `unsafe` ban.
  - Use `NSFileVersion` to detect iCloud conflict versions (ADR-0006) in a later step.
  - Decide whether temporary files should move from the note's folder to `NSItemReplacementDirectory`, so a sync service never uploads a leftover `.tmp` file.
  - Remove the 3 s polling from the test UI once events arrive.
