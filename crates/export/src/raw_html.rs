// SPDX-License-Identifier: Apache-2.0
//! Raw HTML in an exported note (ADR-0021 rule 9).
//!
//! Formatting elements without attributes stay; everything else goes and its
//! text stays, except the content of scripts and the like, which is not text.
//! Afterwards the tree has no HTML blocks, and its inline HTML is only the
//! allowed tags, balanced within their parent. A format without them can
//! ignore those nodes and has the text.

use std::fmt::Write as _;

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};

/// Elements that stay, without their attributes.
const ALLOWED: &[&str] = &[
    "b", "i", "u", "s", "sub", "sup", "mark", "kbd", "br", "span", "small",
];

/// Elements left out with what they contain.
const WITHOUT_CONTENT: &[&str] = &[
    "iframe", "noembed", "noframes", "object", "script", "style", "template",
];

/// Elements that end a paragraph of the text kept from an HTML block.
const BREAKS: &[&str] = &[
    "address",
    "article",
    "aside",
    "blockquote",
    "dd",
    "details",
    "div",
    "dl",
    "dt",
    "figcaption",
    "figure",
    "footer",
    "form",
    "h1",
    "h2",
    "h3",
    "h4",
    "h5",
    "h6",
    "header",
    "hr",
    "li",
    "main",
    "nav",
    "ol",
    "p",
    "pre",
    "section",
    "summary",
    "table",
    "tr",
    "ul",
];

/// Elements that separate words of the text kept from an HTML block.
const SPACES: &[&str] = &["td", "th"];

/// Cleans all raw HTML below `root`.
pub(crate) fn clean<'a>(arena: &'a Arena<'a>, root: &'a AstNode<'a>) {
    let nodes: Vec<_> = root.descendants().collect();
    for node in &nodes {
        if node
            .children()
            .any(|c| matches!(c.data().value, NodeValue::HtmlInline(_)))
        {
            clean_inlines(arena, node);
        }
    }
    for node in nodes {
        let literal = match &node.data().value {
            NodeValue::HtmlBlock(block) => block.literal.clone(),
            _ => continue,
        };
        for paragraph in block_paragraphs(&literal) {
            let new: &'a AstNode<'a> = arena.alloc(NodeValue::Paragraph.into());
            for piece in paragraph {
                new.append(alloc_piece(arena, piece));
            }
            node.insert_before(new);
        }
        node.detach();
    }
}

fn alloc_piece<'a>(arena: &'a Arena<'a>, piece: Piece) -> &'a AstNode<'a> {
    let value = match piece {
        Piece::Text(text) => NodeValue::Text(text.into()),
        Piece::Html(html) => NodeValue::HtmlInline(html),
    };
    arena.alloc(value.into())
}

/// Cleans the inline HTML among the children of `parent`, where each node
/// holds one tag or comment.
fn clean_inlines<'a>(arena: &'a Arena<'a>, parent: &'a AstNode<'a>) {
    let mut state = Sanitizer::default();
    for child in parent.children().collect::<Vec<_>>() {
        let literal = if let NodeValue::HtmlInline(literal) = &child.data().value {
            literal.clone()
        } else {
            if state.dropping.is_some() {
                child.detach();
            }
            continue;
        };
        let kept: String = tokens(&literal)
            .iter()
            .filter_map(|token| match token {
                Token::Tag(tag) => state.tag(tag),
                Token::Text(_) => None,
            })
            .collect();
        if kept.is_empty() {
            child.detach();
        } else {
            child.data_mut().value = NodeValue::HtmlInline(kept);
        }
    }
    if let Some(closing) = state.close_all() {
        parent.append(alloc_piece(arena, Piece::Html(closing)));
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Piece {
    Text(String),
    Html(String),
}

/// The text and allowed tags of an HTML block, as paragraphs.
fn block_paragraphs(html: &str) -> Vec<Vec<Piece>> {
    let mut state = Sanitizer::default();
    let mut paragraphs = Vec::new();
    let mut current = Vec::new();
    for token in tokens(html) {
        match token {
            Token::Text(_) if state.dropping.is_some() => {}
            Token::Text(text) => {
                current.push(Piece::Text(collapse_spaces(&decode_entities(&text))));
            }
            Token::Tag(tag) => {
                let dropping = state.dropping.is_some();
                match state.tag(&tag) {
                    Some(html) => current.push(Piece::Html(html)),
                    None if dropping => {}
                    None if BREAKS.contains(&tag.name.as_str()) => {
                        finish_paragraph(&mut state, &mut current, &mut paragraphs);
                    }
                    None if !tag.closing && SPACES.contains(&tag.name.as_str()) => {
                        current.push(Piece::Text(" ".to_owned()));
                    }
                    None => {}
                }
            }
        }
    }
    finish_paragraph(&mut state, &mut current, &mut paragraphs);
    paragraphs
}

fn finish_paragraph(state: &mut Sanitizer, current: &mut Vec<Piece>, out: &mut Vec<Vec<Piece>>) {
    if let Some(closing) = state.close_all() {
        current.push(Piece::Html(closing));
    }
    let mut pieces = std::mem::take(current);
    if let Some(Piece::Text(first)) = pieces.first_mut() {
        *first = first.trim_start().to_owned();
    }
    if let Some(Piece::Text(last)) = pieces.last_mut() {
        *last = last.trim_end().to_owned();
    }
    let has_text = pieces
        .iter()
        .any(|p| matches!(p, Piece::Text(t) if !t.trim().is_empty()));
    if has_text {
        out.push(pieces);
    }
}

fn collapse_spaces(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut space = false;
    for c in text.chars() {
        if c.is_ascii_whitespace() {
            space = true;
        } else {
            if space {
                out.push(' ');
            }
            space = false;
            out.push(c);
        }
    }
    if space {
        out.push(' ');
    }
    out
}

/// Replaces character references. An unknown one stays as written.
fn decode_entities(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(start) = rest.find('&') {
        out.push_str(&rest[..start]);
        rest = &rest[start..];
        let end = rest.find(';').filter(|&end| {
            let name = rest[1..end].strip_prefix('#').unwrap_or(&rest[1..end]);
            !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric())
        });
        let Some(end) = end else {
            out.push('&');
            rest = &rest[1..];
            continue;
        };
        out.push_str(&decode_entity(&rest[..=end]));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The text of the character reference `entity`, known to be only `&`, `#`,
/// letters, digits and `;`. Comrak's table of the HTML names is not public,
/// but parsing such a reference as Markdown can only give its text.
fn decode_entity(entity: &str) -> String {
    let arena = Arena::new();
    let root = parse_document(&arena, entity, &Options::default());
    let text = root
        .descendants()
        .find_map(|node| match &node.data().value {
            NodeValue::Text(text) => Some(text.to_string()),
            _ => None,
        });
    text.unwrap_or_else(|| entity.to_owned())
}

/// Decides which tags stay, and keeps them balanced.
#[derive(Default)]
struct Sanitizer {
    /// The allowed elements open now.
    open: Vec<String>,
    /// The element whose content is being left out.
    dropping: Option<String>,
}

impl Sanitizer {
    /// The HTML to write for `tag`, if any.
    fn tag(&mut self, tag: &Tag) -> Option<String> {
        let name = tag.name.as_str();
        if let Some(dropping) = &self.dropping {
            if tag.closing && dropping == name {
                self.dropping = None;
            }
            return None;
        }
        if WITHOUT_CONTENT.contains(&name) {
            if !tag.closing && !tag.self_closing {
                self.dropping = Some(tag.name.clone());
            }
            return None;
        }
        if !ALLOWED.contains(&name) {
            return None;
        }
        if name == "br" {
            return (!tag.closing).then(|| "<br>".to_owned());
        }
        if tag.closing {
            let at = self.open.iter().rposition(|open| open == name)?;
            return Some(self.close_from(at));
        }
        if tag.self_closing {
            return None;
        }
        self.open.push(tag.name.clone());
        Some(format!("<{name}>"))
    }

    /// The closing tags for what is still open, ending it all.
    fn close_all(&mut self) -> Option<String> {
        (!self.open.is_empty()).then(|| self.close_from(0))
    }

    fn close_from(&mut self, at: usize) -> String {
        let mut out = String::new();
        for name in self.open.drain(at..).rev() {
            let _ = write!(out, "</{name}>");
        }
        out
    }
}

#[derive(Debug, PartialEq, Eq)]
enum Token {
    Text(String),
    Tag(Tag),
}

#[derive(Debug, PartialEq, Eq)]
struct Tag {
    /// Lowercase.
    name: String,
    closing: bool,
    self_closing: bool,
}

/// Splits HTML into text and tags. Comments, declarations and processing
/// instructions are left out; a `<` that starts none of these is text.
fn tokens(html: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut text = String::new();
    let mut rest = html;
    while let Some(start) = rest.find('<') {
        text.push_str(&rest[..start]);
        rest = &rest[start..];
        let skip = [
            ("<!--", "-->"),
            ("<![CDATA[", "]]>"),
            ("<!", ">"),
            ("<?", ">"),
        ]
        .iter()
        .find(|(open, _)| rest.starts_with(open))
        .map(|(open, close)| {
            rest[open.len()..]
                .find(close)
                .map_or(rest.len(), |end| open.len() + end + close.len())
        });
        if let Some(skip) = skip {
            rest = &rest[skip..];
        } else if let Some((tag, len)) = parse_tag(rest) {
            if !text.is_empty() {
                out.push(Token::Text(std::mem::take(&mut text)));
            }
            out.push(Token::Tag(tag));
            rest = &rest[len..];
        } else {
            text.push('<');
            rest = &rest[1..];
        }
    }
    text.push_str(rest);
    if !text.is_empty() {
        out.push(Token::Text(text));
    }
    out
}

/// The tag at the start of `html`, which starts with `<`, and its length.
fn parse_tag(html: &str) -> Option<(Tag, usize)> {
    let after = html.get(1..)?;
    let (closing, after) = match after.strip_prefix('/') {
        Some(after) => (true, after),
        None => (false, after),
    };
    if !after.starts_with(|c: char| c.is_ascii_alphabetic()) {
        return None;
    }
    let name_len = after
        .find(|c: char| !(c.is_ascii_alphanumeric() || c == '-' || c == ':'))
        .unwrap_or(after.len());
    let mut quote = None;
    for (i, c) in after[name_len..].char_indices() {
        match (quote, c) {
            (Some(q), c) if q == c => quote = None,
            (None, '"' | '\'') => quote = Some(c),
            (None, '>') => {
                let end = name_len + i;
                let tag = Tag {
                    name: after[..name_len].to_ascii_lowercase(),
                    closing,
                    self_closing: after[..end].ends_with('/'),
                };
                return Some((tag, html.len() - after.len() + end + 1));
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_of(html: &str) -> Vec<String> {
        block_paragraphs(html)
            .into_iter()
            .map(|pieces| {
                pieces
                    .into_iter()
                    .map(|p| match p {
                        Piece::Text(t) | Piece::Html(t) => t,
                    })
                    .collect()
            })
            .collect()
    }

    #[test]
    fn scripts_go_with_their_content_and_other_elements_leave_their_text() {
        assert!(text_of("<script>alert(\"no\")</script>").is_empty());
        assert!(text_of("<STYLE type='x'>p { }</STYLE>").is_empty());
        assert!(text_of("<iframe src=\"https://example.org\"></iframe>").is_empty());
        assert_eq!(
            text_of("<div onclick=\"alert(1)\">Its text stays.</div>"),
            ["Its text stays."]
        );
        assert_eq!(text_of("<!-- a comment --><p>After</p>"), ["After"]);
    }

    #[test]
    fn allowed_elements_lose_their_attributes_and_stay_balanced() {
        assert_eq!(
            text_of("<div><span style=\"color:red\" onclick='x>y'>red</span> <b>bold</div>"),
            ["<span>red</span> <b>bold</b>"]
        );
        assert_eq!(text_of("<p></i>a<br/>b</p>"), ["a<br>b"]);
        assert_eq!(text_of("<p><b><i>a</b>c</i></p>"), ["<b><i>a</i></b>c"]);
    }

    #[test]
    fn block_elements_split_paragraphs_and_cells_keep_words_apart() {
        assert_eq!(text_of("<div>a</div>\n<div>b</div>"), ["a", "b"]);
        assert_eq!(
            text_of("<table><tr><td>a</td><td>b</td></tr>\n<tr><td>c</td></tr></table>"),
            ["a b", "c"]
        );
    }

    #[test]
    fn character_references_become_text() {
        assert_eq!(
            text_of("<p>Gr&uuml;&szlig;e &amp; &#228; &#xE4; &nosuch; R&D &lt;b&gt;</p>"),
            ["Grüße & ä ä &nosuch; R&D <b>"]
        );
        assert_eq!(text_of("<p>1 < 2 <3</p>"), ["1 < 2 <3"]);
    }

    #[test]
    fn inline_html_in_a_markdown_note() {
        let html = |markdown: &str| {
            let arena = Arena::new();
            let mut options = Options::default();
            options.render.r#unsafe = true;
            let root = parse_document(&arena, markdown, &options);
            clean(&arena, root);
            let mut out = String::new();
            comrak::format_html(root, &options, &mut out).unwrap();
            out
        };
        assert_eq!(
            html("a <script>**not** text</script> <b onclick=\"x\">b</b> *c <i>d* e</i>\n"),
            "<p>a  <b>b</b> <em>c <i>d</i></em> e</p>\n"
        );
        assert_eq!(
            html("<b>open\n\nnext\n"),
            "<p><b>open</b></p>\n<p>next</p>\n"
        );
        assert_eq!(
            html("- <div>in a list</div>\n"),
            "<ul>\n<li>in a list</li>\n</ul>\n"
        );
    }
}
