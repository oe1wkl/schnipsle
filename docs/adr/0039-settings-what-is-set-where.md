# ADR-0039: Settings: what is set where

- Status: Accepted
- Date: 2026-11-17
- Deciders: Willi Kraml
- Related: N-02, N-04, ADR-0010, ADR-0011, ADR-0017, ADR-0025, ADR-0030, ADR-0036, [format.md §1, §7](../format.md#1-vault)

## Context

1.0 gets a settings view (roadmap, M7). It has to show five settings.

The app already reads three of them from the vault's `.notesapp/settings.yaml`, which syncs to every device:

- `history_keep_all_days` and `history_keep_daily_days`: how long snapshots are kept (ADR-0017).
- `chord_notation`: English or German chord names (ADR-0030).

Until now that file was only edited by hand. A typo silently means the default.

Two more settings belong to one computer, not to the vault:

- **The Pandoc program.** It is found on `PATH` or in Homebrew's folders (ADR-0025). Someone with Pandoc elsewhere, or with several versions, cannot choose one.
- **The update check.** ADR-0036 says it can be turned off in the settings view and is on by default.

Format.md §1 forbids per-device data in the vault.

A shorter retention removes snapshots for good. Today the history is thinned in the background each time a vault is opened. A shorter period entered in a form would therefore delete versions at the next start, without a word.

## Decision

Each setting is stored where it applies: settings of the vault in `settings.yaml`, settings of this device in a file of the app's own. The view shows them in two sections, "This vault" and "This Mac" (or "This iPhone"), with room for more device settings later (the reading view).

1. **Settings of the vault** stay in `.notesapp/settings.yaml`, with the keys format.md already names: retention and chord names. They apply on every device that opens the vault.
2. **Writing `settings.yaml`.** The app changes only the line of each key it sets.
   - Other lines stay as they are: comments, unknown keys and their order.
   - A key the file does not have is added at the end.
   - Setting a value back to its default removes the key's line, so the file only holds what differs.
   - The file is written atomically and must not overwrite a version changed elsewhere (format.md §10). If it changed in between, for example on another device, the same line changes are made again to the newer text, as for smart folders (ADR-0018). A change of another key made elsewhere is kept; for the same key, the later change wins.
   - A read-only vault shows its settings without letting them be changed.
3. **Settings of the device** go in `settings.json` in the app's configuration folder, next to the remembered vault, never in the vault.
   - Unknown keys are kept when the file is written, so a newer app's settings survive an older one.
   - It holds the Pandoc program and the update check. The iPhone has neither.
4. **Pandoc.** The choice is "find it" (the default, as in ADR-0025) or a program the user picked, whose version the view shows. If the picked program does not run, the app does not fall back to another one: DOCX and EPUB are not offered, and the export dialog and the view say why.
5. **Removing versions is confirmed first.** Before a change of the retention is saved, the app counts what thinning with the new periods would remove now ("This removes 312 earlier versions of 48 notes for good, on every device."). If that is anything, it asks; only after the user agrees does it save the setting and thin the history right away, so what was shown is what happens. A change that removes nothing, such as a longer period, is saved without asking. Nothing that is already removed comes back.
6. **The update check** is a switch, on by default, shown before the updater exists. Until the packaging step adds `tauri-plugin-updater`, nothing reads it. The updater then checks only while the switch is on (ADR-0036).
7. **Where the view opens.** On the Mac: Settings… (⌘,) in the app menu. On the iPhone, which has no menu: a gear button in the app's bar. Each change is saved at once, as is usual on the Mac; there is no Save button.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Vault in `settings.yaml`, device in the app's own file (chosen) | Retention the same on every device (ADR-0017); a path of one Mac stays on that Mac | Two places, two sections in the view |
| Everything in `settings.yaml` | One file | A Pandoc path of one Mac is wrong on the next; against format.md §1 |
| Everything on the device | No writing in the vault | Two devices thin one history differently; contradicts ADR-0017 |
| Device settings in the web view's `localStorage` | Already used for small UI choices | The Rust side needs the Pandoc path; storage the web view may clear |
| Rewrite `settings.yaml` whole from its keys | Simplest writer | Loses comments and the keys of a newer app |
| Change only the lines of the keys set (chosen) | Hand edits and unknown keys survive | A line-based writer; only the flat YAML the app already reads |
| A shorter retention saved at once | No dialog | Versions deleted at the next start, without a word (N-04) |
| A picked Pandoc falls back to one that is found | DOCX keeps working | Silently uses a program the user did not choose |

## Consequences

- Positive:
  - Every setting can be changed without editing a file.
  - Hand edits and unknown keys in `settings.yaml` survive the app.
  - A shorter retention cannot delete versions unannounced.
- Negative:
  - A second settings file to look after.
  - The update switch does nothing until the updater comes.
  - `settings.yaml` stays flat YAML; a nested setting would need a fuller writer.
- Follow-up work:
  - format.md §1 and §7: how `settings.yaml` is written (revision 0.16), with fixtures.
  - The updater reads the switch (macOS packaging).
  - The reading view's settings under "This Mac" / "This iPhone".
  - The settings in the manual.
