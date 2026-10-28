// SPDX-License-Identifier: Apache-2.0
//! Song sheets in ChordPro (format.md §5.7, ADR-0014).
//!
//! [`Song::parse`] reads the content of a ` ```chordpro ` block into lines:
//! directives (`{title: …}`, short forms mapped to the long ones), ChordPro
//! comments (`# …`), tablature between `start_of_tab` and `end_of_tab`, and
//! lyrics split at chord brackets. The index uses it for rule 8: the lyrics
//! without brackets and the title, subtitle, artist, composer and lyricist
//! go into the full text ([`Song::index_text`]), chord names into their own
//! table ([`Song::chords`]). Rendering and transposition will build on the
//! same lines.

/// A parsed `chordpro` block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Song {
    /// The lines in order.
    pub lines: Vec<Line>,
}

/// One line of a song.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// An empty line.
    Empty,
    /// A ChordPro comment (`# …`, rule 3): kept, never shown.
    Comment(String),
    /// A directive such as `{title: Abendlied}`.
    Directive(Directive),
    /// A line of a tab section, as written.
    Tab(String),
    /// Lyrics, split at the chord brackets.
    Lyrics(Vec<Segment>),
}

/// A directive (rule 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Directive {
    /// The name in lower case, short forms replaced by the long ones
    /// (`t` → `title`); unknown names as written, in lower case.
    pub name: String,
    /// The value after `:` or a space, trimmed; `None` if there is none.
    pub value: Option<String>,
    /// `false` for a directive format version 1 does not know (shown dimmed).
    pub known: bool,
}

/// A piece of a lyrics line: a bracket group and the text up to the next one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    /// What the brackets before the text hold, as written; `None` for the
    /// text before the first bracket.
    pub chord: Option<String>,
    /// The lyrics.
    pub text: String,
}

/// Directives of format version 1 with their short forms (rule 2).
const DIRECTIVES: &[(&str, &[&str])] = &[
    ("title", &["t"]),
    ("subtitle", &["st"]),
    ("artist", &[]),
    ("composer", &[]),
    ("lyricist", &[]),
    ("key", &[]),
    ("capo", &[]),
    ("tempo", &[]),
    ("time", &[]),
    ("comment", &["c"]),
    ("comment_italic", &["ci"]),
    ("start_of_verse", &["sov"]),
    ("end_of_verse", &["eov"]),
    ("start_of_chorus", &["soc"]),
    ("end_of_chorus", &["eoc"]),
    ("chorus", &[]),
    ("start_of_bridge", &["sob"]),
    ("end_of_bridge", &["eob"]),
    ("start_of_tab", &["sot"]),
    ("end_of_tab", &["eot"]),
    ("transpose", &[]),
];

/// Directives whose value goes into the full-text index (rule 8).
const INDEXED_DIRECTIVES: &[&str] = &["title", "subtitle", "artist", "composer", "lyricist"];

impl Song {
    /// Parses the content of a `chordpro` block. Never fails: what is not a
    /// directive or a comment is lyrics, an unclosed `[` is text.
    #[must_use]
    pub fn parse(source: &str) -> Self {
        let mut lines = Vec::new();
        let mut in_tab = false;
        for raw in source.lines() {
            let line = raw.trim();
            let parsed = if let Some(directive) = directive(line) {
                match directive.name.as_str() {
                    "start_of_tab" => in_tab = true,
                    "end_of_tab" => in_tab = false,
                    _ => {}
                }
                Line::Directive(directive)
            } else if in_tab {
                Line::Tab(raw.to_owned())
            } else if line.is_empty() {
                Line::Empty
            } else if let Some(comment) = line.strip_prefix('#') {
                Line::Comment(comment.trim().to_owned())
            } else {
                Line::Lyrics(segments(raw.trim_end()))
            };
            lines.push(parsed);
        }
        Self { lines }
    }

    /// The text for the full-text index (rule 8): the values of the
    /// `title`, `subtitle`, `artist`, `composer` and `lyricist` directives
    /// and the lyrics without chord brackets, one line each.
    #[must_use]
    pub fn index_text(&self) -> String {
        let mut out = String::new();
        for line in &self.lines {
            let text = match line {
                Line::Directive(d) if INDEXED_DIRECTIVES.contains(&d.name.as_str()) => {
                    d.value.clone().unwrap_or_default()
                }
                Line::Lyrics(segments) => segments.iter().map(|s| s.text.as_str()).collect(),
                _ => continue,
            };
            let text = text.trim();
            if !text.is_empty() {
                out.push_str(text);
                out.push('\n');
            }
        }
        out
    }

    /// The chord names in the lyrics (rule 4), each once, in order of first
    /// appearance, normalized by [`normalize_chord`]. Brackets holding
    /// something else are left out.
    #[must_use]
    pub fn chords(&self) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for line in &self.lines {
            let Line::Lyrics(segments) = line else {
                continue;
            };
            for chord in segments.iter().filter_map(|s| s.chord.as_deref()) {
                if let Some(name) = normalize_chord(chord)
                    && !out.contains(&name)
                {
                    out.push(name);
                }
            }
        }
        out
    }
}

/// `{name}`, `{name: value}` or `{name value}`; `None` if `line` is not a
/// directive.
fn directive(line: &str) -> Option<Directive> {
    let inner = line.strip_prefix('{')?.strip_suffix('}')?.trim();
    let split = inner.find(|c: char| c == ':' || c.is_whitespace());
    let (name, value) = match split {
        Some(at) => (&inner[..at], inner[at + 1..].trim()),
        None => (inner, ""),
    };
    if name.is_empty() {
        return None;
    }
    let name = name.to_ascii_lowercase();
    let known = DIRECTIVES
        .iter()
        .find(|(long, short)| *long == name || short.contains(&name.as_str()));
    Some(Directive {
        name: known.map_or(name, |(long, _)| (*long).to_owned()),
        value: (!value.is_empty()).then(|| value.to_owned()),
        known: known.is_some(),
    })
}

/// Splits a lyrics line at its bracket groups.
fn segments(line: &str) -> Vec<Segment> {
    let mut out = Vec::new();
    let mut chord = None;
    let mut rest = line;
    loop {
        let open = rest.find('[');
        let close = open.and_then(|o| rest[o..].find(']').map(|c| o + c));
        let (Some(open), Some(close)) = (open, close) else {
            out.push(Segment {
                chord,
                text: rest.to_owned(),
            });
            break;
        };
        if open > 0 || chord.is_some() {
            out.push(Segment {
                chord,
                text: rest[..open].to_owned(),
            });
        }
        chord = Some(rest[open + 1..close].to_owned());
        rest = &rest[close + 1..];
    }
    out
}

/// The name of a chord as the index stores it, or `None` if `text` is not
/// one (rule 4): root `A`–`H` in upper case, `#` or `b` (`♯` and `♭` are
/// read as these), the quality and extensions as written, an optional `/`
/// bass note. `am` becomes `Am`; `N.C.`, `x` or `*Riff` are not chords.
#[must_use]
pub fn normalize_chord(text: &str) -> Option<String> {
    let text = text.trim().replace('♯', "#").replace('♭', "b");
    let (main, bass) = match text.split_once('/') {
        Some((main, bass)) => (main, Some(bass)),
        None => (text.as_str(), None),
    };
    let (root, quality) = split_root(main)?;
    let quality_ok = quality
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "+-()#°ø△^'".contains(c));
    if !quality_ok {
        return None;
    }
    let mut name = root;
    name.push_str(quality);
    if let Some(bass) = bass {
        let (bass_root, after) = split_root(bass)?;
        if !after.is_empty() {
            return None;
        }
        name.push('/');
        name.push_str(&bass_root);
    }
    Some(name)
}

/// The root at the start of `text` (`A`–`H` in upper case with `#` or `b`)
/// and what follows.
fn split_root(text: &str) -> Option<(String, &str)> {
    let mut chars = text.chars();
    let letter = chars.next()?.to_ascii_uppercase();
    if !('A'..='H').contains(&letter) {
        return None;
    }
    let mut root = letter.to_string();
    let rest = chars.as_str();
    match rest.chars().next() {
        Some(accidental @ ('#' | 'b')) => {
            root.push(accidental);
            Some((root, &rest[1..]))
        }
        _ => Some((root, rest)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(chord: Option<&str>, text: &str) -> Segment {
        Segment {
            chord: chord.map(str::to_owned),
            text: text.to_owned(),
        }
    }

    #[test]
    fn parses_lines() {
        let song = Song::parse(
            "{title: Abendlied}\n{t Kurz}\n{soc: Refrain}\n\
             Der [G]Mond ist aufge[D]gangen\n[Am] [C]\n\n\
             # nur für mich\n{sot}\ne|---0---|\n{eot}\n{x_custom: a}\n[unclosed text",
        );
        let directive = |name: &str, value: Option<&str>, known| {
            Line::Directive(Directive {
                name: name.into(),
                value: value.map(Into::into),
                known,
            })
        };
        assert_eq!(
            song.lines,
            [
                directive("title", Some("Abendlied"), true),
                directive("title", Some("Kurz"), true),
                directive("start_of_chorus", Some("Refrain"), true),
                Line::Lyrics(vec![
                    seg(None, "Der "),
                    seg(Some("G"), "Mond ist aufge"),
                    seg(Some("D"), "gangen"),
                ]),
                Line::Lyrics(vec![seg(Some("Am"), " "), seg(Some("C"), "")]),
                Line::Empty,
                Line::Comment("nur für mich".into()),
                directive("start_of_tab", None, true),
                Line::Tab("e|---0---|".into()),
                directive("end_of_tab", None, true),
                directive("x_custom", Some("a"), false),
                Line::Lyrics(vec![seg(None, "[unclosed text")]),
            ]
        );
    }

    #[test]
    fn chord_names() {
        for (text, name) in [
            ("Am", Some("Am")),
            ("am", Some("Am")),
            ("F#m7", Some("F#m7")),
            ("F♯m7", Some("F#m7")),
            ("Bb", Some("Bb")),
            ("H7", Some("H7")),
            ("C/E", Some("C/E")),
            ("D/f#", Some("D/F#")),
            ("Gsus4", Some("Gsus4")),
            ("Cmaj7(#11)", Some("Cmaj7(#11)")),
            ("Edim°", Some("Edim°")),
            (" G ", Some("G")),
            ("N.C.", None),
            ("x", None),
            ("*Riff", None),
            ("", None),
            ("C/", None),
            ("C/Em", None),
            ("A B", None),
        ] {
            assert_eq!(normalize_chord(text).as_deref(), name, "{text:?}");
        }
    }

    #[test]
    fn chords_once_in_order_without_non_chords() {
        let song = Song::parse("[G]a [D]b [G]c [N.C.]d\n{sot}\n[E]tab\n{eot}\n[Em]e");
        assert_eq!(song.chords(), ["G", "D", "Em"]);
    }

    /// format.md §5.7 rule 8: the golden files in
    /// `tests/fixtures/format/5.7-index/`.
    #[test]
    fn index_text_and_chords_of_the_fixture() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/5.7-index");
        let read = |name| std::fs::read_to_string(fixtures.join(name)).unwrap();
        let song = Song::parse(&read("song.cho"));
        assert_eq!(song.index_text(), read("index-text.txt"));
        assert_eq!(song.chords().join("\n") + "\n", read("chords.txt"));
    }
}
