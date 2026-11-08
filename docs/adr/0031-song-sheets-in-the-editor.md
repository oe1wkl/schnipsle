# ADR-0031: Song sheets in the editor

- Status: Accepted
- Date: 2026-11-08
- Deciders: Willi Kraml
- Related: F-18, ADR-0007, ADR-0011, ADR-0014, ADR-0030, [format.md §5.7](../format.md#57-lyrics-with-chords)

## Context

ADR-0030 renders a `chordpro` block in `schnipsle-core` and leaves to the editor how it shows the sheet and its controls. The M6 roadmap asks for a transpose control, the chord names and lyrics only per block as view settings, and "Apply transposition" after a history snapshot (§5.7 rule 5).

In ADR-0030 one naming decides both how a chord is read and how it is shown: the block's `notation=`, else the vault default. A control that switched that naming would misread the song: a `B` in a German sheet is B♭, in an English one B.

## Decision

1. **Live preview.** A `chordpro` block shows as its sheet, with each chord over its syllable, unless the cursor is in the block while the editor has focus; then its source shows, as with tables (ADR-0007). A click into the sheet puts the cursor there. Source mode shows the source.
2. **Controls** above the sheet: transpose down and up by a semitone, with the semitones added; the chord names shown; lyrics only. They change only the view, never the note. The settings belong to the block and last until another note is opened. A block with `nochords` shows lyrics only, and the control cannot turn the chords on.
3. **Names shown apart from names written.** Chords are read in the block's or the vault's naming and shown in the one the viewer chooses (`View::shown`). Without transposition, each note keeps the sharp or flat it is written with: English `Bb` shows as German `B`, `A#` stays `A#`. With transposition, chords are spelled for the new key as in ADR-0030.
4. **"Apply transposition"** shows while the viewer's transposition is not a whole number of octaves and the note can be written. The app saves the note and stores it in its history (ADR-0011), then the core rewrites the block with the viewer's semitones in the naming the block is written in (`apply_transposition`); the block's `transpose=` and `{transpose}` lines stay. The rewrite is an edit like any other, so Undo takes it back. If the note could not be saved or stored, or the block changed meanwhile, nothing is rewritten and the status line says why.
5. **Commands.** `render_song` returns the sheet of a block with the vault default naming; `apply_transposition` returns the rewritten block. The editor keeps the sheets it rendered lately, so a block shows at once when the cursor leaves it.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Names shown chosen apart from names written (chosen) | A German sheet can be read in English names and the other way round, nothing misread | One more setting in the model |
| A control that sets the naming the block is read in | No change to the core | Misreads every `B` and `H` of the song |
| No naming control | Nothing to choose | The roadmap asks for it; singers who know only one naming cannot switch |
| View settings for as long as the note is open (chosen) | Simple; nothing stored | Gone when another note opens |
| View settings stored per note (e.g. in the vault's app data) | Kept across notes and devices | A new file or format change for a view setting |

## Consequences

- Positive: song sheets are readable in the editor on the Mac and the iPhone, transposed and in either naming, while the file stays as written until the user applies a transposition.
- Negative: view settings are lost when another note opens; a block is rendered by a command each time its source or view changes.
- Follow-up work: the exports take the same model (M6); view settings kept across notes, if wanted. ADR-0030 is amended by this.
