// SPDX-License-Identifier: Apache-2.0
//! Song sheets in exports (ADR-0030, ADR-0034): a `chordpro` block rendered by
//! the core, as the file says it (its `transpose=`, `notation=` and `nochords`,
//! else the vault's naming), and written for each format.
//!
//! HTML, PDF and EPUB show it as the editor does, each chord over the start of
//! its text ([`html`], styled by `song.css`). ODT and DOCX get a line of chords
//! over each line of lyrics, both in a monospace font, so the chords stay over
//! their syllables in every word processor ([`odt`], [`docx`]).

use std::fmt::Write as _;

use schnipsle_core::chordpro::Segment;
use schnipsle_core::chordpro::chord::Notation;
use schnipsle_core::chordpro::sheet::{self, Meta, Section, SectionKind, Sheet, SheetLine, View};

use crate::odt_content::{escape, write_source};

/// How [`html`] looks, for the HTML page and the EPUB.
pub(crate) const CSS: &str = include_str!("song.css");

/// The sheet of a block with info string `info`, in the vault's `notation`
/// where the block names none.
pub(crate) fn render(source: &str, info: &str, notation: Notation) -> Sheet {
    let view = View {
        notation,
        ..View::default()
    };
    sheet::render(source, info, view)
}

/// Whether `info` is that of a `chordpro` block.
pub(crate) fn is_song(info: &str) -> bool {
    info.split_whitespace()
        .next()
        .is_some_and(|lang| lang.eq_ignore_ascii_case("chordpro"))
}

/// The song's details as the editor shows them: title, subtitle, and lines of
/// the others.
struct Details {
    title: Option<String>,
    subtitle: Option<String>,
    lines: Vec<String>,
}

fn details(meta: &[Meta]) -> Details {
    let value = |name: &str| {
        meta.iter()
            .find(|m| m.name == name)
            .map(|m| m.value.clone())
    };
    let labelled = |label: &str, name: &str| value(name).map(|v| format!("{label} {v}"));
    let mut lines = Vec::new();
    for parts in [
        [
            value("artist"),
            labelled("Music:", "composer"),
            labelled("Words:", "lyricist"),
        ]
        .as_slice(),
        &[
            labelled("Key", "key"),
            labelled("Capo", "capo"),
            labelled("Tempo", "tempo"),
            labelled("Time", "time"),
        ],
    ] {
        let shown: Vec<&str> = parts
            .iter()
            .flatten()
            .map(String::as_str)
            .filter(|p| !p.trim().is_empty())
            .collect();
        if !shown.is_empty() {
            lines.push(shown.join(" · "));
        }
    }
    Details {
        title: value("title"),
        subtitle: value("subtitle"),
        lines,
    }
}

/// The label over a section: its own, or the name of a chorus or bridge.
fn label(section: &Section) -> Option<&str> {
    section.label.as_deref().or(match section.kind {
        SectionKind::Chorus => Some("Chorus"),
        SectionKind::Bridge => Some("Bridge"),
        SectionKind::Plain | SectionKind::Verse | SectionKind::Tab => None,
    })
}

/// Whether a line of lyrics has chords; one without is a plain line.
fn has_chords(segments: &[Segment]) -> bool {
    segments.iter().any(|s| s.chord.is_some())
}

/// A line of lyrics as a line of chords over a line of lyrics, for a monospace
/// font: each chord starts over the start of its text, and the lyrics get
/// spaces where a chord is wider than the text under it.
fn two_lines(segments: &[Segment]) -> (String, String) {
    let (mut chords, mut lyrics) = (String::new(), String::new());
    let (mut chords_width, mut lyrics_width) = (0, 0);
    for segment in segments {
        if let Some(chord) = &segment.chord {
            if chords_width > lyrics_width {
                lyrics.extend(std::iter::repeat_n(' ', chords_width - lyrics_width));
                lyrics_width = chords_width;
            }
            chords.extend(std::iter::repeat_n(' ', lyrics_width - chords_width));
            chords.push_str(chord);
            chords.push(' ');
            chords_width = lyrics_width + chord.chars().count() + 1;
        }
        lyrics.push_str(&segment.text);
        lyrics_width += segment.text.chars().count();
    }
    (chords.trim_end().to_owned(), lyrics)
}

/// `content` as a raw block of `format` in the Markdown Pandoc reads
/// (`raw_attribute`), with blank lines around it.
pub(crate) fn raw_block(format: &str, content: &str) -> String {
    let longest = content.split(|c| c != '`').map(str::len).max().unwrap_or(0);
    let fence = "`".repeat(longest.max(2) + 1);
    format!("\n{fence}{{={format}}}\n{content}{fence}\n\n")
}

// --- HTML, for HTML, PDF and EPUB ---

/// The sheet as HTML, styled by `song.css`.
pub(crate) fn html(sheet: &Sheet) -> String {
    let mut out = String::from("<div class=\"song\">\n");
    let details = details(&sheet.meta);
    let div = |out: &mut String, class: &str, text: &str| {
        let _ = writeln!(out, "<div class=\"{class}\">{}</div>", escape(text));
    };
    if let Some(title) = &details.title {
        div(&mut out, "song-title", title);
    }
    if let Some(subtitle) = &details.subtitle {
        div(&mut out, "song-subtitle", subtitle);
    }
    for line in &details.lines {
        div(&mut out, "song-details", line);
    }
    for section in &sheet.sections {
        let kind = match section.kind {
            SectionKind::Plain => "plain",
            SectionKind::Verse => "verse",
            SectionKind::Chorus => "chorus",
            SectionKind::Bridge => "bridge",
            SectionKind::Tab => "tab",
        };
        let _ = writeln!(out, "<div class=\"song-section song-{kind}\">");
        if let Some(label) = label(section) {
            div(&mut out, "song-label", label);
        }
        for line in &section.lines {
            html_line(&mut out, line);
        }
        out.push_str("</div>\n");
    }
    out.push_str("</div>\n");
    out
}

fn html_line(out: &mut String, line: &SheetLine) {
    match line {
        SheetLine::Lyrics { segments } if has_chords(segments) => {
            out.push_str("<div class=\"song-line song-chorded\">");
            for segment in segments {
                // A chord over nothing still needs a line of lyrics under it.
                let text = if segment.text.is_empty() {
                    "&#160;".to_owned()
                } else {
                    escape(&segment.text)
                };
                let _ = write!(
                    out,
                    "<span class=\"song-seg\"><span class=\"song-chord\">{}</span><span class=\"song-text\">{text}</span></span>",
                    escape(segment.chord.as_deref().unwrap_or_default())
                );
            }
            out.push_str("</div>\n");
        }
        SheetLine::Lyrics { segments } => {
            let text: String = segments.iter().map(|s| s.text.as_str()).collect();
            let _ = writeln!(out, "<div class=\"song-line\">{}</div>", escape(&text));
        }
        SheetLine::Chords { chords } => {
            out.push_str("<div class=\"song-line song-chords\">");
            for chord in chords {
                let _ = write!(out, "<span class=\"song-chord\">{}</span>", escape(chord));
            }
            out.push_str("</div>\n");
        }
        SheetLine::Comment { text, italic } => {
            let class = if *italic {
                "song-comment song-italic"
            } else {
                "song-comment"
            };
            let _ = writeln!(out, "<div class=\"{class}\">{}</div>", escape(text));
        }
        SheetLine::Tab { text } => {
            let _ = writeln!(out, "<div class=\"song-tab\">{}</div>", escape(text));
        }
        SheetLine::Unknown { text } => {
            let _ = writeln!(out, "<div class=\"song-unknown\">{}</div>", escape(text));
        }
        SheetLine::Empty => out.push_str("<div class=\"song-gap\"></div>\n"),
    }
}

// --- ODT ---

/// A paragraph style of `odt_styles.xml`, in a chorus or not.
fn odt_style(name: &str, chorus: bool) -> String {
    if chorus {
        format!("Song_20_Chorus_20_{name}")
    } else {
        format!("Song_20_{name}")
    }
}

/// The sheet as paragraphs of an ODT, in the styles `Song …` of `odt_styles.xml`.
pub(crate) fn odt(sheet: &Sheet) -> String {
    let mut out = String::new();
    let paragraph = |out: &mut String, style: &str, text: &str| {
        let _ = write!(out, "<text:p text:style-name=\"{style}\">");
        write_source(out, text);
        out.push_str("</text:p>\n");
    };
    let details = details(&sheet.meta);
    if let Some(title) = &details.title {
        paragraph(&mut out, "Song_20_Title", title);
    }
    if let Some(subtitle) = &details.subtitle {
        paragraph(&mut out, "Song_20_Subtitle", subtitle);
    }
    for line in &details.lines {
        paragraph(&mut out, "Song_20_Details", line);
    }
    for section in &sheet.sections {
        let in_chorus = section.kind == SectionKind::Chorus;
        let style = |name: &str| odt_style(name, in_chorus);
        if let Some(label) = label(section) {
            paragraph(&mut out, &style("Label"), label);
        }
        for line in &section.lines {
            match line {
                SheetLine::Lyrics { segments } => {
                    let (chords, lyrics) = two_lines(segments);
                    if !chords.is_empty() {
                        paragraph(&mut out, &style("Chords"), &chords);
                    }
                    paragraph(&mut out, &style("Lyrics"), &lyrics);
                }
                SheetLine::Chords { chords } => {
                    paragraph(&mut out, &style("Chords"), &chords.join("  "));
                }
                SheetLine::Comment { text, italic } => {
                    let name = if *italic {
                        "Comment_20_Italic"
                    } else {
                        "Comment"
                    };
                    paragraph(&mut out, &style(name), text);
                }
                SheetLine::Tab { text } => paragraph(&mut out, &style("Tab"), text),
                SheetLine::Unknown { text } => paragraph(&mut out, &style("Unknown"), text),
                SheetLine::Empty => paragraph(&mut out, &style("Lyrics"), ""),
            }
        }
    }
    out
}

// --- DOCX ---

/// How a paragraph of a sheet looks in Word.
#[derive(Clone, Copy)]
enum Look {
    Title,
    Subtitle,
    Details,
    Label,
    Chords,
    Lyrics,
    Comment,
    CommentItalic,
    Tab,
    Unknown,
}

/// Grey of the details, labels and comments, as in the HTML theme.
const GREY: &str = "6E6E73";
/// Blue of the chords, as in the HTML theme.
const BLUE: &str = "0A66D8";
/// A monospace font every word processor has or replaces by one of the same widths.
const MONOSPACE: &str = "Courier New";

/// The sheet as paragraphs of Word XML, formatted directly so that they need
/// no styles of the document.
pub(crate) fn docx(sheet: &Sheet) -> String {
    let mut out = String::new();
    let details = details(&sheet.meta);
    if let Some(title) = &details.title {
        docx_paragraph(&mut out, Look::Title, false, title);
    }
    if let Some(subtitle) = &details.subtitle {
        docx_paragraph(&mut out, Look::Subtitle, false, subtitle);
    }
    for line in &details.lines {
        docx_paragraph(&mut out, Look::Details, false, line);
    }
    for section in &sheet.sections {
        let in_chorus = section.kind == SectionKind::Chorus;
        if let Some(label) = label(section) {
            docx_paragraph(&mut out, Look::Label, in_chorus, label);
        }
        for line in &section.lines {
            match line {
                SheetLine::Lyrics { segments } => {
                    let (chords, lyrics) = two_lines(segments);
                    if !chords.is_empty() {
                        docx_paragraph(&mut out, Look::Chords, in_chorus, &chords);
                    }
                    docx_paragraph(&mut out, Look::Lyrics, in_chorus, &lyrics);
                }
                SheetLine::Chords { chords } => {
                    docx_paragraph(&mut out, Look::Chords, in_chorus, &chords.join("  "));
                }
                SheetLine::Comment { text, italic } => {
                    let look = if *italic {
                        Look::CommentItalic
                    } else {
                        Look::Comment
                    };
                    docx_paragraph(&mut out, look, in_chorus, text);
                }
                SheetLine::Tab { text } => docx_paragraph(&mut out, Look::Tab, in_chorus, text),
                SheetLine::Unknown { text } => {
                    docx_paragraph(&mut out, Look::Unknown, in_chorus, text);
                }
                SheetLine::Empty => docx_paragraph(&mut out, Look::Lyrics, in_chorus, ""),
            }
        }
    }
    out
}

fn docx_paragraph(out: &mut String, look: Look, chorus: bool, text: &str) {
    // Sizes in half points.
    let (size, bold, italic, colour, monospace) = match look {
        Look::Title => (28, true, false, None, false),
        Look::Subtitle => (22, false, true, None, false),
        Look::Details | Look::Comment => (18, false, false, Some(GREY), false),
        Look::CommentItalic => (18, false, true, Some(GREY), false),
        Look::Label => (18, true, false, Some(GREY), false),
        Look::Chords => (20, true, false, Some(BLUE), true),
        Look::Lyrics => (20, false, false, None, true),
        Look::Tab => (18, false, false, None, true),
        Look::Unknown => (18, false, false, Some(GREY), true),
    };
    let (before, after) = match look {
        Look::Title | Look::Label => (120, 0),
        Look::Comment | Look::CommentItalic => (60, 60),
        _ => (0, 0),
    };
    // In the order Word's schema asks for: keepNext, pBdr, spacing, ind.
    out.push_str("<w:p><w:pPr>");
    // A line of chords stays on the page of its lyrics.
    if matches!(look, Look::Chords | Look::Label | Look::Title) {
        out.push_str("<w:keepNext/>");
    }
    if chorus {
        // A bar as the editor draws it.
        out.push_str(
            "<w:pBdr><w:left w:val=\"single\" w:sz=\"18\" w:space=\"6\" w:color=\"D2D2D7\"/></w:pBdr>",
        );
    }
    let _ = write!(
        out,
        "<w:spacing w:before=\"{before}\" w:after=\"{after}\"/>"
    );
    if chorus {
        // 0.5 cm.
        out.push_str("<w:ind w:left=\"284\"/>");
    }
    out.push_str("</w:pPr>");
    if !text.is_empty() {
        out.push_str("<w:r><w:rPr>");
        if monospace {
            let _ = write!(
                out,
                "<w:rFonts w:ascii=\"{MONOSPACE}\" w:hAnsi=\"{MONOSPACE}\" w:cs=\"{MONOSPACE}\"/>"
            );
        }
        if bold {
            out.push_str("<w:b/>");
        }
        if italic {
            out.push_str("<w:i/>");
        }
        if let Some(colour) = colour {
            let _ = write!(out, "<w:color w:val=\"{colour}\"/>");
        }
        let _ = write!(
            out,
            "<w:sz w:val=\"{size}\"/></w:rPr><w:t xml:space=\"preserve\">{}</w:t></w:r>",
            escape(text)
        );
    }
    out.push_str("</w:p>\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn segments(line: &[(Option<&str>, &str)]) -> Vec<Segment> {
        line.iter()
            .map(|(chord, text)| Segment {
                chord: chord.map(str::to_owned),
                text: (*text).to_owned(),
            })
            .collect()
    }

    #[test]
    fn each_chord_starts_over_its_text() {
        let line = segments(&[
            (None, "Der "),
            (Some("G"), "Mond ist aufge"),
            (Some("D"), "gangen,"),
        ]);
        assert_eq!(
            two_lines(&line),
            (
                "    G             D".to_owned(),
                "Der Mond ist aufgegangen,".to_owned()
            )
        );
    }

    #[test]
    fn a_wide_chord_moves_the_lyrics_after_it() {
        let line = segments(&[
            (Some("Am7"), ""),
            (Some("Cmaj7(#11)"), "a"),
            (Some("D"), "bc "),
            (Some("G/H"), "d"),
        ]);
        assert_eq!(
            two_lines(&line),
            (
                "Am7 Cmaj7(#11) D  G/H".to_owned(),
                "    a          bc d".to_owned()
            )
        );
    }

    #[test]
    fn chords_are_counted_in_characters_not_bytes() {
        let line = segments(&[(Some("F♯m7"), "Ü"), (Some("H"), "ber")]);
        assert_eq!(
            two_lines(&line),
            ("F♯m7 H".to_owned(), "Ü    ber".to_owned())
        );
    }

    const SONG: &str = "{title: Abendlied}\n{artist: Volksweise}\n{key: G}\n\n{start_of_verse}\nDer [G]Mond & die <Sterne>\n{end_of_verse}\n{start_of_chorus}\n[C]La [D]la\n{comment: leise}\n{end_of_chorus}\n";

    #[test]
    fn html_shows_chords_over_their_text() {
        let sheet = render(SONG, "chordpro transpose=2", Notation::English);
        let html = html(&sheet);
        assert!(
            html.contains("<div class=\"song-title\">Abendlied</div>"),
            "{html}"
        );
        assert!(
            html.contains("<div class=\"song-details\">Volksweise</div>"),
            "{html}"
        );
        assert!(
            html.contains("<div class=\"song-details\">Key A</div>"),
            "{html}"
        );
        assert!(
            html.contains("<span class=\"song-seg\"><span class=\"song-chord\">A</span><span class=\"song-text\">Mond &amp; die &lt;Sterne&gt;</span></span>"),
            "{html}"
        );
        assert!(
            html.contains(
                "<div class=\"song-section song-chorus\">\n<div class=\"song-label\">Chorus</div>"
            ),
            "{html}"
        );
        assert!(
            html.contains("<div class=\"song-comment\">leise</div>"),
            "{html}"
        );
    }

    #[test]
    fn odt_and_docx_put_a_line_of_chords_over_the_lyrics() {
        let sheet = render(SONG, "chordpro notation=german", Notation::English);
        let odt = odt(&sheet);
        assert!(odt.contains("<text:p text:style-name=\"Song_20_Chords\"><text:s text:c=\"4\"/>G</text:p>\n<text:p text:style-name=\"Song_20_Lyrics\">Der Mond &amp; die &lt;Sterne&gt;</text:p>"), "{odt}");
        assert!(
            odt.contains("<text:p text:style-name=\"Song_20_Chorus_20_Label\">Chorus</text:p>"),
            "{odt}"
        );
        assert!(
            odt.contains(
                "<text:p text:style-name=\"Song_20_Chorus_20_Chords\">C<text:s/>  D</text:p>"
            ) || odt.contains("Song_20_Chorus_20_Chords\">C"),
            "{odt}"
        );
        let docx = docx(&sheet);
        assert!(
            docx.contains("<w:t xml:space=\"preserve\">    G</w:t>"),
            "{docx}"
        );
        assert!(docx.contains("<w:ind w:left=\"284\"/>"), "{docx}");
    }

    #[test]
    fn without_chords_only_the_lyrics_are_written() {
        let sheet = render(SONG, "chordpro nochords", Notation::English);
        assert!(!html(&sheet).contains("song-chord"));
        assert!(!odt(&sheet).contains("Chords"));
        assert!(!docx(&sheet).contains(BLUE));
    }

    #[test]
    fn only_chordpro_blocks_are_songs() {
        assert!(is_song("chordpro"));
        assert!(is_song("ChordPro nosource"));
        assert!(!is_song("abc"));
        assert!(!is_song(""));
    }
}
