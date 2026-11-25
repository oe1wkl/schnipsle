# Performance on devices

How Schnipsle is measured on real devices for N-03 ("search under 200 ms for 10,000 notes"), and the results per device and date. The decisions behind it are in [ADR-0045](../adr/0045-measuring-performance-on-devices.md). The core alone is also measured in CI with 20,000 notes (`crates/core/tests/perf.rs`, job `perf`).

## Limits

| What                                                        | Limit                                 |
| ----------------------------------------------------------- | ------------------------------------- |
| A search, all notes, the notes of a tag or smart folder     | under 200 ms (N-03)                   |
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
   cargo run -p schnipsle-core --release --example make-vault -- ~/Library/Mobile\ Documents/com~apple~CloudDocs/Schnipsle-Perf-10k
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

Each run adds a section: date, device and system, app version and build, the vault (notes, seed), the times against the limits, the memory, the estimate for the A11 where it applies, and the copied text.

### iPhone 18 Pro, 2026-11-24, with ADR-0047

- Device, vault and procedure as in the run before.
- App: 0.1.0, Release build (`pnpm tauri ios build --export-method debugging`) of `perf/n03-bulk-listing`, the code merged in PR 114 (folders listed with `getattrlistbulk`, ADR-0047).

| What                                     | In the core              | Until shown            | Limit  |     |
| ---------------------------------------- | ------------------------ | ---------------------- | ------ | --- |
| Start, indexed vault                     |                          | 204 ms                 | 1.5 s  | met |
| All notes                                | median 38, max 45 ms     | median 57, max 128 ms  | 200 ms | met |
| Notes of a tag folder (up to 2347 notes) | median 2.8, max 11 ms    | median 128, max 162 ms | 200 ms | met |
| Notes of a smart folder                  | 8.2 ms                   | 124 ms                 | 200 ms | met |
| Search, while typing                     | median 25, max 93 ms     | median 34, max 118 ms  | 200 ms | met |
| Opening a note                           | median 22, max 38 ms     | median 45, max 57 ms   | 100 ms | met |
| Index sync, nothing changed              | 27 ms                    |                        | none   |     |
| Memory                                   | 38.4 MB, at most 40.4 MB |                        | none   |     |

- Against the run before (ADR-0046 only): all notes 262 → 38 ms in the core and 273 → 57 ms until shown, the start 583 → 204 ms, the index sync about 300 → 27 ms, reading a note 61 → 22 ms. Asking each of 10,000 files for its time and size with `lstat` had taken about 230 ms; in a test build the same for a folder with `getattrlistbulk` took about 0.4 µs per entry instead of 23 µs.
- Tag and smart folders showed either after about 30 ms or after about 125 ms, although the core took 2–11 ms. On the phone the list was shown while hidden and drawn afterwards, so the next frame laid out every old row first; PR 115 draws it before showing it. That build is not measured here.
- No export was made in this run. The first index build is not in the copied text.

#### Estimate for the A11

Scores and factor as in the run before: 4,735 / 1,040 × 1.5 ≈ 6.8.

| What                    | iPhone 18 Pro, until shown | A11, estimated           | Limit  |        |
| ----------------------- | -------------------------- | ------------------------ | ------ | ------ |
| Start, indexed vault    | 204 ms                     | 1.4 s                    | 1.5 s  | met    |
| All notes               | median 57, max 128 ms      | median 389, max 874 ms   | 200 ms | missed |
| Notes of a tag folder   | median 128, max 162 ms     | median 874 ms, max 1.1 s | 200 ms | missed |
| Notes of a smart folder | 124 ms                     | 847 ms                   | 200 ms | missed |
| Search                  | median 34, max 118 ms      | median 232, max 806 ms   | 200 ms | missed |
| Opening a note          | median 45, max 57 ms       | median 307, max 389 ms   | 100 ms | missed |

- With the start within its limit on the A11 estimate too, the editor stays loaded at the start (see Limits).
- The tag and smart folder times are those before PR 115.

<details>
<summary>Copied text</summary>

```text
Schnipsle 0.1.0 on iPhone19,2, ios 27.2.1
2026-11-24T15:30:30.252Z
Memory: 38.4 MB, at most 40.4 MB

Start, until the note list shows: 1 time; median of the last ones 204 ms, slowest 204 ms
  204 ms

Opening the vault: 1 time; median of the last ones 10 ms, slowest 10 ms
  10 ms

Index sync: 1 time; median of the last ones 27 ms, slowest 27 ms (10000 notes, 0 read, 0 removed)
  27 ms (10000 notes, 0 read, 0 removed)

All notes: 22 times; median of the last ones 38 ms, slowest 45 ms (10000 notes)
  31 ms (10000 notes)
  20 ms (10000 notes)
  43 ms (10000 notes)
  43 ms (10000 notes)
  40 ms (10000 notes)
  40 ms (10000 notes)
  35 ms (10000 notes)
  43 ms (10000 notes)
  28 ms (10000 notes)
  19 ms (10000 notes)
  32 ms (10000 notes)
  44 ms (10000 notes)
  43 ms (10000 notes)
  33 ms (10000 notes)
  38 ms (10000 notes)
  45 ms (10000 notes)
  37 ms (10000 notes)
  39 ms (10000 notes)
  29 ms (10000 notes)
  17 ms (10000 notes)

All notes, until shown: 22 times; median of the last ones 57 ms, slowest 128 ms (10000 notes)
  128 ms (10000 notes)
  34 ms (10000 notes)
  55 ms (10000 notes)
  57 ms (10000 notes)
  59 ms (10000 notes)
  56 ms (10000 notes)
  72 ms (10000 notes)
  58 ms (10000 notes)
  122 ms (10000 notes)
  38 ms (10000 notes)
  70 ms (10000 notes)
  57 ms (10000 notes)
  56 ms (10000 notes)
  75 ms (10000 notes)
  52 ms (10000 notes)
  57 ms (10000 notes)
  73 ms (10000 notes)
  53 ms (10000 notes)
  125 ms (10000 notes)
  29 ms (10000 notes)

Tag folders: 2 times; median of the last ones 25 ms, slowest 26 ms (22 folders)
  25 ms (22 folders)
  26 ms (22 folders)

Notes of a tag folder: 11 times; median of the last ones 2.8 ms, slowest 11 ms (1971 notes)
  11 ms (1971 notes)
  5.4 ms (2347 notes)
  2.0 ms (653 notes)
  2.5 ms (653 notes)
  2.7 ms (622 notes)
  3.0 ms (648 notes)
  1.4 ms (659 notes)
  3.0 ms (659 notes)
  2.8 ms (666 notes)
  1.9 ms (600 notes)
  3.1 ms (661 notes)

Notes of a tag folder, until shown: 11 times; median of the last ones 128 ms, slowest 162 ms (661 notes)
  30 ms (1971 notes)
  128 ms (2347 notes)
  32 ms (653 notes)
  128 ms (653 notes)
  127 ms (622 notes)
  129 ms (648 notes)
  29 ms (659 notes)
  128 ms (659 notes)
  125 ms (666 notes)
  128 ms (600 notes)
  162 ms (661 notes)

Notes of a smart folder: 1 time; median of the last ones 8.2 ms, slowest 8.2 ms (200 notes)
  8.2 ms (200 notes)

Notes of a smart folder, until shown: 1 time; median of the last ones 124 ms, slowest 124 ms (200 notes)
  124 ms (200 notes)

Search: 22 times; median of the last ones 25 ms, slowest 93 ms (200 notes, for “T”)
  29 ms (200 notes, for “Morgen”)
  12 ms (200 notes, for “Morgen”)
  25 ms (200 notes, for “Morgen”)
  12 ms (200 notes, for “Morgen”)
  25 ms (200 notes, for “Morgen”)
  11 ms (200 notes, for “Morgen”)
  29 ms (200 notes, for “Morgen”)
  35 ms (200 notes, for “Morge”)
  45 ms (200 notes, for “Mo”)
  83 ms (200 notes, for “M”)
  54 ms (200 notes, for “P”)
  30 ms (200 notes, for “Fo”)
  0.7 ms (0 notes, for “For”)
  0.8 ms (0 notes, for “Form”)
  0.7 ms (0 notes, for “Forma”)
  0.7 ms (0 notes, for “Form”)
  0.7 ms (0 notes, for “For”)
  38 ms (200 notes, for “Fo”)
  73 ms (200 notes, for “F”)
  0.8 ms (0 notes, for “Typ”)

Search, until shown: 22 times; median of the last ones 34 ms, slowest 118 ms (200 notes)
  39 ms (200 notes)
  25 ms (200 notes)
  33 ms (200 notes)
  27 ms (200 notes)
  34 ms (200 notes)
  25 ms (200 notes)
  41 ms (200 notes)
  49 ms (200 notes)
  58 ms (200 notes)
  108 ms (200 notes)
  66 ms (200 notes)
  42 ms (200 notes)
  5.0 ms (0 notes)
  9.0 ms (0 notes)
  20 ms (0 notes)
  12 ms (0 notes)
  16 ms (0 notes)
  54 ms (200 notes)
  82 ms (200 notes)
  10 ms (0 notes)

Reading a note: 6 times; median of the last ones 22 ms, slowest 38 ms (2 KB)
  20 ms (3 KB)
  38 ms (2 KB)
  35 ms (1 KB)
  19 ms (2 KB)
  15 ms (3 KB)
  23 ms (2 KB)

Opening a note, until shown: 6 times; median of the last ones 45 ms, slowest 57 ms (3 KB)
  43 ms (3 KB)
  46 ms (2 KB)
  43 ms (1 KB)
  26 ms (2 KB)
  57 ms (3 KB)
  52 ms (2 KB)
```

</details>

### iPhone 18 Pro, 2026-11-24, with ADR-0046

- Device: iPhone 18 Pro (iPhone19,2), iOS 27.2.1.
- App: 0.1.0, Release build (`pnpm tauri ios build --export-method debugging`) of `perf/n03-index-reads` at b312adc, the code merged in PR 112.
- Vault: the same `Schnipsle-Perf-10k` in iCloud Drive, completely downloaded; the app was started again after the first index build.

| What                                     | In the core              | Until shown            | Limit  |        |
| ---------------------------------------- | ------------------------ | ---------------------- | ------ | ------ |
| Start, indexed vault                     |                          | 583 ms                 | 1.5 s  | met    |
| All notes                                | median 262, max 290 ms   | median 273, max 314 ms | 200 ms | missed |
| Notes of a tag folder (up to 2347 notes) | median 3.1, max 9.4 ms   | median 40, max 130 ms  | 200 ms | met    |
| Notes of a smart folder                  | median 23, max 27 ms     | median 35, max 130 ms  | 200 ms | met    |
| Search, while typing                     | median 9.4, max 106 ms   | median 26, max 132 ms  | 200 ms | met    |
| Opening a note                           | median 61, max 74 ms     | median 67, max 88 ms   | 100 ms | met    |
| Index sync, nothing changed              | 298–320 ms               |                        | none   |        |
| Export of 33 notes                       | PDF 176 ms, HTML 46 ms   |                        | none   |        |
| Memory                                   | 37.9 MB, at most 46.5 MB |                        | none   |        |

- All notes took 237–290 ms in the core, about five times the Mac's median of 51 ms, although no note had changed since the index sync, so no file needed to be read. The index sync, which also only compares each file's modification time and size, took about 300 ms. Both list the 10,000 files of `notes/` and ask each for its time and size; on the iPhone this probably takes most of the time. It is not measured on its own yet.
- Reading a note took about 61 ms in the core, against 18 ms on the Mac.
- The searches were typed letter by letter; the copied text keeps the last 20 of 137, among them `(spule OR dipol) #radio/cw` (32 ms). The slowest was the one letter "A" (93–106 ms in the core).
- The first index build after the download is not in the copied text: it was in the app's run before the restart.

#### Estimate for the A11

Geekbench 6 single-core scores, as of 2026-11-24: the iPhone 18 Pro (A20 Pro) 4,735, from [Tom's Guide's review](https://www.tomsguide.com/phones/iphones/iphone-18-pro-benchmarks-apple-just-built-the-most-powerful-phones-weve-ever-seen); the A11 1,040, the average on [CPU-Monkey](https://www.cpu-monkey.com/en/benchmark-apple_a11_bionic-geekbench_6_single_core) ([Notebookcheck](https://www.notebookcheck.net/Apple-A11-Bionic-SoC-Benchmarks-and-Specs.250986.0.html) lists 1,100; the lower score is used). Factor: 4,735 / 1,040 × 1.5 ≈ 6.8.

| What                    | iPhone 18 Pro, until shown | A11, estimated          | Limit  |                                |
| ----------------------- | -------------------------- | ----------------------- | ------ | ------------------------------ |
| Start, indexed vault    | 583 ms                     | 4.0 s                   | 1.5 s  | missed                         |
| All notes               | median 273, max 314 ms     | median 1.9 s, max 2.1 s | 200 ms | missed                         |
| Notes of a tag folder   | median 40, max 130 ms      | median 273, max 888 ms  | 200 ms | missed                         |
| Notes of a smart folder | median 35, max 130 ms      | median 239, max 888 ms  | 200 ms | missed                         |
| Search                  | median 26, max 132 ms      | median 178, max 901 ms  | 200 ms | median met, the slowest missed |
| Opening a note          | median 67, max 88 ms       | median 458, max 601 ms  | 100 ms | missed                         |

- The estimate scales every time with the processor, also waiting for storage and drawing in WebKit, and adds 50 %. It is meant to be on the safe side, not exact.

<details>
<summary>Copied text</summary>

```text
Schnipsle 0.1.0 on iPhone19,2, ios 27.2.1
2026-11-24T07:49:25.741Z
Memory: 37.9 MB, at most 46.5 MB

Start, until the note list shows: 1 time; median of the last ones 583 ms, slowest 583 ms
  583 ms

Opening the vault: 1 time; median of the last ones 11 ms, slowest 11 ms
  11 ms

Index sync: 2 times; median of the last ones 309 ms, slowest 320 ms (10000 notes, 0 read, 0 removed)
  298 ms (10000 notes, 0 read, 0 removed)
  320 ms (10000 notes, 0 read, 0 removed)

All notes: 11 times; median of the last ones 262 ms, slowest 290 ms (10000 notes)
  290 ms (10000 notes)
  263 ms (10000 notes)
  266 ms (10000 notes)
  253 ms (10000 notes)
  247 ms (10000 notes)
  245 ms (10000 notes)
  258 ms (10000 notes)
  266 ms (10000 notes)
  262 ms (10000 notes)
  237 ms (10000 notes)
  285 ms (10000 notes)

All notes, until shown: 11 times; median of the last ones 273 ms, slowest 314 ms (10000 notes)
  314 ms (10000 notes)
  270 ms (10000 notes)
  273 ms (10000 notes)
  263 ms (10000 notes)
  258 ms (10000 notes)
  257 ms (10000 notes)
  265 ms (10000 notes)
  273 ms (10000 notes)
  278 ms (10000 notes)
  283 ms (10000 notes)
  292 ms (10000 notes)

Tag folders: 12 times; median of the last ones 20 ms, slowest 47 ms (21 folders)
  47 ms (21 folders)
  16 ms (23 folders)
  16 ms (23 folders)
  18 ms (22 folders)
  22 ms (22 folders)
  17 ms (23 folders)
  31 ms (22 folders)
  16 ms (23 folders)
  28 ms (22 folders)
  40 ms (23 folders)
  19 ms (22 folders)
  23 ms (22 folders)

Notes of a tag folder: 11 times; median of the last ones 3.1 ms, slowest 9.4 ms (1971 notes)
  2.5 ms (33 notes)
  3.0 ms (33 notes)
  1.9 ms (33 notes)
  2.8 ms (33 notes)
  2.8 ms (33 notes)
  3.2 ms (648 notes)
  9.4 ms (1971 notes)
  6.3 ms (2347 notes)
  3.1 ms (651 notes)
  5.9 ms (1870 notes)
  4.2 ms (1870 notes)

Notes of a tag folder, until shown: 11 times; median of the last ones 40 ms, slowest 130 ms (1870 notes)
  18 ms (33 notes)
  18 ms (33 notes)
  13 ms (33 notes)
  14 ms (33 notes)
  40 ms (33 notes)
  30 ms (648 notes)
  65 ms (1971 notes)
  46 ms (2347 notes)
  64 ms (651 notes)
  58 ms (1870 notes)
  130 ms (1870 notes)

Notes of a smart folder: 14 times; median of the last ones 23 ms, slowest 27 ms (200 notes)
  25 ms (200 notes)
  9.6 ms (200 notes)
  25 ms (200 notes)
  10 ms (200 notes)
  27 ms (200 notes)
  9.7 ms (200 notes)
  25 ms (200 notes)
  9.6 ms (200 notes)
  25 ms (200 notes)
  9.0 ms (200 notes)
  23 ms (200 notes)
  23 ms (200 notes)
  26 ms (200 notes)
  22 ms (200 notes)

Notes of a smart folder, until shown: 14 times; median of the last ones 35 ms, slowest 130 ms (200 notes)
  37 ms (200 notes)
  13 ms (200 notes)
  35 ms (200 notes)
  52 ms (200 notes)
  35 ms (200 notes)
  25 ms (200 notes)
  35 ms (200 notes)
  25 ms (200 notes)
  35 ms (200 notes)
  18 ms (200 notes)
  130 ms (200 notes)
  34 ms (200 notes)
  36 ms (200 notes)
  32 ms (200 notes)

Search: 137 times; median of the last ones 9.4 ms, slowest 106 ms (200 notes, for “A”)
  28 ms (200 notes, for “Antenne”)
  29 ms (200 notes, for “Antenn”)
  32 ms (200 notes, for “Ante”)
  57 ms (200 notes, for “An”)
  93 ms (200 notes, for “A”)
  0.0 ms (0 notes, for “-”)
  0.0 ms (0 notes, for “-#”)
  9.2 ms (200 notes, for “-#r”)
  9.6 ms (200 notes, for “-#ra”)
  9.2 ms (200 notes, for “-#rall”)
  9.0 ms (200 notes, for “-#ralll”)
  12 ms (200 notes, for “-#radio”)
  8.1 ms (200 notes, for “-#radi”)
  11 ms (200 notes, for “-#rad”)
  8.4 ms (200 notes, for “-#ra”)
  11 ms (200 notes, for “-#r”)
  0.0 ms (0 notes, for “-#”)
  0.0 ms (0 notes, for “-”)
  32 ms (200 notes, for “(Spule OR Dipol) #radio/CW”)
  0.5 ms (0 notes, for “(Spule OR Dipol) #radio/C”)

Search, until shown: 137 times; median of the last ones 26 ms, slowest 132 ms (200 notes)
  40 ms (200 notes)
  40 ms (200 notes)
  42 ms (200 notes)
  65 ms (200 notes)
  114 ms (200 notes)
  7.0 ms (0 notes)
  19 ms (0 notes)
  25 ms (200 notes)
  27 ms (200 notes)
  27 ms (200 notes)
  24 ms (200 notes)
  23 ms (200 notes)
  21 ms (200 notes)
  26 ms (200 notes)
  21 ms (200 notes)
  23 ms (200 notes)
  19 ms (0 notes)
  71 ms (0 notes)
  42 ms (200 notes)
  12 ms (0 notes)

Reading a note: 5 times; median of the last ones 61 ms, slowest 74 ms (2 KB)
  63 ms (3 KB)
  60 ms (3 KB)
  61 ms (3 KB)
  61 ms (2 KB)
  74 ms (2 KB)

Opening a note, until shown: 5 times; median of the last ones 67 ms, slowest 88 ms (2 KB)
  68 ms (3 KB)
  65 ms (3 KB)
  66 ms (3 KB)
  67 ms (2 KB)
  88 ms (2 KB)

Export: 2 times; median of the last ones 111 ms, slowest 176 ms (PDF, 33 notes)
  46 ms (HTML, 33 notes)
  176 ms (PDF, 33 notes)
```

</details>

### Mac, 2026-11-23, with ADR-0046

- Device, vault and procedure as in the run before; release build of branch `perf/n03-index-reads` (ADR-0046), started twice after the build so the start was not the first one.

| What                                     | In the core              | Until shown           | Limit  |              |
| ---------------------------------------- | ------------------------ | --------------------- | ------ | ------------ |
| Start, indexed vault                     |                          | 423 ms                | 1 s    | met          |
| All notes                                | median 51, max 214 ms    | median 65, max 222 ms | 200 ms | met but once |
| Notes of a tag folder (up to 2347 notes) | median 3.0, max 9.2 ms   | median 26, max 51 ms  | 200 ms | met          |
| Notes of a smart folder                  | median 4.1, max 6.9 ms   | median 17, max 43 ms  | 200 ms | met          |
| Search, the eight of the CI test         | median 26, max 52 ms     | median 61, max 73 ms  | 200 ms | met          |
| Opening a note                           | median 18, max 25 ms     | median 25, max 43 ms  | 100 ms | met          |
| Index sync, nothing changed              | 47–60 ms                 |                       | none   |              |
| Export of 28 notes                       | PDF 172 ms, HTML 40 ms   |                       | none   |              |
| Memory                                   | 51.8 MB, at most 52.9 MB |                       | none   |              |

- One of ten listings of all notes took 214 ms in the core (222 ms until shown); the others took 33–106 ms. Which one it was is not recorded.

<details>
<summary>Copied text</summary>

```text
Schnipsle 0.1.0 on Mac17,3, macos 27.2.1
2026-11-23T16:21:36.807Z
Memory: 51.8 MB, at most 52.9 MB

Start, until the note list shows: 1 time; median of the last ones 423 ms, slowest 423 ms
  423 ms

Opening the vault: 1 time; median of the last ones 8.5 ms, slowest 8.5 ms
  8.5 ms

Index sync: 3 times; median of the last ones 49 ms, slowest 60 ms (10000 notes, 0 read, 0 removed)
  47 ms (10000 notes, 0 read, 0 removed)
  60 ms (10000 notes, 0 read, 0 removed)
  49 ms (10000 notes, 0 read, 0 removed)

All notes: 10 times; median of the last ones 51 ms, slowest 214 ms (10000 notes)
  36 ms (10000 notes)
  52 ms (10000 notes)
  42 ms (10000 notes)
  51 ms (10000 notes)
  51 ms (10000 notes)
  51 ms (10000 notes)
  214 ms (10000 notes)
  106 ms (10000 notes)
  33 ms (10000 notes)
  42 ms (10000 notes)

All notes, until shown: 10 times; median of the last ones 65 ms, slowest 222 ms (10000 notes)
  68 ms (10000 notes)
  82 ms (10000 notes)
  53 ms (10000 notes)
  60 ms (10000 notes)
  61 ms (10000 notes)
  60 ms (10000 notes)
  222 ms (10000 notes)
  113 ms (10000 notes)
  75 ms (10000 notes)
  54 ms (10000 notes)

Tag folders: 31 times; median of the last ones 19 ms, slowest 40 ms (22 folders)
  35 ms (21 folders)
  37 ms (20 folders)
  38 ms (20 folders)
  16 ms (21 folders)
  18 ms (23 folders)
  18 ms (21 folders)
  20 ms (22 folders)
  19 ms (23 folders)
  23 ms (22 folders)
  17 ms (21 folders)
  16 ms (21 folders)
  18 ms (23 folders)
  18 ms (23 folders)
  20 ms (22 folders)
  22 ms (22 folders)
  17 ms (21 folders)
  20 ms (23 folders)
  27 ms (22 folders)
  17 ms (21 folders)
  20 ms (23 folders)

Notes of a tag folder: 10 times; median of the last ones 3.0 ms, slowest 9.2 ms (1971 notes)
  2.0 ms (28 notes)
  3.1 ms (28 notes)
  1.4 ms (28 notes)
  2.9 ms (28 notes)
  2.5 ms (28 notes)
  9.2 ms (1971 notes)
  6.7 ms (1971 notes)
  4.3 ms (2347 notes)
  2.5 ms (651 notes)
  3.3 ms (1870 notes)

Notes of a tag folder, until shown: 10 times; median of the last ones 26 ms, slowest 51 ms (1870 notes)
  19 ms (28 notes)
  30 ms (28 notes)
  8.0 ms (28 notes)
  19 ms (28 notes)
  31 ms (28 notes)
  21 ms (1971 notes)
  32 ms (1971 notes)
  24 ms (2347 notes)
  27 ms (651 notes)
  51 ms (1870 notes)

Notes of a smart folder: 11 times; median of the last ones 4.1 ms, slowest 6.9 ms (200 notes)
  3.8 ms (200 notes)
  4.0 ms (200 notes)
  3.9 ms (200 notes)
  4.1 ms (200 notes)
  3.3 ms (200 notes)
  5.1 ms (200 notes)
  3.9 ms (200 notes)
  4.6 ms (200 notes)
  6.9 ms (200 notes)
  6.6 ms (200 notes)
  4.9 ms (200 notes)

Notes of a smart folder, until shown: 11 times; median of the last ones 17 ms, slowest 43 ms (200 notes)
  15 ms (200 notes)
  10 ms (200 notes)
  22 ms (200 notes)
  8.0 ms (200 notes)
  11 ms (200 notes)
  43 ms (200 notes)
  17 ms (200 notes)
  22 ms (200 notes)
  17 ms (200 notes)
  24 ms (200 notes)
  22 ms (200 notes)

Search: 8 times; median of the last ones 26 ms, slowest 52 ms (200 notes, for “antenne OR kirche OR gitarre”)
  7.8 ms (200 notes, for “-#radio”)
  21 ms (200 notes, for “#projekte kabel -#archiv”)
  33 ms (200 notes, for “#projekte kabel -#archiv”)
  52 ms (200 notes, for “antenne OR kirche OR gitarre”)
  21 ms (127 notes, for “"radio amateur"”)
  31 ms (200 notes, for “lötzinn gehäuse”)
  31 ms (200 notes, for “ant”)
  22 ms (200 notes, for “antenne”)

Search, until shown: 8 times; median of the last ones 61 ms, slowest 73 ms (127 notes)
  61 ms (200 notes)
  69 ms (200 notes)
  72 ms (200 notes)
  61 ms (200 notes)
  73 ms (127 notes)
  59 ms (200 notes)
  47 ms (200 notes)
  36 ms (200 notes)

Reading a note: 5 times; median of the last ones 18 ms, slowest 25 ms (2 KB)
  13 ms (2 KB)
  18 ms (2 KB)
  18 ms (3 KB)
  17 ms (2 KB)
  25 ms (2 KB)

Opening a note, until shown: 5 times; median of the last ones 25 ms, slowest 43 ms (2 KB)
  21 ms (2 KB)
  24 ms (2 KB)
  28 ms (3 KB)
  25 ms (1 KB)
  43 ms (2 KB)

Export: 2 times; median of the last ones 106 ms, slowest 172 ms (PDF, 28 notes)
  40 ms (HTML, 28 notes)
  172 ms (PDF, 28 notes)
```

</details>

### Mac, 2026-11-23, before ADR-0046

- Device: Mac17,3 (Apple silicon), macOS 27.2.1.
- App: 0.1.0, release build (`pnpm tauri build --bundles app`) of `main` at 95e7da3.
- Vault: `Schnipsle-Perf-10k` in iCloud Drive, made with `make-vault` (10,000 notes, seed 42), 39 MB.

| What                                              | In the core              | Until shown          | Limit  |        |
| ------------------------------------------------- | ------------------------ | -------------------- | ------ | ------ |
| Start, indexed vault                              |                          | 1696 ms              | 1 s    | missed |
| All notes                                         | 159–981 ms               | 307–1460 ms          | 200 ms | missed |
| Notes of a tag folder (up to 2347 notes)          | median 4.2, max 39 ms    | median 34, max 64 ms | 200 ms | met    |
| Notes of a smart folder                           | median 15, max 28 ms     | median 31, max 55 ms | 200 ms | met    |
| Search, the eight of the CI test and while typing | median 26, max 61 ms     | median 36, max 69 ms | 200 ms | met    |
| Opening a note                                    | 1425–1682 ms             | 1430–1706 ms         | 100 ms | missed |
| First index build                                 | 14,662 ms                |                      | none   |        |
| Index sync, nothing changed                       | 43–201 ms                |                      | none   |        |
| Export of 28 notes                                | PDF 389 ms, HTML 41 ms   |                      | none   |        |
| Memory                                            | 61.5 MB, at most 66.3 MB |                      | none   |        |

- The start of a freshly built app, the first after the build, took 2172 ms on a vault of 31 notes; the next start 417 ms. A new build or update is started cold once.
- Opening a note read every other note in its folder to look for copies with the same `id`, 10,000 files here. All notes read the start of every file for its title; the first listings after a start took about 0.9 s, later ones about 0.16 s. ADR-0046 changes both.

<details>
<summary>Copied text</summary>

```text
Schnipsle 0.1.0 on Mac17,3, macos 27.2.1
2026-11-23T16:01:58.997Z
Memory: 61.5 MB, at most 62.5 MB

Start, until the note list shows: 1 time; median of the last ones 1696 ms, slowest 1696 ms
  1696 ms

Opening the vault: 1 time; median of the last ones 15 ms, slowest 15 ms
  15 ms

Index sync: 3 times; median of the last ones 47 ms, slowest 201 ms (10000 notes, 0 read, 0 removed)
  43 ms (10000 notes, 0 read, 0 removed)
  47 ms (10000 notes, 0 read, 0 removed)
  201 ms (10000 notes, 0 read, 0 removed)

All notes: 5 times; median of the last ones 898 ms, slowest 981 ms (10000 notes)
  172 ms (10000 notes)
  159 ms (10000 notes)
  898 ms (10000 notes)
  918 ms (10000 notes)
  981 ms (10000 notes)

All notes, until shown: 5 times; median of the last ones 1101 ms, slowest 1460 ms (10000 notes)
  307 ms (10000 notes)
  322 ms (10000 notes)
  1267 ms (10000 notes)
  1101 ms (10000 notes)
  1460 ms (10000 notes)

Tag folders: 16 times; median of the last ones 23 ms, slowest 47 ms (22 folders)
  36 ms (21 folders)
  21 ms (23 folders)
  21 ms (22 folders)
  22 ms (23 folders)
  25 ms (22 folders)
  19 ms (23 folders)
  20 ms (22 folders)
  19 ms (23 folders)
  26 ms (22 folders)
  19 ms (23 folders)
  27 ms (22 folders)
  24 ms (23 folders)
  31 ms (22 folders)
  36 ms (23 folders)
  22 ms (22 folders)
  47 ms (22 folders)

Notes of a tag folder: 14 times; median of the last ones 4.2 ms, slowest 39 ms (1971 notes)
  2.4 ms (28 notes)
  2.3 ms (28 notes)
  20 ms (28 notes)
  4.6 ms (28 notes)
  1.3 ms (28 notes)
  10 ms (1971 notes)
  39 ms (1971 notes)
  11 ms (1971 notes)
  9.1 ms (1971 notes)
  9.4 ms (1971 notes)
  2.9 ms (1971 notes)
  3.9 ms (2347 notes)
  2.1 ms (651 notes)
  2.2 ms (1870 notes)

Notes of a tag folder, until shown: 14 times; median of the last ones 34 ms, slowest 64 ms (1971 notes)
  20 ms (28 notes)
  28 ms (28 notes)
  30 ms (28 notes)
  30 ms (28 notes)
  11 ms (28 notes)
  54 ms (1971 notes)
  64 ms (1971 notes)
  55 ms (1971 notes)
  38 ms (1971 notes)
  38 ms (1971 notes)
  29 ms (1971 notes)
  40 ms (2347 notes)
  30 ms (651 notes)
  39 ms (1870 notes)

Notes of a smart folder: 9 times; median of the last ones 15 ms, slowest 28 ms (200 notes)
  12 ms (200 notes)
  11 ms (200 notes)
  11 ms (200 notes)
  15 ms (200 notes)
  12 ms (200 notes)
  28 ms (200 notes)
  25 ms (200 notes)
  25 ms (200 notes)
  18 ms (200 notes)

Notes of a smart folder, until shown: 9 times; median of the last ones 31 ms, slowest 55 ms (200 notes)
  27 ms (200 notes)
  23 ms (200 notes)
  18 ms (200 notes)
  22 ms (200 notes)
  35 ms (200 notes)
  55 ms (200 notes)
  38 ms (200 notes)
  37 ms (200 notes)
  31 ms (200 notes)

Search: 36 times; median of the last ones 26 ms, slowest 61 ms (200 notes, for “antenne OR kirche O”)
  10.0 ms (200 notes, for “-#radio”)
  28 ms (200 notes, for “(spule OR dipol) #radio/cw”)
  27 ms (200 notes, for “#projekte kabel -#archiv”)
  30 ms (200 notes, for “antenne OR kirche OR gitarre”)
  32 ms (200 notes, for “antenne OR kirche OR gita”)
  43 ms (200 notes, for “antenne OR kirche OR gi”)
  55 ms (200 notes, for “antenne OR kirche OR g”)
  25 ms (200 notes, for “antenne OR kirche OR”)
  26 ms (200 notes, for “antenne OR kirche OR”)
  61 ms (200 notes, for “antenne OR kirche O”)
  26 ms (200 notes, for “antenne OR kirche”)
  26 ms (200 notes, for “antenne OR kirche”)
  31 ms (200 notes, for “antenne OR kir”)
  21 ms (200 notes, for “antenne OR”)
  18 ms (200 notes, for “antenne OR”)
  53 ms (200 notes, for “antenne O”)
  20 ms (200 notes, for “antenne”)
  19 ms (200 notes, for “antenne”)
  9.5 ms (127 notes, for “"radio amateur"”)
  13 ms (127 notes, for “"radio amateur”)

Search, until shown: 36 times; median of the last ones 36 ms, slowest 69 ms (200 notes)
  28 ms (200 notes)
  42 ms (200 notes)
  40 ms (200 notes)
  36 ms (200 notes)
  40 ms (200 notes)
  58 ms (200 notes)
  63 ms (200 notes)
  30 ms (200 notes)
  32 ms (200 notes)
  69 ms (200 notes)
  36 ms (200 notes)
  42 ms (200 notes)
  40 ms (200 notes)
  26 ms (200 notes)
  24 ms (200 notes)
  62 ms (200 notes)
  25 ms (200 notes)
  35 ms (200 notes)
  17 ms (127 notes)
  27 ms (127 notes)

Reading a note: 5 times; median of the last ones 1428 ms, slowest 1682 ms (3 KB)
  1437 ms (2 KB)
  1425 ms (3 KB)
  1427 ms (2 KB)
  1428 ms (3 KB)
  1682 ms (3 KB)

Opening a note, until shown: 5 times; median of the last ones 1434 ms, slowest 1706 ms (3 KB)
  1441 ms (2 KB)
  1430 ms (3 KB)
  1431 ms (2 KB)
  1434 ms (3 KB)
  1706 ms (3 KB)

Export: 2 times; median of the last ones 215 ms, slowest 389 ms (PDF, 28 notes)
  389 ms (PDF, 28 notes)
  41 ms (HTML, 28 notes)
```

</details>
