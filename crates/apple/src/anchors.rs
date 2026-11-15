// SPDX-License-Identifier: Apache-2.0
//! Where texts are on the pages of a PDF, for links that printing lost
//! (ADR-0037). The print path of iOS keeps no links; the exporter lists the
//! text of each link and of each target in page order, and [`find_anchors`]
//! finds them with `PDFKit`, one after the other.
//!
//! White space is ignored in comparing, as the page breaks lines where the HTML
//! has a space, and within long words. A text is looked for from where the
//! previous link ended, so the same text further on goes to its own place; the
//! words before it in its block, when given, tell a short text such as a
//! footnote number from the same characters elsewhere.

use std::io;
use std::ops::Range;

use objc2::AnyThread as _;
use objc2::rc::Retained;
use objc2_foundation::{NSData, NSRange};
use objc2_pdf_kit::{PDFDocument, PDFPage};

/// A text to find.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AnchorText<'a> {
    /// Words just before the text, which may be empty.
    pub before: &'a str,
    /// The text.
    pub text: &'a str,
    /// Whether the next text is looked for after this one; if not, from where
    /// this one was looked for.
    pub moves_on: bool,
}

/// One line of a text found on a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FoundLine {
    /// The page, counted from 0.
    pub page: usize,
    /// Left, bottom, right and top, in points from the bottom left of the page.
    pub rect: [f64; 4],
}

/// Where each of `anchors` is on the pages of `pdf`, in their order: the
/// lines of its text, or `None` if it was not found. Any thread.
///
/// # Errors
/// If `PDFKit` cannot read `pdf`.
pub fn find_anchors(
    pdf: &[u8],
    anchors: &[AnchorText<'_>],
) -> io::Result<Vec<Option<Vec<FoundLine>>>> {
    let data = NSData::with_bytes(pdf);
    // SAFETY: valid data; a document that cannot be read is `None`.
    let document = unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }
        .ok_or_else(|| io::Error::other("PDFKit cannot read the PDF"))?;
    // SAFETY: a plain getter on a valid document.
    let count = unsafe { document.pageCount() };
    let pages: Vec<Retained<PDFPage>> = (0..count)
        // SAFETY: an index below the count.
        .map(|index| unsafe { document.pageAtIndex(index) })
        .collect::<Option<_>>()
        .ok_or_else(|| io::Error::other("PDFKit cannot read a page of the PDF"))?;
    let texts: Vec<String> = pages
        .iter()
        // SAFETY: a plain getter on a valid page.
        .map(|page| unsafe { page.string() }.map_or_else(String::new, |s| s.to_string()))
        .collect();
    Ok(locate(&texts, anchors)
        .into_iter()
        .map(|spans| spans.and_then(|spans| lines(&pages, &spans)))
        .collect())
}

/// The lines of the text at `spans`, if it has any.
fn lines(pages: &[Retained<PDFPage>], spans: &[Span]) -> Option<Vec<FoundLine>> {
    let mut found = Vec::new();
    for (index, range) in spans {
        let page = &pages[*index];
        // SAFETY: a range within the page's text, which `locate` took from it.
        let Some(selection) =
            (unsafe { page.selectionForRange(NSRange::new(range.start, range.len())) })
        else {
            continue;
        };
        // SAFETY: a plain getter on a valid selection.
        let by_line = unsafe { selection.selectionsByLine() };
        for line in &by_line {
            // SAFETY: the page the selection was made on.
            let bounds = unsafe { line.boundsForPage(page) };
            if bounds.size.width > 0.0 && bounds.size.height > 0.0 {
                found.push(FoundLine {
                    page: *index,
                    rect: [
                        bounds.origin.x,
                        bounds.origin.y,
                        bounds.origin.x + bounds.size.width,
                        bounds.origin.y + bounds.size.height,
                    ],
                });
            }
        }
    }
    (!found.is_empty()).then_some(found)
}

/// Part of a text on one page: the page, and a range of its text in UTF-16
/// code units, as `PDFKit` counts them.
type Span = (usize, Range<usize>);

/// The characters of all pages that are compared, and where each one is: its
/// page and its range in that page's text, in UTF-16 code units as `PDFKit`
/// counts them.
#[derive(Default)]
struct Text {
    chars: Vec<char>,
    at: Vec<Span>,
}

/// The characters `c` is compared as: none for white space and invisible
/// marks, the letters of a ligature.
fn compared(c: char, mut add: impl FnMut(char)) {
    match c {
        // Variation selectors (an emoji or its text form), soft hyphen, zero
        // widths, word joiner, byte order mark.
        '\u{fe0e}' | '\u{fe0f}' | '\u{ad}' | '\u{200b}'..='\u{200d}' | '\u{2060}' | '\u{feff}' => {}
        'ﬀ' => "ff".chars().for_each(add),
        'ﬁ' => "fi".chars().for_each(add),
        'ﬂ' => "fl".chars().for_each(add),
        'ﬃ' => "ffi".chars().for_each(add),
        'ﬄ' => "ffl".chars().for_each(add),
        'ﬅ' | 'ﬆ' => "st".chars().for_each(add),
        c if c.is_whitespace() => {}
        c => add(c),
    }
}

fn text_of(pages: &[String]) -> Text {
    let mut text = Text::default();
    for (page, string) in pages.iter().enumerate() {
        let mut at = 0;
        for c in string.chars() {
            let end = at + c.len_utf16();
            compared(c, |c| {
                text.chars.push(c);
                text.at.push((page, at..end));
            });
            at = end;
        }
    }
    text
}

fn chars_of(text: &str) -> Vec<char> {
    let mut chars = Vec::with_capacity(text.len());
    for c in text.chars() {
        compared(c, |c| chars.push(c));
    }
    chars
}

/// The first place of `needle` in `hay` at or after `from`.
fn search(hay: &[char], needle: &[char], from: usize) -> Option<usize> {
    if needle.is_empty() || from >= hay.len() {
        return None;
    }
    hay[from..]
        .windows(needle.len())
        .position(|window| window == needle)
        .map(|at| from + at)
}

/// Where each text is: the pages and ranges of its characters.
fn locate(pages: &[String], anchors: &[AnchorText<'_>]) -> Vec<Option<Vec<Span>>> {
    let text = text_of(pages);
    let mut cursor: usize = 0;
    anchors
        .iter()
        .map(|anchor| {
            let wanted = chars_of(anchor.text);
            if wanted.is_empty() {
                return None;
            }
            let before = chars_of(anchor.before);
            let with_context = [before.as_slice(), wanted.as_slice()].concat();
            // The words before may end before where the search starts: they
            // can hold the text of the previous link.
            let mut from = cursor.saturating_sub(before.len());
            let mut start = None;
            if !before.is_empty() {
                while let Some(at) = search(&text.chars, &with_context, from) {
                    if at + before.len() >= cursor {
                        start = Some(at + before.len());
                        break;
                    }
                    from = at + 1;
                }
            }
            let start = start.or_else(|| search(&text.chars, &wanted, cursor))?;
            let end = start + wanted.len();
            if anchor.moves_on {
                cursor = end;
            }
            Some(spans(&text, start..end))
        })
        .collect()
}

/// The characters `range` of `text`, as a range of each page's text.
fn spans(text: &Text, range: Range<usize>) -> Vec<Span> {
    let mut spans: Vec<Span> = Vec::new();
    for (page, at) in &text.at[range] {
        match spans.last_mut() {
            Some((last, span)) if last == page => span.end = at.end,
            _ => spans.push((*page, at.clone())),
        }
    }
    spans
}

#[cfg(test)]
mod tests {
    use super::*;

    fn link<'a>(before: &'a str, text: &'a str) -> AnchorText<'a> {
        AnchorText {
            before,
            text,
            moves_on: true,
        }
    }

    fn target(text: &str) -> AnchorText<'_> {
        AnchorText {
            before: "",
            text,
            moves_on: false,
        }
    }

    fn pages(texts: &[&str]) -> Vec<String> {
        texts.iter().map(|t| (*t).to_owned()).collect()
    }

    /// The text found for each anchor, page by page.
    fn found(pages: &[String], anchors: &[AnchorText<'_>]) -> Vec<Option<String>> {
        locate(pages, anchors)
            .into_iter()
            .map(|spans| {
                spans.map(|spans| {
                    spans
                        .iter()
                        .map(|(page, range)| {
                            let units: Vec<u16> = pages[*page].encode_utf16().collect();
                            format!("{page}:{}", String::from_utf16_lossy(&units[range.clone()]))
                        })
                        .collect::<Vec<_>>()
                        .join(" | ")
                })
            })
            .collect()
    }

    #[test]
    fn white_space_is_ignored_and_the_search_moves_on() {
        let pages = pages(&[
            "Back to the\nstart. Back to the start.",
            "Back to the start",
        ]);
        let anchors = [
            link("", "Back to the start"),
            link("", "Back to the start"),
            link("", "Back to the start"),
        ];
        assert_eq!(
            found(&pages, &anchors),
            [
                Some("0:Back to the\nstart".to_owned()),
                Some("0:Back to the start".to_owned()),
                Some("1:Back to the start".to_owned()),
            ]
        );
    }

    #[test]
    fn the_words_before_find_a_short_text_in_its_own_place() {
        let pages = pages(&["In 2021 the tuner1 was built."]);
        let anchors = [link("the tuner", "1")];
        let spans = locate(&pages, &anchors);
        assert_eq!(spans, [Some(vec![(0, 17..18)])]);
    }

    #[test]
    fn the_words_before_may_hold_the_previous_link() {
        let pages = pages(&["See the club page and mail us."]);
        let anchors = [
            link("See", "the club page"),
            link("the club page and", "mail"),
        ];
        assert_eq!(
            found(&pages, &anchors),
            [
                Some("0:the club page".to_owned()),
                Some("0:mail".to_owned())
            ]
        );
    }

    #[test]
    fn without_its_words_before_a_text_is_still_found() {
        let pages = pages(&["Changed words here link"]);
        let anchors = [link("other words", "link")];
        assert_eq!(found(&pages, &anchors), [Some("0:link".to_owned())]);
    }

    #[test]
    fn a_target_does_not_move_the_search_on() {
        let pages = pages(&["Contents: Welcome", "Welcome to it. Welcome"]);
        let anchors = [
            link("", "Welcome"),
            target("Welcome to"),
            link("", "Welcome"),
        ];
        assert_eq!(
            found(&pages, &anchors),
            [
                Some("0:Welcome".to_owned()),
                Some("1:Welcome to".to_owned()),
                Some("1:Welcome".to_owned()),
            ]
        );
    }

    #[test]
    fn a_text_over_a_page_break_has_a_span_on_each_page() {
        let pages = pages(&["… a long link", "text goes on"]);
        assert_eq!(
            found(&pages, &[link("", "long link text")]),
            [Some("0:long link | 1:text".to_owned())]
        );
    }

    #[test]
    fn ligatures_emoji_forms_and_utf16_are_compared_as_text() {
        let pages = pages(&["😀 The ﬁnal note ↩\u{fe0f}"]);
        let anchors = [link("", "final note"), link("", "↩\u{fe0e}")];
        let spans = locate(&pages, &anchors);
        // The emoji counts two UTF-16 units, as in PDFKit.
        assert_eq!(spans[0], Some(vec![(0, 7..16)]));
        assert_eq!(spans[1], Some(vec![(0, 17..18)]));
    }

    #[test]
    fn what_is_not_there_is_none_and_the_search_stays() {
        let pages = pages(&["one two"]);
        let anchors = [link("", "three"), link("", ""), link("", "one")];
        assert_eq!(
            found(&pages, &anchors),
            [None, None, Some("0:one".to_owned())]
        );
    }
}
