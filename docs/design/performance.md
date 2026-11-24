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
