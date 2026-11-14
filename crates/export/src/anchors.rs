// SPDX-License-Identifier: Apache-2.0
//! The links and link targets of a page to print, with the text the page shows
//! for each (ADR-0037). A printer whose PDFs lose their links, as on iOS, looks
//! for these texts on the printed pages, and [`crate::pdf_links`] puts the links
//! back where they are. The list is taken from the tree comrak writes, in the
//! order it writes it, so nothing is parsed back from the HTML.

use comrak::nodes::{AstNode, NodeValue};

/// The words of context before a link, and of the text of a target.
const WORDS: usize = 4;

/// A text on a page to print, in the order of the page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Anchor {
    /// Up to four words before a link in its block, so that a short text such
    /// as a footnote number is found in its own place; may be empty.
    pub before: String,
    /// The text the page shows: a link's text, or the first words of the block
    /// a target starts.
    pub text: String,
    /// What it is.
    pub kind: AnchorKind,
}

/// What an [`Anchor`] is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnchorKind {
    /// A link.
    Link {
        /// Where it goes: a web address, or `#` and the id of a target.
        href: String,
        /// The link's own id, as a footnote reference is a target too.
        id: Option<String>,
    },
    /// The start of an element that links go to.
    Target {
        /// Its id.
        id: String,
    },
}

impl Anchor {
    /// Whether the search for the next anchor starts after this one: after a
    /// link, not after a target, whose text is often the next link's too.
    #[must_use]
    pub fn moves_on(&self) -> bool {
        matches!(self.kind, AnchorKind::Link { .. })
    }
}

/// Where the text of an [`Anchor`] was found: a rectangle per line.
#[derive(Debug, Clone, PartialEq)]
pub struct Place {
    /// In the order of the text; never empty.
    pub lines: Vec<Line>,
}

/// One line of an anchor's text on a page.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Line {
    /// The page, counted from 0.
    pub page: usize,
    /// Left, bottom, right and top, in points from the bottom left of the page.
    pub rect: [f64; 4],
}

/// The entry of a table of contents for the note at `id`.
pub(crate) fn contents_entry(title: &str, href: &str) -> Anchor {
    Anchor {
        before: String::new(),
        text: words(title, usize::MAX),
        kind: AnchorKind::Link {
            href: format!("#{href}"),
            id: None,
        },
    }
}

/// The anchors of a `<section>` with `id` and the HTML of `root`.
pub(crate) fn of_section<'a>(root: &'a AstNode<'a>, id: &str) -> Vec<Anchor> {
    let mut walk = Walk::default();
    if let Some(text) = root
        .children()
        .map(|block| first_words(block))
        .find(|text| !text.is_empty())
    {
        walk.target(id, text);
    }
    walk.walk(root);
    walk.anchors
}

#[derive(Default)]
struct Walk {
    anchors: Vec<Anchor>,
    /// The text of the block so far.
    block: String,
}

impl Walk {
    fn target(&mut self, id: &str, text: String) {
        self.anchors.push(Anchor {
            before: String::new(),
            text,
            kind: AnchorKind::Target { id: id.to_owned() },
        });
    }

    fn link(&mut self, href: String, id: Option<String>, text: &str) {
        let text = words(text, usize::MAX);
        if text.is_empty() {
            // A picture alone: nothing to find.
            return;
        }
        self.anchors.push(Anchor {
            before: last_words(&self.block),
            text: text.clone(),
            kind: AnchorKind::Link { href, id },
        });
        self.block.push(' ');
        self.block.push_str(&text);
    }

    fn walk<'a>(&mut self, node: &'a AstNode<'a>) {
        match &node.data().value {
            NodeValue::Paragraph | NodeValue::Heading(_) | NodeValue::TableCell => {
                self.block.clear();
                self.children(node);
            }
            NodeValue::Link(link) => {
                let href = link.url.clone();
                self.link(href, None, &shown_text(node));
            }
            NodeValue::FootnoteReference(reference) => {
                let mut id = format!("fnref-{}", reference.name);
                if reference.ref_num > 1 {
                    id = format!("{id}-{}", reference.ref_num);
                }
                let href = format!("#fn-{}", reference.name);
                self.link(href, Some(id), &reference.ix.to_string());
            }
            NodeValue::FootnoteDefinition(definition) => {
                let (name, references) = (definition.name.clone(), definition.total_references);
                if let Some(text) = node
                    .children()
                    .map(|block| first_words(block))
                    .find(|text| !text.is_empty())
                {
                    self.target(&format!("fn-{name}"), text);
                }
                self.children(node);
                // comrak ends the footnote with a link back to each reference.
                for number in 1..=references {
                    let (suffix, text) = if number > 1 {
                        (format!("-{number}"), format!("↩{number}"))
                    } else {
                        (String::new(), "↩".to_owned())
                    };
                    self.link(format!("#fnref-{name}{suffix}"), None, &text);
                }
            }
            NodeValue::Text(text) => self.block.push_str(text),
            NodeValue::Code(code) => self.block.push_str(&code.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => self.block.push(' '),
            NodeValue::Image(_) => {}
            _ => self.children(node),
        }
    }

    fn children<'a>(&mut self, node: &'a AstNode<'a>) {
        for child in node.children() {
            self.walk(child);
        }
    }
}

/// The text of `node` as the page shows it: footnote numbers included, the
/// description of a picture not.
fn shown_text<'a>(node: &'a AstNode<'a>) -> String {
    fn add<'a>(node: &'a AstNode<'a>, out: &mut String) {
        match &node.data().value {
            NodeValue::Text(t) => out.push_str(t),
            NodeValue::Code(c) => out.push_str(&c.literal),
            NodeValue::FootnoteReference(r) => out.push_str(&r.ix.to_string()),
            NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
            NodeValue::Image(_) => {}
            _ => node.children().for_each(|child| add(child, out)),
        }
    }
    let mut out = String::new();
    add(node, &mut out);
    out
}

/// The first words of a block with text, for a target.
fn first_words<'a>(block: &'a AstNode<'a>) -> String {
    let mut node = block;
    // A list or a quote: its first block.
    while let Some(first) = node.first_child()
        && matches!(
            node.data().value,
            NodeValue::List(_)
                | NodeValue::Item(_)
                | NodeValue::TaskItem(_)
                | NodeValue::BlockQuote
                | NodeValue::Table(_)
                | NodeValue::TableRow(_)
        )
    {
        node = first;
    }
    words(&shown_text(node), WORDS)
}

/// Up to `count` words of `text`, one space apart.
fn words(text: &str, count: usize) -> String {
    text.split_whitespace()
        .take(count)
        .collect::<Vec<_>>()
        .join(" ")
}

/// The last few words of `text`.
fn last_words(text: &str) -> String {
    let words: Vec<&str> = text.split_whitespace().collect();
    words[words.len().saturating_sub(WORDS)..].join(" ")
}

#[cfg(test)]
mod tests {
    use comrak::{Arena, parse_document};
    use schnipsle_core::tags::parse_options;

    use super::*;

    fn anchors(markdown: &str) -> Vec<(String, String, String)> {
        let arena = Arena::new();
        let root = parse_document(&arena, markdown, &parse_options());
        of_section(root, "note-x")
            .into_iter()
            .map(|a| {
                let what = match a.kind {
                    AnchorKind::Link { href, id: None } => format!("link {href}"),
                    AnchorKind::Link { href, id: Some(id) } => format!("link {href} id {id}"),
                    AnchorKind::Target { id } => format!("target {id}"),
                };
                (what, a.before, a.text)
            })
            .collect()
    }

    fn row(what: &str, before: &str, text: &str) -> (String, String, String) {
        (what.to_owned(), before.to_owned(), text.to_owned())
    }

    #[test]
    fn links_in_order_with_the_words_before() {
        let found = anchors(
            "# A long title of the note\n\nSee [the *club* page](https://example.org) and\n[mail](mailto:a@example.org).\n",
        );
        assert_eq!(
            found,
            [
                row("target note-x", "", "A long title of"),
                row("link https://example.org", "See", "the club page"),
                row("link mailto:a@example.org", "the club page and", "mail"),
            ]
        );
    }

    #[test]
    fn footnotes_their_references_and_the_links_back() {
        let found = anchors("Tuner[^1] text.\n\n[^1]: The tuner is from a kit.\n");
        assert_eq!(
            found,
            [
                row("target note-x", "", "Tuner1 text."),
                row("link #fn-1 id fnref-1", "Tuner", "1"),
                row("target fn-1", "", "The tuner is from"),
                row("link #fnref-1", "is from a kit.", "↩"),
            ]
        );
    }

    #[test]
    fn a_footnote_used_twice_has_two_links_back() {
        let found = anchors("A[^n] and B[^n].\n\n[^n]: Note.\n");
        let links: Vec<&str> = found.iter().map(|(what, ..)| what.as_str()).collect();
        assert_eq!(
            links,
            [
                "target note-x",
                "link #fn-n id fnref-n",
                "link #fn-n id fnref-n-2",
                "target fn-n",
                "link #fnref-n",
                "link #fnref-n-2",
            ]
        );
        assert_eq!(found[5].2, "↩2");
    }

    #[test]
    fn a_picture_alone_is_no_anchor_and_a_list_gives_its_first_words() {
        let found = anchors("- [![p](a.png)](https://example.org)\n\nAfter the list.\n");
        assert_eq!(found, [row("target note-x", "", "After the list.")]);
        let found = anchors("> - First item here\n");
        assert_eq!(found, [row("target note-x", "", "First item here")]);
    }

    #[test]
    fn contents_entries_have_the_whole_title() {
        let entry = contents_entry("Packing list for  Field Day", "note-p");
        assert_eq!(entry.text, "Packing list for Field Day");
        assert!(entry.moves_on());
    }
}
