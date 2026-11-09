# ADR-0030: Rendering and transposing song sheets

- Status: Accepted; how sheets look in ODT and DOCX exports amended by [ADR-0034](0034-song-sheets-in-exports.md)
- Date: 2026-11-07
- Deciders: Willi Kraml
- Related: F-18, ADR-0014, [format.md §5.7](../format.md#57-lyrics-with-chords)

## Context

ADR-0014 puts the ChordPro renderer and transposer into `schnipsle-core`, shared by the editor and the exports. Format.md §5.7 leaves open how the pieces work together and how some cases behave:

- how the transposition of the info string, the viewer and `{transpose}` lines adds up;
- which key "sharps or flats follow the target key" refers to when a song has no `{key}`;
- how German names read `Bb` or `Hb`;
- what `{chorus}` shows after a `{transpose}`;
- what lyrics only leaves out;
- what "Apply transposition" rewrites;
- the name of the vault default of rule 6 in `settings.yaml`.

## Decision

1. **One model.** `chordpro::sheet::render` turns a block into a sheet: the song's details (title, subtitle, artist, composer, lyricist, key, capo, tempo, time), then sections of lines.
   - **Sections** are verse, chorus, bridge, tab, or lines outside any of them.
   - **Lines** are lyrics with each chord over the start of its text, chord lines, comments, tablature, unknown directives as written, and empty lines.
   - **Comments:** ChordPro comments are left out.
   - **Use:** the editor gets the model as JSON through a command; the exporters use it directly.
2. **Transposition** is the sum of three amounts:
   - the info string's `transpose=`;
   - the viewer's control;
   - the last `{transpose: n}` above the line, which applies from its line on (`{transpose}` without a value sets it back to 0).

   The shown `{key}` moves with the chords. A whole number of octaves leaves every chord as written.
3. **Spelling.** A moved chord is named to suit the key it is moved to.
   - **Which key:** the `{key}` that applies there, else the song's first chord, moved by the same amount.
   - **Sharp and flat keys:** sharp keys (G, D, A, E, B, F# and their relative minors) use sharps, flat keys (F, B♭, E♭, A♭, D♭) use flats.
   - **Enharmonic keys:** of two that sound alike, the one with fewer accidentals is chosen, and F# rather than G♭ where both have six.
   - **C major and A minor:** C#, E♭, F#, A♭, B♭.
   - **Kept as written:** the quality and extensions (`maj7(#11)`), and a root in lower case.
   - **What is not a chord** (`N.C.`, `x`) stays as written.
4. **German names** (`notation=german` or the vault default): `H` is B, `B` is B♭.
   - `Bb` and `Hb` are read as B♭ too, as song sheets write them so.
   - A moved B♭ is shown as `B` and B as `H`.
   - `H` is read as B in English as well, as it means nothing else there.
   - A key in lower case (`{key: e}`) or a minor quality (`m`, `min`, `-`) is minor.
5. **The vault default** is `chord_notation: english|german` in `.notesapp/settings.yaml`. Anything else counts as English. A block's `notation=` wins.
6. **`{chorus}`** shows the last chorus again, with its label unless the directive has one. Its chords move as the lines around the `{chorus}` do, so a chorus after a key change is sung in the new key.
7. **Lyrics only** (`nochords` or the viewer's choice) leaves out the chords, chord lines and tablature. Comments and the song's details stay.
8. **"Apply transposition"** (`apply_transposition`) rewrites only the chords in brackets and the `{key}` values, spelled as above. Everything else stays byte for byte, including `{transpose}` lines, tablature, comments and line ends. The editor stores a history snapshot first (rule 5).

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Model in core, view in TypeScript (chosen) | One renderer for preview and exports, tested with golden files | A command per block for the editor |
| Render to HTML in core | The editor shows a string | Exports to ODT and DOCX need the model anyway |
| Spell by key (chosen) | Reads as musicians write: B♭ in F, A# nowhere near it | The key has to be known or guessed |
| Keep the accidentals as written | Simple | `Bb` +2 gives `C`, but `A#` +1 gives `B` next to `Eb` |
| `{chorus}` moved as at its place (chosen) | Right after a key change | The chorus differs from the written one |
| `{chorus}` as it was written | A literal repeat | Wrong key after `{transpose}` |

## Consequences

- **Positive:**
  - Song sheets look the same in the editor and every export.
  - Transposition is predictable and tested in both namings.
  - The file changes only on "Apply transposition".
- **Negative:**
  - A song without `{key}` is spelled for its first chord. That is usually the key, not always.
  - Rare spellings (C♭, E#, double accidentals) are never produced.
- **Follow-up work:**
  - The editor view and "Apply transposition" with a snapshot.
  - Song sheets in the exports.
  - format.md 0.15 names the setting.
