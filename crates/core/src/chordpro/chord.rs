// SPDX-License-Identifier: Apache-2.0
//! Chords and their transposition (format.md §5.7 rules 4 to 6, ADR-0030).
//!
//! A chord is read with the block's [`Notation`]: in German `B` is B♭ and `H`
//! is B; `H` is read as B in English too, as it means nothing else. A
//! transposed chord is spelled to suit the key it is transposed to, with
//! sharps in sharp keys and flats in flat keys; in C major and A minor `C#`,
//! `Eb`, `F#`, `Ab` and `Bb`. The quality and extensions stay as written,
//! and so does a root written in lower case, which German song sheets use
//! for minor chords.

use serde::Serialize;

use crate::vault::Vault;

/// The names of the notes in a song (rule 6).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Notation {
    /// `B` is B, `Bb` is B♭.
    #[default]
    English,
    /// `H` is B, `B` is B♭.
    German,
}

impl Notation {
    /// `english` or `german`, in any case.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        if text.eq_ignore_ascii_case("english") {
            Some(Self::English)
        } else if text.eq_ignore_ascii_case("german") {
            Some(Self::German)
        } else {
            None
        }
    }

    /// The vault default of rule 6: `chord_notation` in a `settings.yaml`
    /// (format.md §7), `None` if missing or not `english` or `german`.
    #[must_use]
    pub fn from_settings(settings: &str) -> Option<Self> {
        crate::flat_yaml::value(settings, "chord_notation").and_then(|v| Self::parse(&v))
    }
}

impl Vault {
    /// The vault default of rule 6 in `.notesapp/settings.yaml`; English if
    /// it is not set or the file cannot be read.
    #[must_use]
    pub fn chord_notation(&self) -> Notation {
        self.settings_text().map_or_else(
            |e| {
                eprintln!("settings not readable, English chord names: {e}");
                Notation::default()
            },
            |text| Notation::from_settings(&text).unwrap_or_default(),
        )
    }
}

/// A key: its tonic as a pitch class (0 = C … 11 = B) and whether it is minor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Key {
    /// The tonic, 0 = C to 11 = B.
    pub tonic: u8,
    /// A minor key.
    pub minor: bool,
}

impl Key {
    /// The key a chord or a `{key}` value names: `G`, `Em`, `F#m`, `Bb`; a
    /// root in lower case is minor (`e`). `None` if it is not a chord.
    #[must_use]
    pub fn parse(text: &str, notation: Notation) -> Option<Self> {
        let chord = Chord::parse(text, notation)?;
        Some(Self {
            tonic: chord.root.pc,
            minor: chord.minor(),
        })
    }

    /// This key moved by `semitones`.
    #[must_use]
    pub fn transposed(self, semitones: i32) -> Self {
        Self {
            tonic: shift(self.tonic, semitones),
            minor: self.minor,
        }
    }

    /// Sharps or flats for the notes of this key.
    fn spelling(self) -> Spelling {
        let major = if self.minor {
            (self.tonic + 3) % 12
        } else {
            self.tonic
        };
        KEY_SPELLING[usize::from(major)]
    }
}

/// How the black keys are named.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Spelling {
    /// C major and A minor: the usual names of each.
    Natural,
    Sharps,
    Flats,
}

/// The spelling of each major key by its tonic; a minor key takes that of
/// its relative major. F# rather than G♭, D♭ rather than C#: the fewer
/// accidentals, and sharps where both have six.
const KEY_SPELLING: [Spelling; 12] = {
    use Spelling::{Flats, Natural, Sharps};
    [
        Natural, Flats, Sharps, Flats, Sharps, Flats, Sharps, Sharps, Flats, Sharps, Flats, Sharps,
    ]
};

const SHARP_NAMES: [&str; 12] = [
    "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
];
const FLAT_NAMES: [&str; 12] = [
    "C", "Db", "D", "Eb", "E", "F", "Gb", "G", "Ab", "A", "Bb", "B",
];
const NATURAL_NAMES: [&str; 12] = [
    "C", "C#", "D", "Eb", "E", "F", "F#", "G", "Ab", "A", "Bb", "B",
];

/// The name of pitch class `pc` in `spelling` and `notation`.
fn name(pc: u8, spelling: Spelling, notation: Notation) -> &'static str {
    let pc = usize::from(pc % 12);
    let english = match spelling {
        Spelling::Natural => NATURAL_NAMES[pc],
        Spelling::Sharps => SHARP_NAMES[pc],
        Spelling::Flats => FLAT_NAMES[pc],
    };
    match (notation, english) {
        (Notation::German, "B") => "H",
        (Notation::German, "Bb") => "B",
        _ => english,
    }
}

/// `pc` moved by `semitones`, as a pitch class.
fn shift(pc: u8, semitones: i32) -> u8 {
    // rem_euclid(12) is in 0..12, so it fits.
    u8::try_from((i32::from(pc) + semitones).rem_euclid(12)).unwrap_or(0)
}

/// A note as written: its pitch class and whether its letter is lower case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Note {
    pc: u8,
    lower: bool,
}

/// A chord split into root, quality and bass note (rule 4).
#[derive(Debug, Clone, PartialEq, Eq)]
struct Chord<'a> {
    root: Note,
    quality: &'a str,
    bass: Option<Note>,
}

impl<'a> Chord<'a> {
    fn parse(text: &'a str, notation: Notation) -> Option<Self> {
        let (main, bass) = match text.split_once('/') {
            Some((main, bass)) => (main, Some(bass)),
            None => (text, None),
        };
        let (root, quality) = note(main, notation)?;
        if !quality
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "+-()#♯♭°ø△^'".contains(c))
        {
            return None;
        }
        let bass = match bass {
            Some(bass) => match note(bass, notation)? {
                (note, "") => Some(note),
                _ => return None,
            },
            None => None,
        };
        Some(Self {
            root,
            quality,
            bass,
        })
    }

    /// A minor chord: `m`, `min` or `-` after the root (not `maj`), or a
    /// root in lower case.
    fn minor(&self) -> bool {
        let q = self.quality;
        self.root.lower || q.starts_with('-') || (q.starts_with('m') && !q.starts_with("maj"))
    }

    fn spell(&self, semitones: i32, spelling: Spelling, notation: Notation) -> String {
        let spell_note = |note: Note| {
            let name = name(shift(note.pc, semitones), spelling, notation);
            if note.lower {
                name.to_lowercase()
            } else {
                name.to_owned()
            }
        };
        let mut out = spell_note(self.root);
        out.push_str(self.quality);
        if let Some(bass) = self.bass {
            out.push('/');
            out.push_str(&spell_note(bass));
        }
        out
    }
}

/// The note at the start of `text` and what follows: a letter `A`–`H` in
/// either case and an optional `#`, `b`, `♯` or `♭`.
fn note(text: &str, notation: Notation) -> Option<(Note, &str)> {
    let mut chars = text.chars();
    let letter = chars.next()?;
    let lower = letter.is_ascii_lowercase();
    let natural: u8 = match (letter.to_ascii_uppercase(), notation) {
        ('C', _) => 0,
        ('D', _) => 2,
        ('E', _) => 4,
        ('F', _) => 5,
        ('G', _) => 7,
        ('A', _) => 9,
        ('B', Notation::German) => 10,
        ('B', Notation::English) | ('H', _) => 11,
        _ => return None,
    };
    let rest = chars.as_str();
    let (pc, rest) = match rest.chars().next() {
        Some(c @ ('#' | '♯')) => (natural + 1, &rest[c.len_utf8()..]),
        // German `Bb` is read as B♭ like `B`, as song sheets write it so.
        Some(c @ ('b' | '♭')) if natural == 10 => (natural, &rest[c.len_utf8()..]),
        Some(c @ ('b' | '♭')) => (natural + 11, &rest[c.len_utf8()..]),
        _ => (natural, rest),
    };
    Some((Note { pc: pc % 12, lower }, rest))
}

/// `text`, the inside of a chord bracket, moved by `semitones` and spelled
/// for `key`, the key it is moved to (in C major and A minor without one).
/// `None` if `text` is not a chord or `semitones` is a whole number of
/// octaves: it is shown as written then. Spaces around the chord are kept.
#[must_use]
pub fn transpose(
    text: &str,
    semitones: i32,
    key: Option<Key>,
    notation: Notation,
) -> Option<String> {
    if semitones.rem_euclid(12) == 0 {
        return None;
    }
    let trimmed = text.trim();
    let chord = Chord::parse(trimmed, notation)?;
    let spelling = key.map_or(Spelling::Natural, Key::spelling);
    let start = text.len() - text.trim_start().len();
    let end = start + trimmed.len();
    Some(format!(
        "{}{}{}",
        &text[..start],
        chord.spell(semitones, spelling, notation),
        &text[end..]
    ))
}

/// A `{key}` value moved by `semitones`, spelled in its own key; `None` as
/// for [`transpose`].
#[must_use]
pub fn transpose_key(text: &str, semitones: i32, notation: Notation) -> Option<String> {
    let key = Key::parse(text.trim(), notation)?.transposed(semitones);
    transpose(text, semitones, Some(key), notation)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(text: &str, semitones: i32, key: &str, notation: Notation) -> String {
        let key = Key::parse(key, notation).map(|k| k.transposed(semitones));
        transpose(text, semitones, key, notation).unwrap_or_else(|| text.to_owned())
    }

    #[test]
    fn reads_notes_in_both_namings() {
        let pc = |text, notation| Chord::parse(text, notation).map(|c| c.root.pc);
        assert_eq!(pc("B", Notation::English), Some(11));
        assert_eq!(pc("Bb", Notation::English), Some(10));
        assert_eq!(pc("H", Notation::English), Some(11));
        assert_eq!(pc("B", Notation::German), Some(10));
        assert_eq!(pc("Bb", Notation::German), Some(10));
        assert_eq!(pc("H", Notation::German), Some(11));
        assert_eq!(pc("Hb", Notation::German), Some(10));
        assert_eq!(pc("Cb", Notation::English), Some(11));
        assert_eq!(pc("F♯m", Notation::English), Some(6));
        assert_eq!(pc("N.C.", Notation::English), None);
        assert_eq!(pc("C/Em", Notation::English), None);
    }

    #[test]
    fn minor_keys() {
        let minor = |text| Key::parse(text, Notation::English).unwrap().minor;
        assert!(minor("Em") && minor("e") && minor("Amin") && minor("C-7"));
        assert!(!minor("Cmaj7") && !minor("CM7") && !minor("G"));
    }

    #[test]
    fn whole_octaves_and_non_chords_stay_as_written() {
        assert_eq!(transpose("A#", 12, None, Notation::English), None);
        assert_eq!(transpose("N.C.", 2, None, Notation::English), None);
        assert_eq!(t(" G ", 2, "G", Notation::English), " A ");
        assert_eq!(t("G", -14, "G", Notation::English), "F");
    }

    #[test]
    fn keys_are_spelled_in_their_own_key() {
        let k = |text, n, notation| transpose_key(text, n, notation).unwrap();
        assert_eq!(k("C", 1, Notation::English), "Db");
        assert_eq!(k("E", 2, Notation::English), "F#");
        assert_eq!(k("Am", 1, Notation::English), "Bbm");
        assert_eq!(k("Em", 1, Notation::German), "Fm");
        assert_eq!(k("G", 4, Notation::German), "H");
        assert_eq!(k("e", 7, Notation::German), "h");
    }

    #[test]
    fn settings_give_the_vault_default() {
        assert_eq!(
            Notation::from_settings("chord_notation: German\n"),
            Some(Notation::German)
        );
        assert_eq!(Notation::from_settings("chord_notation: latin\n"), None);
        assert_eq!(Notation::from_settings(""), None);
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/7-settings/settings.yaml");
        let settings = std::fs::read_to_string(fixture).unwrap();
        assert_eq!(Notation::from_settings(&settings), Some(Notation::German));
    }

    /// format.md §5.7 rules 5 and 6: `tests/fixtures/format/5.7-render/transpose.tsv`.
    #[test]
    fn transposition_of_the_fixture() {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/5.7-render/transpose.tsv");
        let table = std::fs::read_to_string(path).unwrap();
        let mut rows = 0;
        for line in table
            .lines()
            .filter(|l| !l.is_empty() && !l.starts_with('#'))
        {
            let cells: Vec<&str> = line.split('\t').collect();
            let [notation, key, semitones, written, expected] = cells[..] else {
                panic!("five columns: {line:?}");
            };
            let notation = Notation::parse(notation).unwrap();
            let semitones: i32 = semitones.parse().unwrap();
            assert_eq!(t(written, semitones, key, notation), expected, "{line:?}");
            rows += 1;
        }
        assert!(rows > 20);
    }
}
