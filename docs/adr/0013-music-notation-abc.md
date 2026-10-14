# ADR-0013: Music notation as ABC in fenced code blocks

- Status: Accepted
- Date: 2026-10-14
- Deciders: Willi Kraml
- Related: F-16, F-17, ADR-0004, ADR-0007, ADR-0010, [format.md §5.6](../format.md#56-music-notation)

## Context

Notes should be able to contain music: tunes, melodies, choir parts, exercises. The notation has to fit the plain-text principle (ADR-0004): readable and writable by hand in a Markdown file, small, diff-able in the version history. It must render on macOS and iOS inside the WebView editor (ADR-0003), with no external program (iOS restriction, ADR-0010), under a licence compatible with Apache-2.0 and the App Store (ADR-0005).

## Decision

- Music is written in **ABC notation 2.1** in fenced code blocks with the info string `abc`, as specified in `format.md` §5.6.
- Rendering uses **abcjs** (MIT licence) in the WebView: engraved SVG in the live preview (ADR-0007), with the source shown while the cursor is in the block.
- **Playback** through abcjs's Web Audio synthesizer, per block, with tempo control and per-voice mute/solo (useful for practising a single choir part).
- **Export** (ADR-0010): the block is rendered to SVG once in the export pipeline and embedded as an image in HTML, PDF and ODT. For the optional Pandoc path, the SVG is substituted before Pandoc runs. An option adds the ABC source as an appendix or attachment.
- Titles, composers and lyrics inside ABC blocks are indexed for full-text search.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| ABC + abcjs (chosen) | Compact plain text, writable by hand, large existing tune collections, multiple voices and lyrics, MIT renderer that runs in the WebView, playback included | Engraving quality below LilyPond; complex orchestral scores are awkward |
| LilyPond | Best engraving quality | GPL; needs a native executable, cannot run on iOS; slow to render |
| MusicXML inline | Standard exchange format | Verbose XML, not writable by hand, unreadable in a note |
| Verovio (MEI / Humdrum / MusicXML) | High-quality engraving, WebAssembly build, can also read ABC | LGPL-3.0 and a large WASM bundle; MEI/Humdrum not hand-friendly |
| ChordPro | Ideal for lyrics with chord symbols | Not music notation; solves a different need |

## Consequences

- Positive: music notes stay plain text, searchable by title and lyrics, versioned like any note, and play back on every platform.
- Negative: users must learn ABC; mitigated by a snippet menu (key, metre, voice templates) and a link to the standard in the manual.
- Follow-up:
  - Golden-file tests for rendering (SVG snapshot) and indexing.
  - Verovio may be added later as an optional renderer for MusicXML attachments; that needs its own ADR because of LGPL.
  - Lyrics with chords are handled separately in ChordPro blocks (ADR-0014).
