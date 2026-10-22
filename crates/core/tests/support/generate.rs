// SPDX-License-Identifier: Apache-2.0
//! Generates realistic test notes for performance tests (N-03).
//!
//! Deterministic for a given seed, so every run measures the same notes:
//! German and English words (with umlauts) plus invented words for a long
//! tail, headings, paragraphs, tasks, image links, 0–4 tags (some
//! hierarchical) and front matter with a valid `id`, spread over subfolders.

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

/// Common words; the generator also invents words from syllables.
const WORDS: &[&str] = &[
    "antenne",
    "kabel",
    "lötzinn",
    "gehäuse",
    "spule",
    "dipol",
    "frequenz",
    "messung",
    "bericht",
    "termin",
    "einkauf",
    "milch",
    "brot",
    "äpfel",
    "kirche",
    "chor",
    "probe",
    "dienstag",
    "montag",
    "garten",
    "baum",
    "blume",
    "regen",
    "sonne",
    "wetter",
    "reise",
    "zug",
    "bahnhof",
    "hotel",
    "rechnung",
    "steuer",
    "versicherung",
    "arzt",
    "apotheke",
    "buch",
    "notiz",
    "idee",
    "projekt",
    "plan",
    "skizze",
    "foto",
    "bild",
    "lied",
    "noten",
    "akkord",
    "gitarre",
    "klavier",
    "über",
    "für",
    "und",
    "oder",
    "mit",
    "ohne",
    "nach",
    "vor",
    "heute",
    "morgen",
    "gestern",
    "schnell",
    "langsam",
    "größe",
    "höhe",
    "länge",
    "breite",
    "radio",
    "amateur",
    "funk",
    "morse",
    "taste",
    "station",
    "verbindung",
    "the",
    "and",
    "with",
    "tuner",
    "power",
    "supply",
    "battery",
    "test",
    "meeting",
    "notes",
    "draft",
    "review",
    "summary",
    "question",
    "answer",
    "kitchen",
    "recipe",
    "flour",
    "sugar",
    "butter",
    "oven",
    "minutes",
    "weekend",
    "holiday",
    "train",
    "ticket",
];

const SYLLABLES: &[&str] = &[
    "ka", "lo", "mi", "ne", "ru", "ta", "fe", "gi", "bo", "sa", "ver", "an", "ein", "aus", "zu",
    "ber", "ten", "ung", "lich", "keit", "heit", "schaft", "or", "el", "ür", "äh", "ön",
];

const TAGS: &[&str] = &[
    "radio",
    "radio/qrp",
    "radio/cw",
    "projekte",
    "projekte/tuner",
    "projekte/antenne",
    "projekte/garten",
    "chor",
    "musik",
    "musik/noten",
    "einkauf",
    "rezepte",
    "reise",
    "arbeit",
    "arbeit/meeting",
    "familie",
    "gesundheit",
    "finanzen",
    "ideen",
    "lesen",
    "todo",
    "wichtig",
    "archiv",
    "foto",
    "garten",
    "auto",
    "haus",
    "haus/reparatur",
    "technik",
    "computer",
];

const FOLDERS: &[&str] = &[
    "",
    "",
    "",
    "Projekte",
    "Archiv",
    "Musik",
    "Reisen/2026",
    "Arbeit",
];

/// A small, fast, deterministic random generator (splitmix64).
pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        Self(seed)
    }

    pub fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    /// A number in `0..n`.
    pub fn below(&mut self, n: usize) -> usize {
        usize::try_from(self.next() % (n as u64)).unwrap_or(0)
    }

    fn pick<'a>(&mut self, items: &[&'a str]) -> &'a str {
        items[self.below(items.len())]
    }
}

fn word(rng: &mut Rng) -> String {
    if rng.below(10) < 7 {
        rng.pick(WORDS).to_owned()
    } else {
        (0..2 + rng.below(2)).map(|_| rng.pick(SYLLABLES)).collect()
    }
}

/// The text of note number `n`.
pub fn note(rng: &mut Rng, n: usize) -> String {
    let id = schnipsle_core::stamp::ulid(1_700_000_000_000 + n as u64, {
        let mut random = [0u8; 10];
        for b in &mut random {
            *b = u8::try_from(rng.below(256)).unwrap_or(0);
        }
        random
    });
    let mut text = format!(
        "---\nid: {id}\ncreated: 2026-01-01T00:00:00Z\nmodified: 2026-01-01T00:00:00Z\n---\n# {} {} {n}\n\n",
        word(rng),
        word(rng)
    );
    let words = 50 + rng.below(350);
    let mut line = 0;
    for i in 0..words {
        text.push_str(&word(rng));
        line += 1;
        if line > 8 + rng.below(10) {
            text.push_str(if rng.below(4) == 0 { ".\n\n" } else { ".\n" });
            line = 0;
        } else {
            text.push(' ');
        }
        if i % 97 == 50 && rng.below(3) == 0 {
            let _ = write!(text, "\n- [ ] {} {}\n", word(rng), word(rng));
        }
        if i % 151 == 70 && rng.below(4) == 0 {
            let _ = write!(text, "\n![{}](../attachments/ab/{n:x}.png)\n", word(rng));
        }
    }
    let tags = if rng.below(5) == 0 {
        0
    } else {
        1 + rng.below(4)
    };
    text.push('\n');
    for _ in 0..tags {
        let _ = write!(text, " #{}", rng.pick(TAGS));
    }
    text.push('\n');
    text
}

/// Writes `count` notes below `notes_dir`.
///
/// # Errors
/// I/O errors.
pub fn generate(notes_dir: &Path, count: usize, seed: u64) -> io::Result<()> {
    let mut rng = Rng::new(seed);
    for n in 0..count {
        let folder = notes_dir.join(rng.pick(FOLDERS));
        fs::create_dir_all(&folder)?;
        fs::write(folder.join(format!("note-{n:05}.md")), note(&mut rng, n))?;
    }
    Ok(())
}
