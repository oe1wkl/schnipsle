# ADR-0051: The manual links its notes by relative links, and is built into the app

- Status: Proposed
- Date: 2026-11-27
- Deciders: Willi Kraml
- Related: F-12, ADR-0016, ADR-0021, ADR-0048, ADR-0049

## Context

ADR-0048 rule 6 has the manual notes link to each other as `[[Title]]` (format.md §5.4), and rule 9 accepts that GitHub shows such links as text. It also has the notes copied into the app as Tauri resources.

While the manual was being moved into a vault on 2026-11-27, two things came up:

- The app does not follow `[[Title]]` links yet: ⌘-click opens web addresses, files and relative links to other notes (`[Music](music.md)`), but note links by title come with F-12, which is not in 1.0. The book note "Schnipsle manual" (ADR-0049), which Help › Manual opens, would be a list of titles that cannot be clicked.
- Tauri resources are files beside the app, found at run time by a path that differs on the Mac and the iPhone, and the iPhone build copies them through the generated Xcode project. The manual is small (text now, a few pictures later).

The owner chose relative links on 2026-11-27.

## Decision

**Manual notes link to each other with relative Markdown links, and the app carries the manual inside its program file.**

1. **Links** (amends ADR-0048 rules 6 and 9). A link from one manual note to another is an ordinary relative link to its file, such as `[Music](music.md)` or `[Query language](query-language.md#songs-and-scores)`. All manual notes are in one folder (`notes/schnipsle-manual/`), in the repository and in the vault, so the links are the same in both. Pictures are linked as format.md §5.5 says (`../../attachments/…`). `[[Title]]` is not used in the manual until the app follows it (F-12).
2. **Built in** (amends ADR-0048 rule 6, "Tauri resources"). The app's build script (`src-tauri/build.rs`) lists the files in `docs/manual/notes/` and `docs/manual/attachments/` and includes them in the program; `schnipsle_core::manual::Manual` reads them from there. Nothing is looked up on disk at run time.

The rest of ADR-0048 stands. No change to the vault format.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Relative links (chosen) | Open in the app today, are clickable on GitHub, and exports and books resolve them (ADR-0021 rule 7, ADR-0049 rule 2) | Break if a manual note's file is renamed; the app renames a file when its title changes (format.md §3), but the manual's titles and names already match |
| `[[Title]]`, made clickable now | As ADR-0048 says; survives renames | Part of F-12 pulled into 1.0; plain text on GitHub |
| `[[Title]]`, not clickable until F-12 | As ADR-0048 says, no extra work | The manual's book note cannot be used as a table of contents in the app |
| Built into the program (chosen) | Same on every platform; no path to find; the core's tests use the same files | The program grows by the manual's size; a new manual needs a new build anyway |
| Tauri resources (ADR-0048) | Files can be replaced without rebuilding | A path per platform; the Xcode project has to copy them |

## Consequences

- Positive: Help › Manual opens a note whose links lead through the manual, in the app and on GitHub.
- Negative: a user who renames a manual note's title also renames its file, and links to it from other manual notes stop working until F-12 brings note links that follow renames. `docs/manual/README.md` sits outside `notes/`, so a copy of the repository's manual opened as a vault offers to move it in (stray notes); that copy is for writing the manual, not for reading it.
- Follow-up work: when F-12 comes, decide whether the manual moves to `[[Title]]`.
