// SPDX-License-Identifier: Apache-2.0
//! The search query language (HLD §7), parsed into an AST.
//!
//! M2 understands: words (matched as prefixes), `"phrases"`, `#tags` (a tag
//! also matches its subtags), `-term` to exclude, `OR`, and parentheses.
//! Terms next to each other must all match. The parser never fails: a missing
//! `)` closes at the end, a stray `)` or a lone `-` is ignored, and an empty
//! query gives `None`. M3 adds `created:`, `modified:`, `has:`, `is:` and
//! `in:` on the same parser, for saved searches too.

use crate::tags::normalize;

/// A parsed query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Query {
    /// A word; matches words starting with it (diacritics and case ignored).
    Word(String),
    /// Words in this order.
    Phrase(String),
    /// A tag key (normalized, format.md §5.3); matches its subtags too.
    Tag(String),
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
            Self::Tag(_) | Self::Not(_) => {}
        }
    }
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
}
