// SPDX-License-Identifier: Apache-2.0
//! Hashtag extraction as specified in `docs/format.md` §5.3 and ADR-0008.
//!
//! Tags are taken from the visible text of a note only: code spans, code blocks
//! (including `abc` and `chordpro` blocks), raw HTML, math, image descriptions,
//! link destinations and the front matter never yield tags.

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, Options, parse_document};
use serde::Serialize;
use unicode_normalization::UnicodeNormalization;

/// Placeholder pushed into the scan buffer for inline content that must not
/// contribute tag characters (code span, HTML, math, image, escaped character).
/// It is not a tag character and does not block a following `#`.
const BOUNDARY: char = '\u{0}';

/// A hashtag found in a note.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Tag {
    /// The tag as written, without the leading `#`, e.g. `Projects/Tuner`.
    pub name: String,
    /// Normalized form used for grouping: NFC, then lowercase, e.g. `projects/tuner`.
    pub key: String,
}

impl Tag {
    fn new(name: &str) -> Self {
        Self {
            name: name.to_owned(),
            key: normalize(name),
        }
    }

    /// The keys of this tag and all its hierarchy ancestors, outermost first:
    /// `a/b/c` yields `a`, `a/b`, `a/b/c`.
    #[must_use]
    pub fn self_and_ancestor_keys(&self) -> Vec<String> {
        let mut keys = Vec::new();
        let mut end = 0;
        for segment in self.key.split('/') {
            end += segment.len();
            keys.push(self.key[..end].to_owned());
            end += 1; // the '/'
        }
        keys
    }
}

/// Normalizes a tag name for grouping (format.md §5.3): Unicode NFC, then lowercase.
#[must_use]
pub fn normalize(name: &str) -> String {
    name.nfc().collect::<String>().to_lowercase()
}

/// Extracts the hashtags of a Markdown note, in order of first appearance,
/// without duplicates (by normalized key; the first spelling is kept).
#[must_use]
pub fn extract_tags(markdown: &str) -> Vec<Tag> {
    let arena = Arena::new();
    let root = parse_document(&arena, markdown, &parse_options());

    let mut found = Vec::new();
    let mut buffer = String::new();
    walk(root, &mut buffer, &mut found);
    flush(&mut buffer, &mut found);

    let mut seen = std::collections::HashSet::new();
    found.retain(|tag| seen.insert(tag.key.clone()));
    found
}

/// The Markdown dialect of format.md §5.1, as far as it matters for tag extraction.
pub(crate) fn parse_options() -> Options<'static> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options.extension.math_dollars = true;
    options.extension.wikilinks_title_after_pipe = true;
    options.extension.front_matter_delimiter = Some("---".to_owned());
    options.parse.escaped_char_spans = true;
    options
}

fn walk<'a>(node: &'a AstNode<'a>, buffer: &mut String, found: &mut Vec<Tag>) {
    let value = &node.data.borrow().value;
    match value {
        NodeValue::Text(text) => buffer.push_str(text),
        NodeValue::SoftBreak | NodeValue::LineBreak => buffer.push('\n'),
        // Inline content that is visible but must not produce tags.
        NodeValue::Code(_)
        | NodeValue::HtmlInline(_)
        | NodeValue::Math(_)
        | NodeValue::Image(_)
        | NodeValue::Escaped
        | NodeValue::FootnoteReference(_) => buffer.push(BOUNDARY),
        // Blocks whose content is never scanned.
        NodeValue::CodeBlock(_) | NodeValue::HtmlBlock(_) | NodeValue::FrontMatter(_) => {
            flush(buffer, found);
        }
        _ if value.block() => {
            flush(buffer, found);
            for child in node.children() {
                walk(child, buffer, found);
            }
            flush(buffer, found);
        }
        // Inline containers (emphasis, links, wiki links, …): scan their text.
        _ => {
            for child in node.children() {
                walk(child, buffer, found);
            }
        }
    }
}

fn flush(buffer: &mut String, found: &mut Vec<Tag>) {
    if !buffer.is_empty() {
        // NFC first, so a letter written as base + combining mark stays one tag character.
        let text: String = buffer.nfc().collect();
        scan_text(&text, found);
        buffer.clear();
    }
}

fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-'
}

/// Characters that, directly before `#`, prevent a tag (word boundary rule).
fn blocks_tag(c: char) -> bool {
    c.is_alphanumeric() || matches!(c, '_' | '&' | '/' | '#')
}

/// Scans plain text (already free of code, HTML and link destinations) for tags.
fn scan_text(text: &str, found: &mut Vec<Tag>) {
    let chars: Vec<char> = text.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '#' || (i > 0 && blocks_tag(chars[i - 1])) {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        loop {
            while end < chars.len() && is_tag_char(chars[end]) {
                end += 1;
            }
            // A '/' continues the tag only if a tag character follows it.
            if end + 1 < chars.len() && chars[end] == '/' && is_tag_char(chars[end + 1]) {
                end += 1;
            } else {
                break;
            }
        }
        let raw: String = chars[start..end].iter().collect();
        let name = raw.trim_end_matches(['-', '/']);
        if is_valid_name(name) {
            found.push(Tag::new(name));
        }
        i = end.max(i + 1);
    }
}

fn is_valid_name(name: &str) -> bool {
    if name.is_empty() || !name.chars().any(char::is_alphabetic) {
        return false; // rule 1: at least one letter
    }
    // Rule 2: a hex colour with at least one digit (#00f, #a0b1c2) is not a tag.
    let is_hex_colour = matches!(name.len(), 3 | 6)
        && name.chars().all(|c| c.is_ascii_hexdigit())
        && name.chars().any(|c| c.is_ascii_digit());
    !is_hex_colour
}

#[cfg(test)]
mod tests {
    use super::*;

    fn names(markdown: &str) -> Vec<String> {
        extract_tags(markdown).into_iter().map(|t| t.name).collect()
    }

    // The example table of format.md §5.3, row by row.
    #[test]
    fn spec_examples() {
        assert_eq!(
            names("Antenna test #radio #projects/tuner"),
            ["radio", "projects/tuner"]
        );
        assert!(names("# Heading").is_empty());
        assert!(names("Colour #00ff00 and issue #123").is_empty());
        assert_eq!(names("Grüße an #Österreich"), ["Österreich"]);
        assert!(names("`#notatag`").is_empty());
        assert!(names("https://example.com/page#anchor").is_empty());
        assert!(names("C# and F#").is_empty());
        assert_eq!(names("#cw_training, #m32."), ["cw_training", "m32"]);
        assert_eq!(names("Ideas to #add for the #decade"), ["add", "decade"]);
        assert!(names(r"Not a tag: \#literal").is_empty());
    }

    #[test]
    fn normalization_and_dedup() {
        let tags = extract_tags("#Radio and #radio and #RADIO");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].name, "Radio");
        assert_eq!(tags[0].key, "radio");
        // NFC: "Ö" as O + combining diaeresis groups with the precomposed form.
        let tags = extract_tags("#O\u{308}sterreich #Österreich");
        assert_eq!(tags.len(), 1);
        assert_eq!(tags[0].key, "österreich");
    }

    #[test]
    fn hierarchy() {
        let tags = extract_tags("#a/b/c");
        assert_eq!(tags[0].self_and_ancestor_keys(), ["a", "a/b", "a/b/c"]);
        assert_eq!(names("#radio/ and #cw- end"), ["radio", "cw"]);
        assert_eq!(names("#a//b"), ["a"]);
    }

    #[test]
    fn heading_text_can_carry_tags() {
        assert_eq!(names("# Meeting #radio"), ["radio"]);
        assert_eq!(names("#notaheading"), ["notaheading"]);
    }

    #[test]
    fn excluded_contexts() {
        let md = "\
---
title: x
# a yaml comment #nottag
---
```abc
K:F#
\"F#m\" #inabc
```

```chordpro
# chordpro comment #inchordpro
[F#m]Text
```

    #indented

<div>#inhtml</div>

Inline <span title=\"#inattr\">#visible</span> and $#inmath$ and ![#inalt](a.png) and [link](https://x.org/#frag)

Visible #yes
";
        // Text between inline HTML tags is visible text; the tags themselves are not.
        assert_eq!(names(md), ["visible", "yes"]);
    }

    #[test]
    fn boundaries() {
        assert!(names("foo#bar a&#bar /#bar ##double").is_empty());
        assert_eq!(
            names("(#paren) [#bracket] \"#quote\""),
            ["paren", "bracket", "quote"]
        );
        assert_eq!(names("`code`#after"), ["after"]);
        assert_eq!(names("line one\n#second"), ["second"]);
        assert_eq!(
            names("**#bold** _#emph_ [#linktext](https://x.org)"),
            ["bold", "emph", "linktext"]
        );
        assert_eq!(
            names("- [ ] task #todo\n\n| a |\n|---|\n| #cell |"),
            ["todo", "cell"]
        );
    }

    #[test]
    fn hex_rule() {
        assert!(names("#00f #a0b1c2 #123456").is_empty());
        assert_eq!(names("#fff #abc #cafe"), ["fff", "abc", "cafe"]);
    }

    #[test]
    fn serializes_for_the_ui() {
        let json = serde_json::to_string(&extract_tags("#M32")).unwrap();
        assert_eq!(json, r#"[{"name":"M32","key":"m32"}]"#);
    }
}
