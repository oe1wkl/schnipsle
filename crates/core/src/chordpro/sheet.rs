// SPDX-License-Identifier: Apache-2.0
//! Song sheets rendered from a `chordpro` block (format.md §5.7, ADR-0014,
//! ADR-0030), for the editor's preview and the exports alike.
//!
//! [`render`] turns the block's [`Song`] into a [`Sheet`]: the song's
//! details, then sections of lines with each chord over the text it belongs
//! to (rule 1), moved by the transposition of the info string, the viewer
//! and `{transpose}` lines (rule 5) and spelled for the key they are moved
//! to ([`chord`](super::chord)). ChordPro comments are left out (rule 3),
//! unknown directives are shown as written (rule 2). [`apply_transposition`]
//! is "Apply transposition": it rewrites the chords in the file.

use serde::Serialize;

use super::chord::{Key, Notation, respell, respell_key, transpose, transpose_key};
use super::{Directive, Line, Segment, Song};

/// The options of a block's info string (rule 7), such as
/// `chordpro transpose=2 notation=german nochords`. Unknown options and
/// invalid values are ignored.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct BlockOptions {
    /// `transpose=<semitones>`.
    pub transpose: i32,
    /// `notation=english|german`.
    pub notation: Option<Notation>,
    /// `nochords`: lyrics only.
    pub nochords: bool,
    /// `nosource`: no source in read-only views.
    pub nosource: bool,
}

impl BlockOptions {
    /// Reads the options after the first word of `info` (the language).
    #[must_use]
    pub fn parse(info: &str) -> Self {
        let mut options = Self::default();
        for option in info.split_whitespace().skip(1) {
            match option.split_once('=') {
                Some(("transpose", n)) => {
                    if let Ok(n) = n.parse() {
                        options.transpose = n;
                    }
                }
                Some(("notation", name)) => {
                    if let Some(notation) = Notation::parse(name) {
                        options.notation = Some(notation);
                    }
                }
                None if option == "nochords" => options.nochords = true,
                None if option == "nosource" => options.nosource = true,
                _ => {}
            }
        }
        options
    }
}

/// How the viewer shows a block, besides what the file says.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct View {
    /// The viewer's transpose control, added to the block's.
    pub transpose: i32,
    /// Lyrics only, as with `nochords`.
    pub lyrics_only: bool,
    /// The vault default of rule 6, for a block without `notation=`.
    pub notation: Notation,
    /// The names to show the chords in, if not those they are written in
    /// (ADR-0031).
    pub shown: Option<Notation>,
}

/// A rendered song sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sheet {
    /// Title, subtitle, artist, composer, lyricist, key (moved like the
    /// chords), capo, tempo and time, in the order written.
    pub meta: Vec<Meta>,
    /// The song.
    pub sections: Vec<Section>,
    /// The names the chords are written in: the block's or the vault's.
    pub notation: Notation,
    /// The names they are shown in.
    pub shown: Notation,
    /// Semitones of the info string and the viewer, before any `{transpose}`.
    pub transpose: i32,
    /// Chords, chord lines and tablature left out.
    pub lyrics_only: bool,
    /// The info string asks to hide the source.
    pub nosource: bool,
}

/// A detail of the song, such as its title.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Meta {
    /// The directive's long name: `title`, `subtitle`, `artist`, `composer`,
    /// `lyricist`, `key`, `capo`, `tempo` or `time`.
    pub name: String,
    /// Its value, the key as shown.
    pub value: String,
}

/// A verse, chorus, bridge or tab, or lines outside any of them.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Section {
    /// What it is.
    pub kind: SectionKind,
    /// The label of `{start_of_verse: Strophe 2}` or `{chorus: Refrain}`.
    pub label: Option<String>,
    /// A chorus sung again by `{chorus}`.
    pub repeat: bool,
    /// Its lines, without empty lines at either end or two in a row.
    pub lines: Vec<SheetLine>,
}

/// The kind of a [`Section`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SectionKind {
    /// Lines outside a section.
    Plain,
    /// `{start_of_verse}` to `{end_of_verse}`.
    Verse,
    /// `{start_of_chorus}` to `{end_of_chorus}`, or `{chorus}`.
    Chorus,
    /// `{start_of_bridge}` to `{end_of_bridge}`.
    Bridge,
    /// `{start_of_tab}` to `{end_of_tab}`.
    Tab,
}

/// A line of a song sheet.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum SheetLine {
    /// Text with chords over it: each segment's chord, as shown, goes over
    /// the start of its text. With lyrics only, one segment without a chord.
    Lyrics {
        /// The pieces of the line.
        segments: Vec<Segment>,
    },
    /// A line of chords only (rule 1).
    Chords {
        /// The chords, as shown.
        chords: Vec<String>,
    },
    /// `{comment}` or, italic, `{comment_italic}`.
    Comment {
        /// The comment.
        text: String,
        /// Shown in italics.
        italic: bool,
    },
    /// A line of tablature, as written.
    Tab {
        /// The line.
        text: String,
    },
    /// A directive format version 1 does not know, shown dimmed (rule 2).
    Unknown {
        /// The directive, such as `{x_lights: dim}`.
        text: String,
    },
    /// Space between paragraphs.
    Empty,
}

/// Directives shown among the song's details.
const META: &[&str] = &[
    "title", "subtitle", "artist", "composer", "lyricist", "key", "capo", "tempo", "time",
];

/// Renders the content of a `chordpro` block whose info string is `info`.
#[must_use]
pub fn render(source: &str, info: &str, view: View) -> Sheet {
    let options = BlockOptions::parse(info);
    let notation = options.notation.unwrap_or(view.notation);
    let song = Song::parse(source);
    let shown = view.shown.unwrap_or(notation);
    let mut renderer = Renderer {
        notation,
        shown,
        lyrics_only: options.nochords || view.lyrics_only,
        base: options.transpose.saturating_add(view.transpose),
        shift: 0,
        key: first_key(&song, notation),
        meta: Vec::new(),
        sections: Vec::new(),
        current: None,
        chorus: None,
    };
    for line in &song.lines {
        renderer.line(line);
    }
    renderer.close();
    Sheet {
        meta: renderer.meta,
        sections: renderer.sections,
        notation,
        shown,
        transpose: renderer.base,
        lyrics_only: renderer.lyrics_only,
        nosource: options.nosource,
    }
}

/// The key the chords are spelled for until a `{key}` line: the first
/// `{key}` of the song, else its first chord.
fn first_key(song: &Song, notation: Notation) -> Option<Key> {
    let directive = song.lines.iter().find_map(|line| match line {
        Line::Directive(d) if d.name == "key" => d.value.as_deref(),
        _ => None,
    });
    if let Some(key) = directive.and_then(|v| Key::parse(v.trim(), notation)) {
        return Some(key);
    }
    song.lines.iter().find_map(|line| match line {
        Line::Lyrics(segments) => segments
            .iter()
            .filter_map(|s| s.chord.as_deref())
            .find_map(|c| Key::parse(c.trim(), notation)),
        _ => None,
    })
}

/// The last chorus, for `{chorus}`.
struct Chorus<'a> {
    label: Option<String>,
    lines: Vec<&'a Line>,
}

struct Renderer<'a> {
    /// The names chords are written in.
    notation: Notation,
    /// The names they are shown in.
    shown: Notation,
    lyrics_only: bool,
    /// Semitones of the info string and the viewer.
    base: i32,
    /// Semitones of the last `{transpose}`.
    shift: i32,
    /// The key as written.
    key: Option<Key>,
    meta: Vec<Meta>,
    sections: Vec<Section>,
    current: Option<Section>,
    /// The last chorus; while one is open, the chorus being written.
    chorus: Option<Chorus<'a>>,
}

impl<'a> Renderer<'a> {
    fn semitones(&self) -> i32 {
        self.base.saturating_add(self.shift)
    }

    fn line(&mut self, line: &'a Line) {
        let in_chorus = self
            .current
            .as_ref()
            .is_some_and(|s| s.kind == SectionKind::Chorus && !s.repeat);
        if in_chorus
            && !matches!(line, Line::Directive(d) if d.name.starts_with("end_of_"))
            && let Some(chorus) = &mut self.chorus
        {
            chorus.lines.push(line);
        }
        match line {
            Line::Empty => {
                if self.current.is_some() {
                    self.push(SheetLine::Empty);
                }
            }
            Line::Comment(_) => {}
            Line::Tab(text) => self.push(SheetLine::Tab { text: text.clone() }),
            Line::Lyrics(segments) => {
                if let Some(line) = self.lyrics(segments) {
                    self.push(line);
                }
            }
            Line::Directive(directive) => self.directive(directive),
        }
    }

    fn directive(&mut self, d: &Directive) {
        let value = d.value.clone();
        match d.name.as_str() {
            "key" => {
                if let Some(v) = value {
                    if let Some(key) = Key::parse(v.trim(), self.notation) {
                        self.key = Some(key);
                    }
                    let shown =
                        respell_key(&v, self.semitones(), self.notation, self.shown).unwrap_or(v);
                    self.meta.push(Meta {
                        name: d.name.clone(),
                        value: shown,
                    });
                }
            }
            name if META.contains(&name) => {
                if let Some(value) = value {
                    self.meta.push(Meta {
                        name: d.name.clone(),
                        value,
                    });
                }
            }
            "comment" | "comment_italic" => {
                if let Some(text) = value {
                    let italic = d.name == "comment_italic";
                    self.push(SheetLine::Comment { text, italic });
                }
            }
            "start_of_verse" => self.open(SectionKind::Verse, value),
            "start_of_chorus" => {
                self.open(SectionKind::Chorus, value.clone());
                self.chorus = Some(Chorus {
                    label: value,
                    lines: Vec::new(),
                });
            }
            "start_of_bridge" => self.open(SectionKind::Bridge, value),
            "start_of_tab" => self.open(SectionKind::Tab, value),
            name if name.starts_with("end_of_") => self.close(),
            "chorus" => self.recall(value),
            "transpose" => {
                self.shift = value.and_then(|v| v.trim().parse().ok()).unwrap_or(0);
            }
            _ if !d.known => {
                let text = match &d.value {
                    Some(v) => format!("{{{}: {v}}}", d.name),
                    None => format!("{{{}}}", d.name),
                };
                self.push(SheetLine::Unknown { text });
            }
            _ => {}
        }
    }

    /// A lyrics line with its chords moved, or a chord line; `None` for a
    /// chord line with lyrics only.
    fn lyrics(&self, segments: &[Segment]) -> Option<SheetLine> {
        let chords_only = segments.iter().all(|s| s.text.trim().is_empty());
        if self.lyrics_only {
            if chords_only {
                return None;
            }
            let text: String = segments.iter().map(|s| s.text.as_str()).collect();
            return Some(SheetLine::Lyrics {
                segments: vec![Segment {
                    chord: None,
                    text: text.trim_end().to_owned(),
                }],
            });
        }
        let semitones = self.semitones();
        let key = self.key.map(|k| k.transposed(semitones));
        let shown = |chord: &str| {
            respell(chord, semitones, key, self.notation, self.shown)
                .unwrap_or_else(|| chord.to_owned())
        };
        if chords_only {
            let chords = segments
                .iter()
                .filter_map(|s| s.chord.as_deref())
                .map(|c| shown(c).trim().to_owned())
                .collect();
            return Some(SheetLine::Chords { chords });
        }
        Some(SheetLine::Lyrics {
            segments: segments
                .iter()
                .map(|s| Segment {
                    chord: s.chord.as_deref().map(shown),
                    text: s.text.clone(),
                })
                .collect(),
        })
    }

    /// `{chorus}`: the last chorus again, moved as the lines here are.
    fn recall(&mut self, label: Option<String>) {
        self.close();
        let (lines, written_label) = match self.chorus.take() {
            Some(chorus) => {
                let lines = chorus.lines.clone();
                let written = chorus.label.clone();
                self.chorus = Some(chorus);
                (lines, written)
            }
            None => (Vec::new(), None),
        };
        self.current = Some(Section {
            kind: SectionKind::Chorus,
            label: label.or(written_label),
            repeat: true,
            lines: Vec::new(),
        });
        for line in lines {
            self.line(line);
        }
        self.close();
    }

    fn open(&mut self, kind: SectionKind, label: Option<String>) {
        self.close();
        self.current = Some(Section {
            kind,
            label,
            repeat: false,
            lines: Vec::new(),
        });
    }

    fn push(&mut self, line: SheetLine) {
        let section = self.current.get_or_insert_with(|| Section {
            kind: SectionKind::Plain,
            label: None,
            repeat: false,
            lines: Vec::new(),
        });
        if line == SheetLine::Empty && section.lines.last().is_none_or(|l| *l == SheetLine::Empty) {
            return;
        }
        section.lines.push(line);
    }

    fn close(&mut self) {
        let Some(mut section) = self.current.take() else {
            return;
        };
        while section.lines.last() == Some(&SheetLine::Empty) {
            section.lines.pop();
        }
        let dropped = (section.kind == SectionKind::Tab && self.lyrics_only)
            || (section.kind == SectionKind::Plain && section.lines.is_empty());
        if !dropped {
            self.sections.push(section);
        }
    }
}

/// "Apply transposition" (rule 5): `source` with every chord moved by
/// `semitones` and spelled for its new key, and `{key}` values moved too.
/// Everything else stays exactly as written, `{transpose}` lines, tablature
/// and what is not a chord included.
#[must_use]
pub fn apply_transposition(source: &str, semitones: i32, notation: Notation) -> String {
    if semitones.rem_euclid(12) == 0 {
        return source.to_owned();
    }
    let song = Song::parse(source);
    let mut key = first_key(&song, notation);
    let mut out = String::with_capacity(source.len());
    for (raw, line) in source.split_inclusive('\n').zip(&song.lines) {
        match line {
            Line::Directive(d) if d.name == "key" => {
                let value = d.value.as_deref().unwrap_or_default();
                if let Some(k) = Key::parse(value.trim(), notation) {
                    key = Some(k);
                }
                match (transpose_key(value, semitones, notation), raw.rfind(value)) {
                    (Some(moved), Some(at)) if !value.is_empty() => {
                        out.push_str(&raw[..at]);
                        out.push_str(&moved);
                        out.push_str(&raw[at + value.len()..]);
                    }
                    _ => out.push_str(raw),
                }
            }
            Line::Lyrics(_) => {
                let key = key.map(|k| k.transposed(semitones));
                let mut rest = raw;
                while let Some(open) = rest.find('[') {
                    let Some(close) = rest[open..].find(']').map(|c| open + c) else {
                        break;
                    };
                    let inner = &rest[open + 1..close];
                    out.push_str(&rest[..=open]);
                    out.push_str(
                        &transpose(inner, semitones, key, notation)
                            .unwrap_or_else(|| inner.to_owned()),
                    );
                    out.push(']');
                    rest = &rest[close + 1..];
                }
                out.push_str(rest);
            }
            _ => out.push_str(raw),
        }
    }
    out
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
    fn info_string_options() {
        assert_eq!(
            BlockOptions::parse("chordpro transpose=-3 notation=German nochords nosource x=1 y"),
            BlockOptions {
                transpose: -3,
                notation: Some(Notation::German),
                nochords: true,
                nosource: true,
            }
        );
        assert_eq!(
            BlockOptions::parse("chordpro transpose=two notation=latin"),
            BlockOptions::default()
        );
        assert_eq!(BlockOptions::parse("chordpro transpose=+2").transpose, 2);
    }

    #[test]
    fn sections_lines_and_empty_lines() {
        let sheet = render(
            "\n{title: Lied}\n# geheim\nvorher\n\n\n{sov: 1}\n\n[G]a [D]b\n[G] [C]\n\n{eov}\n\nnachher\n{unknown}\n",
            "chordpro",
            View::default(),
        );
        assert_eq!(
            sheet.meta,
            [Meta {
                name: "title".into(),
                value: "Lied".into()
            }]
        );
        let kinds: Vec<_> = sheet.sections.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            [SectionKind::Plain, SectionKind::Verse, SectionKind::Plain]
        );
        assert_eq!(
            sheet.sections[0].lines,
            [SheetLine::Lyrics {
                segments: vec![seg(None, "vorher")]
            }]
        );
        assert_eq!(sheet.sections[1].label.as_deref(), Some("1"));
        assert_eq!(
            sheet.sections[1].lines,
            [
                SheetLine::Lyrics {
                    segments: vec![seg(Some("G"), "a "), seg(Some("D"), "b")]
                },
                SheetLine::Chords {
                    chords: vec!["G".into(), "C".into()]
                },
            ]
        );
        assert_eq!(
            sheet.sections[2].lines[1],
            SheetLine::Unknown {
                text: "{unknown}".into()
            }
        );
    }

    #[test]
    fn transposition_adds_up_and_follows_the_key() {
        let source = "{key: G}\n[G]a [D/F#]b\n{transpose: 2}\n[G]c\n{transpose}\n[G]d\n";
        let view = View {
            transpose: -1,
            ..View::default()
        };
        let sheet = render(source, "chordpro transpose=2", view);
        let chords: Vec<String> = sheet
            .sections
            .iter()
            .flat_map(|s| &s.lines)
            .flat_map(|l| match l {
                SheetLine::Lyrics { segments } => {
                    segments.iter().filter_map(|s| s.chord.clone()).collect()
                }
                _ => Vec::new(),
            })
            .collect();
        // G + 1 is A♭ major, with flats; + 3 is B♭ major.
        assert_eq!(chords, ["Ab", "Eb/G", "Bb", "Ab"]);
        assert_eq!(sheet.meta[0].value, "Ab");
        assert_eq!(sheet.transpose, 1);
    }

    #[test]
    fn a_chorus_is_sung_again_in_the_key_of_its_place() {
        let source =
            "{soc: Refrain}\n[C]la\n{eoc}\n{transpose: 2}\n{chorus}\n{chorus: Noch einmal}\n";
        let sheet = render(source, "chordpro", View::default());
        let chorus = &sheet.sections[1];
        assert!(chorus.repeat && chorus.kind == SectionKind::Chorus);
        assert_eq!(chorus.label.as_deref(), Some("Refrain"));
        assert_eq!(
            chorus.lines,
            [SheetLine::Lyrics {
                segments: vec![seg(Some("D"), "la")]
            }]
        );
        assert_eq!(sheet.sections[2].label.as_deref(), Some("Noch einmal"));
        let alone = render("{chorus}\n", "chordpro", View::default());
        assert!(alone.sections[0].repeat && alone.sections[0].lines.is_empty());
    }

    #[test]
    fn lyrics_only() {
        let sheet = render(
            "[G]Der [D]Mond \n[G] [D]\n{sot}\ne|--|\n{eot}\n",
            "chordpro nochords",
            View::default(),
        );
        assert!(sheet.lyrics_only);
        assert_eq!(sheet.sections.len(), 1, "no tab");
        assert_eq!(
            sheet.sections[0].lines,
            [SheetLine::Lyrics {
                segments: vec![seg(None, "Der Mond")]
            }]
        );
    }

    #[test]
    fn notation_from_the_block_or_the_vault() {
        let view = View {
            notation: Notation::German,
            ..View::default()
        };
        assert_eq!(render("", "chordpro", view).notation, Notation::German);
        assert_eq!(
            render("", "chordpro notation=english", view).notation,
            Notation::English
        );
        let sheet = render("{key: F}\n[B]a\n", "chordpro transpose=2", view);
        assert_eq!(sheet.meta[0].value, "G");
        assert_eq!(
            sheet.sections[0].lines[0],
            SheetLine::Lyrics {
                segments: vec![seg(Some("C"), "a")]
            }
        );
    }

    #[test]
    fn applying_keeps_everything_but_the_chords() {
        let source =
            "{key: D}\r\n{transpose: 1}\r\nDer [D]Mond [N.C.] [unclosed\n{sot}\n[D]\n{eot}\n[A7]";
        assert_eq!(
            apply_transposition(source, 2, Notation::English),
            "{key: E}\r\n{transpose: 1}\r\nDer [E]Mond [N.C.] [unclosed\n{sot}\n[D]\n{eot}\n[B7]"
        );
        assert_eq!(apply_transposition(source, -12, Notation::English), source);
    }

    /// Golden files in `tests/fixtures/format/5.7-render/`; `SCHNIPSLE_BLESS=1`
    /// writes them anew.
    #[test]
    fn the_fixture_song() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/5.7-render");
        let song = std::fs::read_to_string(fixtures.join("song.cho")).unwrap();
        let transposed = View {
            transpose: 2,
            ..View::default()
        };
        let lyrics_only = View {
            lyrics_only: true,
            ..View::default()
        };
        let in_german = View {
            shown: Some(Notation::German),
            ..View::default()
        };
        let cases = [
            ("render.json", render(&song, "chordpro", View::default())),
            (
                "render-transposed.json",
                render(&song, "chordpro transpose=1", transposed),
            ),
            (
                "render-lyrics-only.json",
                render(&song, "chordpro", lyrics_only),
            ),
            ("render-german.json", render(&song, "chordpro", in_german)),
        ];
        let mut outputs: Vec<(&str, String)> = cases
            .iter()
            .map(|(name, sheet)| (*name, serde_json::to_string_pretty(sheet).unwrap() + "\n"))
            .collect();
        outputs.push((
            "song+3.cho",
            apply_transposition(&song, 3, Notation::English),
        ));
        for (name, output) in outputs {
            let path = fixtures.join(name);
            if std::env::var_os("SCHNIPSLE_BLESS").is_some() {
                std::fs::write(&path, &output).unwrap();
            }
            assert_eq!(output, std::fs::read_to_string(&path).unwrap(), "{name}");
        }
    }
}
