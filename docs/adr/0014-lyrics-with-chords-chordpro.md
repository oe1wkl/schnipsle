# ADR-0014: Lyrics with chords as ChordPro in fenced code blocks

- Status: Accepted
- Date: 2026-10-14
- Deciders: Willi Kraml
- Related: F-18, ADR-0005, ADR-0013, [format.md §5.7](../format.md#57-lyrics-with-chords)

## Context

Besides full scores (ADR-0013), notes need songs as lyrics with chord symbols: song sheets for singing and accompaniment. ABC can carry chord symbols, but forces a melody line; most song sheets have none. The notation must be plain text, render on iOS without external programs, support transposition, and fit the Apache-2.0 licence.

## Decision

- Song sheets are written in **ChordPro** inside fenced code blocks with the info string `chordpro`, as specified in `format.md` §5.7: inline chords in brackets, the common directives, sections (verse, chorus, bridge, tab).
- **Own renderer**, written in Rust in `crates/core` (parser + chord model + transposer) with a small TypeScript view for the editor. The parser output is shared by the editor preview, all exporters and the indexer.
- **Transposition** is a view setting by default; rewriting the file is an explicit action with a history snapshot.
- **German note names** (`H`, `B`) are supported as an option, per block or per vault.
- `.cho` / `.chopro` / `.chordpro` files can be imported.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| ChordPro, own renderer (chosen) | De-facto standard for song sheets; plain text; small subset is easy to implement; licence-clean; one parser for preview, export, index | Implementation and test effort |
| ChordPro with ChordSheetJS | Mature parser and formatters | GPL-2.0-only ([package.json](https://cdn.jsdelivr.net/npm/chordsheetjs@15.2.0/package.json)), incompatible with Apache-2.0 / App Store (ADR-0005) |
| Chords over lyrics as plain text lines | No syntax | Alignment breaks with proportional fonts; cannot be transposed or reflowed reliably |
| Chord symbols in ABC | One notation for all music | Requires a melody; not how song sheets are written |

## Consequences

- Positive: song sheets are searchable by title, artist and lyrics, filterable by chord, transposable, and exported with correct chord placement in HTML, PDF and ODT.
- Negative: the subset of ChordPro must be documented and grown carefully; chord diagrams (`{define}`) and multi-column layout are not in format version 1.
- Follow-up: golden-file tests for parsing, transposition (English and German naming, sharp/flat choice by key) and rendering; chord diagrams as a later extension.
