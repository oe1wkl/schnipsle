# ADR-0035: Playing scores

- Status: Accepted
- Date: 2026-11-10
- Deciders: Willi Kraml
- Related: F-17, N-02, ADR-0005, ADR-0013, ADR-0032, [format.md §5.6](../format.md#56-music-notation)

## Context

ADR-0013 plays an `abc` block with abcjs's Web Audio synthesizer, with a tempo control and per-voice mute and solo, for practising a choir part. ADR-0032 rule 6 left the controls of a score until playback, so they would be designed together.

abcjs plays from sound-font samples, one MP3 per note and instrument, which by default it fetches from a web site, and it picks each voice's instrument from the tune's `%%MIDI program`. The app must work offline (N-02), and its licences must allow the App Store (ADR-0005).

Two more things found while trying it:

- abcjs plays a visual transposition at concert pitch, as for a transposing instrument. Rule 6 of §5.6, however, moves the sound with `transpose=`.
- The app's content-security policy allowed no requests to its own files (`connect-src`), which abcjs uses to load the samples.

## Decision

1. **Sounds.** The app brings along the acoustic grand piano of the Fluid (R3) General MIDI SoundFont, as MIDI.js Soundfonts converts it to MP3. That is 88 files and 2.1 MB in `ui/public/sounds/`. The licence is Creative Commons Attribution 3.0; `NOTICE` and the folder's README give the attribution.
   - Every voice and the chord accompaniment play on that piano, whatever `%%MIDI program` says.
   - Drums are left out.
   - Nothing is loaded from the web.
2. **Controls.** A bar above each engraved score has these controls:
   - **Play / Stop:** one per tune, named after the tune when a block has several.
   - **Tempo:** in percent of the tune's own tempo, from 30 to 200 in steps of 10.
   - **Transpose:** semitones added to `transpose=`, which move the score and the sound.
   - **Mute (M) and Solo (S) for each voice,** when a tune has more than one. With any voice solo, only solo voices play, and a muted voice never plays.
   - **Chords:** switches the accompaniment abcjs plays to the chord symbols, when the tune has any.

   These are view settings of the block, as a song sheet's (ADR-0031). The note never changes, and the settings last until another note opens.
3. **One tune plays at a time** in the window. Starting another stops it.
   - A change of tempo, mute, solo or chords while a tune plays goes on from where it was.
   - A change of transposition, or an edit of the block, stops it, as does closing the note.
4. **Pitch.** The sound is moved by the same semitones as the score (`midiTranspose`), so the score shows what is heard.
5. **iPhone.** On Play, the page declares that it plays music (`navigator.audioSession.type = "playback"`, iOS 16.4 and later), so the ring/silent switch does not mute it.
6. **Content-security policy.** `connect-src` gets `'self'`, so the page can load the app's own files. Nothing outside the app is allowed.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Piano samples in the app (chosen) | Works offline; a clear sound for practising parts; abcjs plays as designed | 2.1 MB in the app; one instrument; attribution needed |
| Piano and choir samples | Voices sound sung | 4.1 MB; a choir sound blurs fast notes |
| Samples from the web (abcjs's default) | Nothing to bring along; every instrument | Breaks N-02; sends what is played to a web site |
| A tone made by the app | No files, no licence | A plain sound; a synthesizer of our own to write and keep |
| abcjs's own controls (`SynthController`) | Less code | No mute or solo; its look and words are abcjs's, not the app's |

## Consequences

- Positive: a choir part can be practised alone or against the others, slower and in another key, offline on the Mac and the iPhone.
- Negative:
  - Every voice sounds like a piano.
  - On iOS 16.0–16.3 the silent switch still mutes playback.
  - Playing does not yet mark the note it is at.
- Follow-up work:
  - A check on the iPhone with the silent switch on.
  - Marking the note that plays, if wanted.
  - More instruments, if wanted (licence and size to weigh).
  - ADR-0032 rule 6 is settled by this.
