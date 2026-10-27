// SPDX-License-Identifier: Apache-2.0
//! The search query language (HLD §7), parsed into an AST.
//!
//! Terms: words (matched as prefixes), `"phrases"`, `#tags` or `tag:x` (a tag
//! also matches its subtags), `created:` and `modified:` with a date, period
//! or range (see [`DateSpec`]), `has:image|task|link`, `is:pinned|untagged`
//! and `in:trash`. `-term` excludes, `OR` and parentheses combine; terms next
//! to each other must all match. The reference for users is
//! `docs/manual/query-language.md`.
//!
//! The parser never fails: a missing `)` closes at the end, a stray `)` or a
//! lone `-` is ignored, an operator with a value it does not know (`has:x`,
//! `created:soon`) is searched as a word, and an empty query gives `None`.

use crate::dates::DateSpec;
use crate::tags::normalize;

/// Which date of a note a date term looks at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateField {
    /// Front matter `created`, else the file time.
    Created,
    /// Front matter `modified`, else the file time.
    Modified,
}

/// The dates a date term accepts.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DateBound {
    /// Within the period: `created:2026-10`.
    Within(DateSpec),
    /// After the period, or from its start if inclusive: `>2026`, `>=2026`.
    After(DateSpec, bool),
    /// Before the period, or until its end if inclusive: `<2026`, `<=2026`.
    Before(DateSpec, bool),
    /// From the start of the first to the end of the second; either may be
    /// open: `2026-01..2026-03`, `2026-10..`, `..2025`.
    Between(Option<DateSpec>, Option<DateSpec>),
}

/// What `has:` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Feature {
    /// An image.
    Image,
    /// A task list item.
    Task,
    /// A link to a note, a file or the web.
    Link,
}

/// What `is:` asks for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Flag {
    /// Front matter `pinned: true`.
    Pinned,
    /// No tags.
    Untagged,
}

/// A parsed query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// A word; matches words starting with it (diacritics and case ignored).
    Word(String),
    /// Words in this order.
    Phrase(String),
    /// A tag key (normalized, format.md §5.3); matches its subtags too.
    Tag(String),
    /// A date of the note in a period.
    Date(DateField, DateBound),
    /// The note has this.
    Has(Feature),
    /// The note is this.
    Is(Flag),
    /// The note is in the trash; without this term the trash is not searched.
    InTrash,
    /// Notes not matching.
    Not(Box<Query>),
    /// Notes matching all.
    And(Vec<Query>),
    /// Notes matching any.
    Or(Vec<Query>),
}

impl Query {
    /// Parses `text`; `None` if it holds no term.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let tokens = tokenize(text);
        let mut parser = Parser { tokens, pos: 0 };
        let mut parts = Vec::new();
        // A stray `)` ends `or()` early; skip it and go on.
        while parser.pos < parser.tokens.len() {
            if let Some(q) = parser.or() {
                parts.push(q);
            }
            if parser.peek() == Some(&Token::Close) {
                parser.pos += 1;
            }
        }
        combine(parts, Query::And)
    }

    /// The words and phrases that must match (not under a `-`), for ranking
    /// and snippets.
    #[must_use]
    pub fn positive_terms(&self) -> Vec<&Self> {
        let mut out = Vec::new();
        self.collect_positive(&mut out);
        out
    }

    fn collect_positive<'a>(&'a self, out: &mut Vec<&'a Self>) {
        match self {
            Self::Word(_) | Self::Phrase(_) => out.push(self),
            Self::And(parts) | Self::Or(parts) => {
                for part in parts {
                    part.collect_positive(out);
                }
            }
            Self::Tag(_)
            | Self::Not(_)
            | Self::Date(..)
            | Self::Has(_)
            | Self::Is(_)
            | Self::InTrash => {}
        }
    }

    /// `true` if `in:trash` occurs anywhere, so the search covers the trash.
    #[must_use]
    pub fn mentions_trash(&self) -> bool {
        match self {
            Self::InTrash => true,
            Self::Not(inner) => inner.mentions_trash(),
            Self::And(parts) | Self::Or(parts) => parts.iter().any(Self::mentions_trash),
            _ => false,
        }
    }
}

/// An operator term such as `created:2026` or `has:task`; `None` if `word`
/// is not one this parser knows.
fn operator(word: &str) -> Option<Query> {
    let (name, value) = word.split_once(':')?;
    if value.is_empty() {
        return None;
    }
    match name.to_ascii_lowercase().as_str() {
        "tag" => {
            let tag = value.trim_start_matches('#').trim_end_matches(['/', '-']);
            tag.chars()
                .any(char::is_alphabetic)
                .then(|| Query::Tag(normalize(tag)))
        }
        "created" => date_bound(value).map(|b| Query::Date(DateField::Created, b)),
        "modified" => date_bound(value).map(|b| Query::Date(DateField::Modified, b)),
        "has" => match value.to_ascii_lowercase().as_str() {
            "image" => Some(Query::Has(Feature::Image)),
            "task" => Some(Query::Has(Feature::Task)),
            "link" => Some(Query::Has(Feature::Link)),
            _ => None,
        },
        "is" => match value.to_ascii_lowercase().as_str() {
            "pinned" => Some(Query::Is(Flag::Pinned)),
            "untagged" => Some(Query::Is(Flag::Untagged)),
            _ => None,
        },
        "in" => value
            .eq_ignore_ascii_case("trash")
            .then_some(Query::InTrash),
        _ => None,
    }
}

fn date_bound(value: &str) -> Option<DateBound> {
    if let Some((from, to)) = value.split_once("..") {
        let side = |s: &str| {
            if s.is_empty() {
                Ok(None)
            } else {
                DateSpec::parse(s).map(Some).ok_or(())
            }
        };
        let (from, to) = (side(from).ok()?, side(to).ok()?);
        return (from.is_some() || to.is_some()).then_some(DateBound::Between(from, to));
    }
    for (prefix, make) in [
        (
            ">=",
            (|s| DateBound::After(s, true)) as fn(DateSpec) -> DateBound,
        ),
        ("<=", |s| DateBound::Before(s, true)),
        (">", |s| DateBound::After(s, false)),
        ("<", |s| DateBound::Before(s, false)),
    ] {
        if let Some(rest) = value.strip_prefix(prefix) {
            return DateSpec::parse(rest).map(make);
        }
    }
    DateSpec::parse(value).map(DateBound::Within)
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Open,
    Close,
    Or,
    Minus,
    Word(String),
    Phrase(String),
    Tag(String),
    Operator(Query),
}

fn tokenize(text: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut chars = text.chars().peekable();
    while let Some(&c) = chars.peek() {
        match c {
            c if c.is_whitespace() => {
                chars.next();
            }
            '(' => {
                chars.next();
                tokens.push(Token::Open);
            }
            ')' => {
                chars.next();
                tokens.push(Token::Close);
            }
            '-' => {
                chars.next();
                tokens.push(Token::Minus);
            }
            '"' => {
                chars.next();
                let phrase: String = chars.by_ref().take_while(|&c| c != '"').collect();
                if phrase.chars().any(char::is_alphanumeric) {
                    tokens.push(Token::Phrase(
                        phrase.split_whitespace().collect::<Vec<_>>().join(" "),
                    ));
                }
            }
            _ => {
                let mut word = String::new();
                while let Some(&c) = chars.peek() {
                    if c.is_whitespace() || matches!(c, '(' | ')' | '"') {
                        break;
                    }
                    word.push(c);
                    chars.next();
                }
                if word == "OR" {
                    tokens.push(Token::Or);
                } else if let Some(term) = operator(&word) {
                    tokens.push(Token::Operator(term));
                } else if let Some(tag) = word.strip_prefix('#') {
                    let tag = tag.trim_end_matches(['/', '-']);
                    if tag.chars().any(char::is_alphabetic) {
                        tokens.push(Token::Tag(normalize(tag)));
                    }
                } else if word.chars().any(char::is_alphanumeric) {
                    tokens.push(Token::Word(word));
                }
            }
        }
    }
    tokens
}

struct Parser {
    tokens: Vec<Token>,
    pos: usize,
}

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos)
    }

    /// `and ("OR" and)*`
    fn or(&mut self) -> Option<Query> {
        let mut parts: Vec<Query> = self.and().into_iter().collect();
        while self.peek() == Some(&Token::Or) {
            self.pos += 1;
            parts.extend(self.and());
        }
        combine(parts, Query::Or)
    }

    /// `unary+`, up to `OR`, `)` or the end
    fn and(&mut self) -> Option<Query> {
        let mut parts = Vec::new();
        while let Some(token) = self.peek() {
            if matches!(token, Token::Or | Token::Close) {
                break;
            }
            parts.extend(self.unary());
        }
        combine(parts, Query::And)
    }

    /// `"-" unary | primary`
    fn unary(&mut self) -> Option<Query> {
        if self.peek() == Some(&Token::Minus) {
            self.pos += 1;
            return match self.peek() {
                None | Some(Token::Or | Token::Close) => None, // a lone `-`
                _ => self.unary().map(|q| Query::Not(Box::new(q))),
            };
        }
        self.primary()
    }

    /// `"(" or ")" | word | phrase | tag`
    fn primary(&mut self) -> Option<Query> {
        let token = self.tokens.get(self.pos).cloned()?;
        self.pos += 1;
        match token {
            Token::Open => {
                let inner = self.or();
                if self.peek() == Some(&Token::Close) {
                    self.pos += 1;
                }
                inner
            }
            Token::Word(w) => Some(Query::Word(w)),
            Token::Phrase(p) => Some(Query::Phrase(p)),
            Token::Tag(t) => Some(Query::Tag(t)),
            Token::Operator(q) => Some(q),
            Token::Close | Token::Or | Token::Minus => None,
        }
    }
}

/// One part as it is, several joined with `join`, none as `None`.
fn combine(mut parts: Vec<Query>, join: fn(Vec<Query>) -> Query) -> Option<Query> {
    match parts.len() {
        0 => None,
        1 => parts.pop(),
        _ => Some(join(parts)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn w(s: &str) -> Query {
        Query::Word(s.into())
    }
    fn tag(s: &str) -> Query {
        Query::Tag(s.into())
    }
    fn not(q: Query) -> Query {
        Query::Not(Box::new(q))
    }

    #[test]
    fn terms_and_operators() {
        assert_eq!(Query::parse("antenne"), Some(w("antenne")));
        assert_eq!(
            Query::parse("antenne tuner"),
            Some(Query::And(vec![w("antenne"), w("tuner")]))
        );
        assert_eq!(
            Query::parse("\"40  m band\" #Radio/QRP -#done"),
            Some(Query::And(vec![
                Query::Phrase("40 m band".into()),
                tag("radio/qrp"),
                not(tag("done")),
            ]))
        );
        assert_eq!(
            Query::parse("a b OR c"),
            Some(Query::Or(vec![Query::And(vec![w("a"), w("b")]), w("c")]))
        );
        assert_eq!(
            Query::parse("a (b OR c) -(d e)"),
            Some(Query::And(vec![
                w("a"),
                Query::Or(vec![w("b"), w("c")]),
                not(Query::And(vec![w("d"), w("e")])),
            ]))
        );
        assert_eq!(Query::parse("Lötzinn"), Some(w("Lötzinn")));
        assert_eq!(
            Query::parse("or"),
            Some(w("or")),
            "only OR in capitals is an operator"
        );
    }

    #[test]
    fn never_fails() {
        for (text, expected) in [
            ("", None),
            ("   ", None),
            ("-", None),
            ("a -", Some(w("a"))),
            ("(a", Some(w("a"))),
            ("a) b", Some(Query::And(vec![w("a"), w("b")]))),
            ("OR a OR", Some(w("a"))),
            (
                "\"unclosed phrase",
                Some(Query::Phrase("unclosed phrase".into())),
            ),
            ("\"\" # #/ -- ...", None),
        ] {
            assert_eq!(Query::parse(text), expected, "{text:?}");
        }
    }

    #[test]
    fn positive_terms_skip_exclusions_and_tags() {
        let q = Query::parse("a -b (c OR \"d e\") #t -(f)").unwrap();
        let terms = q.positive_terms();
        assert_eq!(terms, [&w("a"), &w("c"), &Query::Phrase("d e".into())]);
    }

    #[test]
    fn operators() {
        use crate::dates::{AgoUnit, DateSpec as D};
        let date = |f, b| Query::Date(f, b);
        assert_eq!(Query::parse("tag:Radio/QRP"), Some(tag("radio/qrp")));
        assert_eq!(Query::parse("tag:#chor"), Some(tag("chor")));
        assert_eq!(
            Query::parse("created:2026-10"),
            Some(date(
                DateField::Created,
                DateBound::Within(D::Month(2026, 10))
            ))
        );
        assert_eq!(
            Query::parse("modified:>=2026-01-01"),
            Some(date(
                DateField::Modified,
                DateBound::After(D::Day(2026, 1, 1), true)
            ))
        );
        assert_eq!(
            Query::parse("MODIFIED:<yesterday"),
            Some(date(
                DateField::Modified,
                DateBound::Before(D::Yesterday, false)
            ))
        );
        assert_eq!(
            Query::parse("created:2026-01..2026-03"),
            Some(date(
                DateField::Created,
                DateBound::Between(Some(D::Month(2026, 1)), Some(D::Month(2026, 3)))
            ))
        );
        assert_eq!(
            Query::parse("created:..2025"),
            Some(date(
                DateField::Created,
                DateBound::Between(None, Some(D::Year(2025)))
            ))
        );
        assert_eq!(
            Query::parse("modified:7d"),
            Some(date(
                DateField::Modified,
                DateBound::Within(D::Ago(7, AgoUnit::Day))
            ))
        );
        assert_eq!(
            Query::parse("has:task -is:pinned in:trash"),
            Some(Query::And(vec![
                Query::Has(Feature::Task),
                not(Query::Is(Flag::Pinned)),
                Query::InTrash
            ]))
        );
        // Unknown operators or values are words.
        for text in [
            "has:music",
            "created:soon",
            "created:..",
            "foo:bar",
            "is:",
            "in:archive",
        ] {
            assert_eq!(Query::parse(text), Some(w(text)), "{text}");
        }
    }

    #[test]
    fn trash_is_searched_only_when_asked() {
        assert!(Query::parse("a in:trash").unwrap().mentions_trash());
        assert!(Query::parse("a OR (b -in:trash)").unwrap().mentions_trash());
        assert!(!Query::parse("a #trash").unwrap().mentions_trash());
    }
}
