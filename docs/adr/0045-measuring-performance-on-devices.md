# ADR-0045: Measuring performance on devices

- Status: Accepted
- Date: 2026-11-23
- Deciders: Willi Kraml
- Related: N-03, N-02, ADR-0002, ADR-0009, ADR-0036, ADR-0039

## Context

N-03 asks for search under 200 ms with 10,000 notes. The M7 roadmap asks for more: 10,000 notes on the oldest iPhone we support and on an Intel Mac if we build for it, with start time, memory, search and export times written down, and the editor loaded on demand if start time needs it.

What exists measures only the core: `crates/core/tests/perf.rs` builds the index of 20,000 generated notes and times searches, tag folders and note lists, in a CI job on a Mac. Nothing measures the app: not its start, not the round trip from the web view, not memory, and not an iPhone.

Two limits are given:

- The only iPhone at hand is an iPhone 18 Pro. The oldest iPhone iOS 16 supports (ADR-0002) is the iPhone 8 or X (A11), several times slower.
- The Mac 1.0 build is for Apple silicon only (decided on 2026-11-23, below), so there is no Intel Mac to test.

## Decision

**The app measures its own times and memory and shows them in Settings › Diagnostics. A command makes a test vault of generated notes, which reaches the iPhone through iCloud. We measure on the owner's Mac and iPhone 18 Pro, and estimate the oldest supported iPhone from published benchmarks.**

1. **Mac build:** Apple silicon only (`aarch64-apple-darwin`) for 1.0. No universal build, no Intel test. This settles the open question in the roadmap's macOS packaging item.
2. **Test vault:** `cargo run -p schnipsle-core --release --example make-vault -- <folder> [notes] [seed]` makes a new vault in an empty folder with 10,000 notes by default. It uses the generator of the core performance test, so the text is invented and the same for a given seed. The folder goes into iCloud Drive, and the iPhone opens it once it has synced. The command refuses a folder that is not empty.
3. **What the app measures:** the app shell times these steps with a monotonic clock:
   - the start, from the process start to the first note list shown (the UI reports when it has drawn it);
   - opening the vault, and the index sync after it, with the number of notes;
   - each search and each note list of a tag folder or smart folder, in the core and as the UI sees it;
   - opening a note;
   - each export, with its format and number of notes.

   It also reads the app's memory: the physical footprint, now and at its peak, from `task_info` in `schnipsle-apple` (ADR-0015).
4. **Where the measurements go:** they are kept in memory only, the last 20 of each kind with the slowest. Settings › Diagnostics shows them, on the Mac and the iPhone, with a Copy button that gives them as text for the report. Nothing is written to disk and nothing is sent (N-02). There is no switch, since measuring costs a clock read per step.
5. **The oldest iPhone:** estimated, not measured. Each time measured on the iPhone 18 Pro is multiplied by the ratio of the Geekbench 6 single-core scores of the two chips, then by 1.5 as a margin for slower storage and less memory. A measured time stays below its limit only if its estimate does too. The report says which numbers are estimates and from which scores.
6. **Limits:**
   - Search and a note list: under 200 ms (N-03).
   - Opening a note: under 100 ms.
   - The start with an indexed vault: under 1.5 s on the iPhone and under 1 s on the Mac.
   - Building the index the first time, and exports: written down, without a limit.

   If the start misses its limit, the editor is loaded on demand, as the roadmap foresees.
7. **The report:** `docs/design/performance.md` holds how to run it and the results per device and date.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| A: a Diagnostics view in the app (chosen) | The same on the Mac and the iPhone; no Xcode; anyone can copy the numbers into a report; it also helps with later bug reports. | Code in the shell and the UI; one more section in Settings. |
| B: a log file in the app's folder | Little UI. | It has to be fetched from the iPhone; disk writes for every step. |
| C: external tools only (Instruments, Activity Monitor) | No code. | Start and search round trips are hard to read off; each run is manual work in Xcode. |
| D: the test vault generated on the iPhone | Quick to set up. | Needs a hidden command in the app; misses what iCloud does with 10,000 files. |
| E: buy or borrow an iPhone 8 or X | Real numbers. | Not at hand; estimates with a margin are enough to see whether we are near a limit. |

## Consequences

- Positive: start, search, note opening, export and memory are measured on real devices with real file access, and the numbers can be measured again for every release.
- Negative: the oldest iPhone is estimated, not measured. One more section in Settings, and a little code in every measured command.
- Follow-up work:
  - the `make-vault` example;
  - the measuring in `src-tauri` and memory in `schnipsle-apple`;
  - Settings › Diagnostics;
  - `docs/design/performance.md`;
  - the runs on the Mac and the iPhone;
  - lazy loading of the editor if the start needs it.
