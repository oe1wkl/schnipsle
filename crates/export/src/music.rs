// SPDX-License-Identifier: Apache-2.0
//! Music in exports: what scores and song sheets become in each format.
//!
//! Scores (ADR-0033) are `abc` blocks engraved by the app's web view.
//! The exporters cannot run abcjs, so [`crate::score_blocks`] lists the
//! blocks of the notes to export, the app engraves them with the editor's
//! code and passes the SVGs back in [`crate::Input::scores`]. Each SVG is
//! cleaned here before it goes into a document: only drawing elements stay,
//! the text is black, and the word joiners of the editor's preparation go.

use std::collections::HashMap;
use std::fmt::Write as _;

use comrak::nodes::NodeValue;
use comrak::{Arena, parse_document};
use quick_xml::escape::{escape, resolve_predefined_entity};
use quick_xml::events::{BytesStart, Event};
use quick_xml::reader::Reader;
use schnipsle_core::tags::parse_options;
use serde::{Deserialize, Serialize};

/// An `abc` block to engrave: its content and `transpose=`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScoreBlock {
    /// The content between the fences, as the note has it.
    pub source: String,
    /// `transpose=` of the info string (format.md §5.6 rule 6); 0 without one.
    pub transpose: i32,
}

/// An engraved block, as the app sends it back.
#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Score {
    /// The block's content, as [`ScoreBlock::source`] gave it.
    pub source: String,
    /// As [`ScoreBlock::transpose`] gave it.
    pub transpose: i32,
    /// One SVG per tune, in order. Empty if the block could not be engraved,
    /// such as when abcjs warned about it.
    pub tunes: Vec<String>,
}

/// `transpose=` of an `abc` info string, or `None` for another block. As in
/// the editor (`ui/src/score.ts`): one to three digits, an unknown option or
/// value ignored.
pub(crate) fn abc_transpose(info: &str) -> Option<i32> {
    let mut words = info.split_whitespace();
    if !words.next()?.eq_ignore_ascii_case("abc") {
        return None;
    }
    let mut transpose = 0;
    for word in words {
        let Some((name, value)) = word.split_once('=') else {
            continue;
        };
        let digits = value.strip_prefix(['+', '-']).unwrap_or(value);
        if name.eq_ignore_ascii_case("transpose")
            && (1..=3).contains(&digits.len())
            && digits.bytes().all(|b| b.is_ascii_digit())
            && let Ok(semitones) = value.parse()
        {
            transpose = semitones;
        }
    }
    Some(transpose)
}

/// The `abc` blocks of a note's text, in order.
pub(crate) fn blocks_of(text: &str) -> Vec<ScoreBlock> {
    let arena = Arena::new();
    let root = parse_document(&arena, text, &parse_options());
    root.descendants()
        .filter_map(|node| match &node.data().value {
            NodeValue::CodeBlock(block) if block.fenced => {
                abc_transpose(&block.info).map(|transpose| ScoreBlock {
                    source: block.literal.clone(),
                    transpose,
                })
            }
            _ => None,
        })
        .collect()
}

/// What music is written as: the format, or the one it is made from.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Output {
    /// HTML, and PDF printed from it: scores inline, song sheets as the editor shows them.
    #[default]
    Html,
    /// ODT: scores as pictures, song sheets as lines of chords over lines of lyrics.
    Odt,
    /// DOCX through Pandoc, as ODT.
    Docx,
    /// EPUB through Pandoc: scores as pictures, song sheets in HTML.
    Epub,
}

/// The music of an export: the engraved scores by block, and the format.
#[derive(Debug, Default)]
pub(crate) struct Music<'i> {
    by_block: HashMap<(&'i str, i32), &'i [String]>,
    pub output: Output,
    /// The ABC source stays under its score ([`crate::Options::abc_source`]).
    pub abc_source: bool,
}

impl<'i> Music<'i> {
    pub(crate) fn of(scores: &'i [Score], output: Output, abc_source: bool) -> Self {
        Self {
            by_block: scores
                .iter()
                .filter(|score| !score.tunes.is_empty())
                .map(|score| {
                    (
                        (score.source.as_str(), score.transpose),
                        score.tunes.as_slice(),
                    )
                })
                .collect(),
            output,
            abc_source,
        }
    }

    /// The cleaned SVGs of a block, or `None` if it has none or one of them
    /// is no SVG.
    pub(crate) fn tunes(&self, source: &str, transpose: i32) -> Option<Vec<String>> {
        let tunes = self.by_block.get(&(source, transpose))?;
        tunes.iter().map(|svg| clean_svg(svg)).collect()
    }
}

/// The first title (`T:`) of a block, to name it in the report.
pub(crate) fn title_of(source: &str) -> String {
    source
        .lines()
        .find_map(|line| line.strip_prefix("T:"))
        .map(str::trim)
        .filter(|title| !title.is_empty())
        .unwrap_or("Score")
        .to_owned()
}

/// Elements of a score; anything else is left out with what is inside it.
const ELEMENTS: &[&str] = &[
    "svg", "g", "path", "text", "tspan", "rect", "circle", "ellipse", "line", "polyline", "polygon",
];

/// Attributes that draw; any other, `style`, links and events among them, is
/// left out.
const ATTRIBUTES: &[&str] = &[
    "xmlns",
    "width",
    "height",
    "viewBox",
    "role",
    "aria-label",
    "d",
    "x",
    "y",
    "x1",
    "y1",
    "x2",
    "y2",
    "dx",
    "dy",
    "cx",
    "cy",
    "r",
    "rx",
    "ry",
    "points",
    "transform",
    "fill",
    "fill-opacity",
    "fill-rule",
    "stroke",
    "stroke-width",
    "stroke-opacity",
    "stroke-linecap",
    "stroke-linejoin",
    "stroke-dasharray",
    "opacity",
    "font-family",
    "font-size",
    "font-style",
    "font-weight",
    "text-anchor",
    "text-decoration",
    "dominant-baseline",
    "letter-spacing",
];

/// Inserted by the editor's preparation to keep offsets (ADR-0032).
const WORD_JOINER: char = '\u{2060}';

/// The SVG of an engraved tune, cleaned (ADR-0033 rule 3), or `None` if it is
/// not one well-formed `svg` element.
pub(crate) fn clean_svg(svg: &str) -> Option<String> {
    let mut reader = Reader::from_str(svg);
    let mut out = String::with_capacity(svg.len());
    // Names of the open elements that are kept; `skipping` counts open ones
    // that are left out.
    let mut open: Vec<String> = Vec::new();
    let mut skipping = 0usize;
    let mut roots = 0;
    loop {
        match reader.read_event().ok()? {
            Event::Start(start) => {
                let name = start.name().as_ref().to_owned();
                if skipping > 0
                    || !ELEMENTS.contains(&name.as_str())
                    || (open.is_empty() && name != "svg")
                {
                    skipping += 1;
                    continue;
                }
                if open.is_empty() {
                    roots += 1;
                }
                write_start(&mut out, &start, &name, open.is_empty())?;
                out.push('>');
                open.push(name);
            }
            Event::Empty(start) => {
                let name = start.name().as_ref().to_owned();
                if skipping == 0 && !open.is_empty() && ELEMENTS.contains(&name.as_str()) {
                    write_start(&mut out, &start, &name, false)?;
                    out.push_str("/>");
                }
            }
            Event::End(_) => {
                if skipping > 0 {
                    skipping -= 1;
                } else {
                    let name = open.pop()?;
                    let _ = write!(out, "</{name}>");
                }
            }
            Event::Text(text) if skipping == 0 && !open.is_empty() => {
                let text: String = text
                    .xml10_content()
                    .chars()
                    .filter(|&c| c != WORD_JOINER)
                    .collect();
                out.push_str(&escape(text.as_str()));
            }
            Event::GeneralRef(reference) if skipping == 0 && !open.is_empty() => {
                let character = match reference.resolve_char_ref().ok()? {
                    Some(c) => c.to_string(),
                    None => resolve_predefined_entity(&reference)?.to_owned(),
                };
                if character != WORD_JOINER.to_string() {
                    out.push_str(&escape(character.as_str()));
                }
            }
            Event::CData(_) => return None,
            Event::Eof => break,
            _ => {}
        }
    }
    (roots == 1 && open.is_empty() && skipping == 0).then_some(out)
}

/// Writes `<name` and the kept attributes of `start`. The root gets black
/// instead of `currentColor`, and a `viewBox` if it has none.
fn write_start(out: &mut String, start: &BytesStart<'_>, name: &str, root: bool) -> Option<()> {
    let _ = write!(out, "<{name}");
    let (mut width, mut height, mut view_box) = (None, None, false);
    for attribute in start.attributes() {
        let attribute = attribute.ok()?;
        let key = attribute.key.as_ref();
        if !ATTRIBUTES.contains(&key) {
            continue;
        }
        let value = attribute
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .ok()?;
        let value = if value == "currentColor" {
            "#000000".into()
        } else {
            value
        };
        match key {
            "width" => width = value.parse::<f64>().ok(),
            "height" => height = value.parse::<f64>().ok(),
            "viewBox" => view_box = true,
            _ => {}
        }
        let _ = write!(out, " {key}=\"{}\"", escape(value.as_ref()));
    }
    if root
        && !view_box
        && let (Some(width), Some(height)) = (width, height)
    {
        let _ = write!(out, " viewBox=\"0 0 {width} {height}\"");
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transpose_is_read_as_the_editor_reads_it() {
        assert_eq!(abc_transpose("abc"), Some(0));
        assert_eq!(abc_transpose("ABC transpose=-3 nosource"), Some(-3));
        assert_eq!(abc_transpose("abc Transpose=+12"), Some(12));
        assert_eq!(abc_transpose("abc transpose=1234"), Some(0));
        assert_eq!(abc_transpose("abc transpose=two"), Some(0));
        assert_eq!(abc_transpose("abcd"), None);
        assert_eq!(abc_transpose("chordpro transpose=2"), None);
        assert_eq!(abc_transpose(""), None);
    }

    #[test]
    fn the_abc_blocks_of_a_note_are_listed_in_order() {
        let text = "# Lied\n\n```abc\nX:1\nK:C\nCDE|\n```\n\n```chordpro\n[C]la\n```\n\n- ```abc transpose=2\n  K:G\n  GAB|\n  ```\n";
        assert_eq!(
            blocks_of(text),
            [
                ScoreBlock {
                    source: "X:1\nK:C\nCDE|\n".to_owned(),
                    transpose: 0
                },
                ScoreBlock {
                    source: "K:G\nGAB|\n".to_owned(),
                    transpose: 2
                },
            ]
        );
    }

    #[test]
    fn a_block_is_named_by_its_first_title() {
        assert_eq!(title_of("X:1\nT: Kanon \nT:zwei\nK:D\n"), "Kanon");
        assert_eq!(title_of("K:D\nfed|\n"), "Score");
    }

    /// As abcjs 6.7 writes it, shortened.
    const ENGRAVED: &str = concat!(
        r#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" role="img" fill="currentColor" stroke="currentColor" aria-label="Sheet Music for &quot;K&amp;L&quot;" width="600" height="135.5">"#,
        r#"<style>.abcjs-dragging-in-progress text { user-select: none; }</style>"#,
        r#"<title xmlns="http://www.w3.org/1999/xhtml">Sheet Music for "K&amp;L"</title>"#,
        r#"<g><text stroke="none" font-size="27" font-family="Times New Roman" class="" x="300" y="49.5" data-name="title"><tspan x="300">Ü&#x2060;&#8288;"#,
        "\u{2060}",
        r#"ber &amp; mehr</tspan></text></g>"#,
        r##"<g fill="#000000" stroke="none" data-name="clef"><path d="M 0 77 L 336 77 z" onclick="alert(1)"/></g>"##,
        r#"<a href="https://example.com"><text>weg</text></a><script>alert(2)</script>"#,
        "</svg>"
    );

    #[test]
    fn an_engraved_tune_keeps_only_what_draws_in_black() {
        let clean = clean_svg(ENGRAVED).unwrap();
        assert_eq!(
            clean,
            concat!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" role="img" fill="#000000" stroke="#000000" aria-label="Sheet Music for &quot;K&amp;L&quot;" width="600" height="135.5" viewBox="0 0 600 135.5">"##,
                r#"<g><text stroke="none" font-size="27" font-family="Times New Roman" x="300" y="49.5"><tspan x="300">Über &amp; mehr</tspan></text></g>"#,
                r##"<g fill="#000000" stroke="none"><path d="M 0 77 L 336 77 z"/></g>"##,
                "</svg>"
            )
        );
    }

    #[test]
    fn anything_but_one_svg_is_refused() {
        assert_eq!(clean_svg("<p>no</p>"), None);
        assert_eq!(clean_svg("<svg><g></svg>"), None);
        assert_eq!(clean_svg("<svg></svg><svg></svg>"), None);
        assert_eq!(clean_svg("<svg"), None);
        assert!(clean_svg("<svg width=\"1\" height=\"2\"/>").is_none());
        assert_eq!(
            clean_svg("<svg viewBox=\"0 0 1 2\"></svg>").as_deref(),
            Some("<svg viewBox=\"0 0 1 2\"></svg>")
        );
    }

    #[test]
    fn scores_are_found_by_source_and_transposition() {
        let scores = [
            Score {
                source: "K:C\nC|\n".to_owned(),
                transpose: 2,
                tunes: vec!["<svg></svg>".to_owned()],
            },
            Score {
                source: "K:D\nD|\n".to_owned(),
                transpose: 0,
                tunes: vec![],
            },
        ];
        let scores = Music::of(&scores, Output::Html, false);
        assert_eq!(
            scores.tunes("K:C\nC|\n", 2),
            Some(vec!["<svg></svg>".to_owned()])
        );
        assert_eq!(scores.tunes("K:C\nC|\n", 0), None);
        assert_eq!(scores.tunes("K:D\nD|\n", 0), None);
    }
}
