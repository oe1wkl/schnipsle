# Music

A note can hold music of two kinds, each in a fenced code block:

- **Scores** in ABC notation: tunes, choir parts and exercises, with notes on staves. Schnipsle engraves them in the note and plays them.
- **Song sheets** in ChordPro: lyrics with chords above the syllables, for singing and playing along.

Both stay plain text in the note's file (format.md §5.6 and §5.7), so they can be searched, synced and seen in the history like any other text.

## Scores

A score is an `abc` block:

````markdown
```abc
X:1
T:Kanon
C:Johann Pachelbel
M:4/4
L:1/4
Q:1/4=60
K:D
f2 e2 | d2 c2 | B2 A2 | B2 c2 |]
w:Lob und Dank sei dir, o Herr
```
````

The note shows the engraved score. Put the cursor in the block to see and edit its ABC; the score comes back when the cursor leaves it. A click on a note, rest or bar line puts the cursor on it in the ABC. Each line of notes is a line of the score, as you wrote it.

If the block has something Schnipsle cannot engrave, the block shows its ABC with the problems underneath, each with its line and column. Click a problem to go there. Nothing is engraved until every problem is fixed, so the score never leaves out part of what the ABC says.

One block can hold several tunes, one after the other, each starting with its own `X:` line. A short fragment needs no header: `K:G` and a line of notes are enough.

### The Music menu

**Music** in the bar under the note writes the ABC that people most often look up:

- **New score**, with the cursor outside a score: a melody with chords, two voices, or a choir score for soprano, alto, tenor and bass. The score comes after the paragraph the cursor is in, with its title selected so you can type over it.
- **Key** and **Metre**, with the cursor in a score: they set the `K:` and `M:` lines of the tune the cursor is in. The current ones are highlighted. A clef or anything else on the `K:` line stays.
- **Add a voice**: a new voice at the end of the tune, with the clef you choose and one bar's rest to start from. A tune without voices gets `V:1` before its notes, and a `%%score` line gets the new voice, so it shows.
- **Lyrics line**: a `w:` line under the line of notes the cursor is in.
- **Chord symbol**: `"C"` at the cursor, with the `C` selected to type over.
- **Another tune**: a new tune at the end of the block, numbered after the others.

Each choice is one ordinary change of the note, undone with ⌘Z like typing. The menu also links to the ABC standard.

### ABC in brief

This covers what most notes need. The full language is in the [ABC standard 2.1](https://abcnotation.com/wiki/abc:standard:v2.1).

**The header** comes first, one field per line. `X:` starts a tune and `K:` ends its header.

| Field | Meaning                                | Example                                  |
| ----- | -------------------------------------- | ---------------------------------------- |
| `X:`  | number of the tune                     | `X:1`                                    |
| `T:`  | title                                  | `T:Abendlied`                            |
| `C:`  | composer                               | `C:Trad.`                                |
| `M:`  | metre; `C` is 4/4, `C\|` is 2/2        | `M:6/8`                                  |
| `L:`  | the length a plain note has            | `L:1/8`                                  |
| `Q:`  | tempo: beats per minute                | `Q:1/4=100`                              |
| `K:`  | key, optionally with a mode and a clef | `K:G`, `K:Am`, `K:Ddor`, `K:F clef=bass` |
| `V:`  | a voice, with its name and clef        | `V:T name="Tenor" clef=treble-8`         |
| `W:`  | words printed after the tune           | `W:Second verse …`                       |
| `%`   | a comment, to the end of the line      | `% check bar 3`                          |

**Notes** are letters: `C D E F G A B` start at middle C, `c d e f g a b` are the octave above. A comma takes a note an octave lower (`C,`), an apostrophe an octave higher (`c'`).

| Write                         | Means                                                           |
| ----------------------------- | --------------------------------------------------------------- |
| `A2`, `A/2` (or `A/`), `A3/2` | twice, half, one and a half times the length of `L:`            |
| `A>B`                         | dotted rhythm: the first note longer, the second shorter        |
| `z`, `z2`, `Z`                | rests; `Z` is a whole bar                                       |
| `^F`, `_B`, `=F`              | sharp, flat, natural                                            |
| `\|`, `\|\|`, `\|]`           | bar line, double bar line, final bar line                       |
| `\|:` … `:\|`                 | repeat; `[1` and `[2` mark the first and second ending          |
| `[CEG]`                       | notes played together                                           |
| `A-A`                         | tie                                                             |
| `(AB)`                        | slur                                                            |
| `(3ABc`                       | triplet                                                         |
| `{g}A`                        | grace note                                                      |
| `!fermata!A`, `!p!`           | decorations and dynamics                                        |
| `"Am"A`                       | a chord symbol over the note; it is also played if Chords is on |
| `"^rit."A`                    | text over the note                                              |

**Lyrics** go in a `w:` line under the line of notes: one syllable per note, `-` between the syllables of a word, `_` to hold a syllable over the next note, `*` to skip a note. Several `w:` lines give several verses.

```abc
X:1
T:Kuckuck
M:3/4
L:1/4
K:F
c A z | c A z | G F G | F2 z |]
w:Ku-ckuck, ku-ckuck, ruft's aus dem Wald
```

**Voices**: every `V:` line starts a voice, and the notes after it belong to that voice until the next `V:` line. Clefs are `treble`, `bass`, `alto`, `tenor` and `treble-8` (a tenor part, written in the treble clef an octave higher than it sounds). A `%%score` line in the header lists the voices to show: `%%score [S A T B]` draws four staves joined by a bracket, `%%score (S A) (T B)` puts two voices on each staff.

**Umlauts** and other letters can be written as they are (`ü`), or as ABC writes them: `\"u`, `&uuml;` or `ü`.

### Options

Options go after `abc` on the opening line of the block:

- `transpose=<semitones>` moves the score and its sound, e.g. ` ```abc transpose=-2 ` one tone down. The ABC in the note stays as written.
- `nosource` keeps the score engraved in read-only views, such as an old version in the history.

### Playing a score

A bar over each score plays it with a piano that comes with the app, so it plays offline:

- **▶ Play** and **■ Stop**, one button for each tune of the block.
- **Tempo** slower or faster, from 30 to 200 percent of the tune's own tempo.
- **Transpose** moves the score and its sound by semitones.
- **M** mutes a voice, **S** plays it alone (or together with the other voices set to S), for practising one part.
- **Chords** plays an accompaniment to the chord symbols.

These settings only change what you see and hear, never the note. They last until you open another note. Editing the score stops it playing.

## Song sheets

A song sheet is a `chordpro` block. A chord goes in square brackets right before the syllable it belongs over:

````markdown
```chordpro
{title: Abendlied}
{key: G}

{start_of_verse}
Der [G]Mond ist aufge[D]gangen,
die [G]goldnen Sternlein [C]pran[D]gen
{end_of_verse}

{start_of_chorus}
[Em]Lalala [C]lala, [D7]lala [G]la
{end_of_chorus}
```
````

The note shows the sheet with the chords over the lyrics; with the cursor in the block it shows the ChordPro. Verses, choruses and bridges are set off by `{start_of_verse}` … `{end_of_verse}` and their pairs, optionally with a label: `{start_of_verse: Strophe 2}`. `{comment: …}` adds a remark, `{chorus}` repeats the last chorus. Directives Schnipsle does not know are kept and shown dimmed. The full list is in format.md §5.7 and on [chordpro.org](https://www.chordpro.org/chordpro/chordpro-directives/).

A bar over each sheet changes how it is shown, never the note:

- **Transpose** moves the chords by semitones. **Apply transposition** then writes the moved chords into the note; the version before is kept in the history.
- **Chord names** in English (B, B♭) or German (H, B).
- **Lyrics only** hides the chords, e.g. for singers.

Options after `chordpro` on the opening line: `transpose=<semitones>`, `notation=german` for German chord names (`H` is B, `B` is B♭), `nochords` for lyrics only, and `nosource`. For German names in every song of a vault, put `chord_notation: german` in `.notesapp/settings.yaml`.

## Finding music

Search finds scores by their titles, composers and lyrics, and song sheets by their lyrics, title and people; `has:chord` and `chord:Am` find songs by their chords. See [Query language](query-language.md#songs-and-scores).

## Exporting

Scores and song sheets are exported as they look in the note:

- **HTML and PDF:** scores as sharp drawings, song sheets as in the editor.
- **ODT and DOCX:** scores as pictures; song sheets with each line of chords over its line of lyrics in a monospace font, so they stay aligned in any word processor.
- **EPUB:** scores as pictures, song sheets as in the editor.

**Add the ABC source under each score** in the export dialog also puts each score's ABC under it, so the text can be copied from the export.

## Larger scores

Scores from notation programs, such as MusicXML (`.musicxml`, `.mxl`), MIDI (`.mid`), LilyPond (`.ly`) or PDF, can be dragged into a note on the Mac: Schnipsle keeps the file in the vault and links it. ⌘-click the link to open the file. It is not shown in the note itself.
