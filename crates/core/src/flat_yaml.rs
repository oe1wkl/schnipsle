// SPDX-License-Identifier: Apache-2.0
//! The small YAML subset of the app's own files (`vault.yaml`, trash records):
//! one top-level `key: value` per line, values plain, single- or double-quoted,
//! optionally followed by a `# comment`.

use std::fmt::Write as _;

/// A YAML double-quoted scalar.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The value of the top-level `key`; `None` if the key is missing or its
/// quoted value is not closed.
pub(crate) fn value(text: &str, key: &str) -> Option<String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let raw = text
        .lines()
        .find_map(|line| line.strip_prefix(key)?.strip_prefix(':'))?
        .trim();
    if let Some(rest) = raw.strip_prefix('"') {
        return double_quoted(rest);
    }
    if let Some(rest) = raw.strip_prefix('\'') {
        return single_quoted(rest);
    }
    if raw.starts_with('#') {
        return Some(String::new());
    }
    let end = raw
        .find(" #")
        .or_else(|| raw.find("\t#"))
        .unwrap_or(raw.len());
    Some(raw[..end].trim_end().to_owned())
}

/// The content of a double-quoted scalar; `rest` follows the opening quote.
fn double_quoted(rest: &str) -> Option<String> {
    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars();
    while let Some(c) = chars.next() {
        match c {
            '"' => return Some(out),
            '\\' => match chars.next()? {
                'n' => out.push('\n'),
                't' => out.push('\t'),
                'u' => {
                    let hex: String = chars.by_ref().take(4).collect();
                    out.extend(u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32));
                }
                other => out.push(other),
            },
            c => out.push(c),
        }
    }
    None
}

/// The content of a single-quoted scalar, where `''` stands for `'`.
fn single_quoted(rest: &str) -> Option<String> {
    let mut out = String::with_capacity(rest.len());
    let mut chars = rest.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\'' {
            out.push(c);
        } else if chars.next_if_eq(&'\'').is_some() {
            out.push('\'');
        } else {
            return Some(out);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_quoted_and_commented_values() {
        let text = "\u{feff}format: 1   # integer\nid: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T\n\
                    name: 'Willi''s notes' # display\npath: \"a \\\"b\\\"\\n\\u00e4\" # c\n\
                    empty:\nnote: # only a comment\n  nested: x\n";
        assert_eq!(value(text, "format").as_deref(), Some("1"));
        assert_eq!(
            value(text, "id").as_deref(),
            Some("01JA7Q5Y0Z8H3K2M6N4P9R1S2T")
        );
        assert_eq!(value(text, "name").as_deref(), Some("Willi's notes"));
        assert_eq!(value(text, "path").as_deref(), Some("a \"b\"\nä"));
        assert_eq!(value(text, "empty").as_deref(), Some(""));
        assert_eq!(value(text, "note").as_deref(), Some(""));
        assert_eq!(value(text, "nested"), None); // not top-level
        assert_eq!(value(text, "missing"), None);
        assert_eq!(value("name: \"open", "name"), None);
        assert_eq!(value("name: 'open", "name"), None);
        assert_eq!(value("ids: x", "id"), None);
    }

    #[test]
    fn quote_round_trips() {
        for s in [
            "plain",
            "with \"quotes\" and \\",
            "line\nbreak",
            "tab\tbell\u{7}",
            "Ünïcødé",
        ] {
            let line = format!("k: {}\n", quote(s));
            assert_eq!(value(&line, "k").as_deref(), Some(s), "{line}");
        }
    }
}
