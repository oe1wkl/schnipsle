# Performance on devices

How Schnipsle is measured on real devices for N-03 ("search under 200 ms for 10,000 notes"), and the results per device and date. The decisions behind it are in [ADR-0045](../adr/0045-measuring-performance-on-devices.md). The core alone is also measured in CI with 20,000 notes (`crates/core/tests/perf.rs`, job `perf`).

## Limits

| What                                                        | Limit                                 |
| ----------------------------------------------------------- | ------------------------------------- |
| A search, the notes of a tag folder or of a smart folder    | under 200 ms (N-03)                   |
| Opening a note                                              | under 100 ms                          |
| The start with an indexed vault, until the note list shows  | under 1.5 s on the iPhone, 1 s on Mac |
| Building the index the first time, an index sync, an export | written down, no limit                |
| Memory                                                      | written down, no limit                |

A limit counts as met on the oldest supported iPhone only if the estimate for it (below) is under the limit too. If the start misses its limit, the editor is loaded on demand.

## What the app measures

The app shell times its steps with a monotonic clock and keeps the times in memory only: per kind, the last 20 and the slowest since the app started. Settings › Diagnostics shows them, with the app version, the device model, the system version and the memory; Copy gives them as text for this report. Nothing is written to disk or sent.

| Kind                                                                                  | From                                | Until                                    |
| ------------------------------------------------------------------------------------- | ----------------------------------- | ---------------------------------------- |
| Start, until the note list shows                                                      | the app's code starts (`run`)       | the first note list is drawn             |
| Opening the vault                                                                     | the vault is opened                 | it is watched and its index sync started |
| Index sync                                                                            | the sync starts, in the background  | it ends; with the notes in the index     |
| All notes, tag folders, notes of a tag folder or smart folder, search, reading a note | the core starts the step            | the core returns                         |
| … until shown                                                                         | the page calls the command          | the next frame after the page drew it    |
| Export                                                                                | the export starts, after the dialog | the last file is written                 |

- The start leaves out what the system does before the app's code runs (loading the program and its libraries), typically some tens of milliseconds. It is measured only when the app opens the vault of the last time by itself; a start that waits for the user to pick a folder is not measured.
- "Until shown" includes the call from the page to the shell and back, drawing the list or putting the note into the editor, and laying out the next frame.
- Memory is the physical footprint of the app's process, now and at its peak, as Xcode's memory gauge and Activity Monitor's Memory column show it and as iOS compares it with its limit. The web view runs in processes of its own (WebKit's WebContent and Networking), which are not included; Activity Monitor on the Mac and Instruments show them.

## How to run it

1. Make the test vault on the Mac, in iCloud Drive, with 10,000 generated notes (invented text, the same for a given seed):

   ```sh
   cargo run -p schnipsle-core --release --example make-vault -- ~/Library/Mobile\ Documents/com~apple~CloudDocs/Schnipsle-Perf
   ```

   The command makes the folder; it refuses one that is not empty. Wait until iCloud has uploaded it.

2. Use release builds: on the Mac `pnpm tauri build --bundles app`; on the iPhone the Release configuration. Note the build in the results.
3. Open the vault (on the iPhone once iCloud has downloaded it; Files shows the progress). The first index build is measured as "Index sync".
4. Quit the app and start it again, so the start is measured with an indexed vault.
5. Then, in this order:
   - All notes, sorted by recently changed and by title.
   - The tag folders `radio`, `radio/cw`, `projekte`, and Untagged.
   - The searches `antenne`, `ant`, `lötzinn gehäuse`, `"radio amateur"`, `antenne OR kirche OR gitarre`, `#projekte kabel -#archiv`, `(spule OR dipol) #radio/cw`, `-#radio` (those of the CI test).
   - A smart folder saved from one of the searches.
   - Five notes opened from the list.
   - Exports of a tag folder with about 25 notes as PDF and as HTML.
6. Settings › Diagnostics › Copy, and paste the text into the results below.

## The oldest supported iPhone

iOS 16 runs on the iPhone 8 and X (A11) and later (ADR-0002). There is none at hand, so its times are estimated from those of the iPhone 18 Pro: each time multiplied by the ratio of the two chips' Geekbench 6 single-core scores, then by 1.5 as a margin for slower storage and less memory. The results name the scores used and where they come from.

## Results

None yet. Each run adds a section: date, device and system, app version and build, the vault (notes, seed), the times against the limits, the memory, the estimate for the A11 where it applies, and the copied text.
