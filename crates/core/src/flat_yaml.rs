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

/// `text` with the top-level `key` set to `value`, or with the key's line
/// removed for `None` (ADR-0039). `value` must be a plain scalar the caller
/// has checked, such as a number or a word. Only the key's first line
/// changes, together with the lines indented under it. A `# comment` after
/// the old value is kept. Every other line stays as it is, comments and
/// unknown keys included. A key the text does not have is added at the end.
pub(crate) fn set(text: &str, key: &str, value: Option<&str>) -> String {
    let (bom, body) = text
        .strip_prefix('\u{feff}')
        .map_or(("", text), |body| ("\u{feff}", body));
    let mut out = String::with_capacity(text.len() + key.len() + 16);
    out.push_str(bom);
    let mut lines = body.split_inclusive('\n').peekable();
    let mut found = false;
    while let Some(line) = lines.next() {
        let raw = if found {
            None
        } else {
            line.strip_prefix(key)
                .and_then(|rest| rest.strip_prefix(':'))
        };
        let Some(raw) = raw else {
            out.push_str(line);
            continue;
        };
        found = true;
        // Lines indented under the key belong to its value.
        while lines
            .next_if(|next| next.starts_with([' ', '\t']))
            .is_some()
        {}
        if let Some(value) = value {
            out.push_str(key);
            out.push_str(": ");
            out.push_str(value);
            if let Some(comment) = comment(raw) {
                // The comment stays in its column if the new value leaves room.
                let column = line[..line.trim_end().len() - comment.len()]
                    .chars()
                    .count();
                let used = key.chars().count() + 2 + value.chars().count();
                out.push_str(&" ".repeat(column.saturating_sub(used).max(1)));
                out.push_str(comment);
            }
            out.push_str(if line.ends_with("\r\n") { "\r\n" } else { "\n" });
        }
    }
    if let (false, Some(value)) = (found, value) {
        if out.len() > bom.len() && !out.ends_with('\n') {
            out.push('\n');
        }
        out.push_str(key);
        out.push_str(": ");
        out.push_str(value);
        out.push('\n');
    }
    out
}

/// The `# comment` after the value in `raw` (what follows `key:`), if any.
fn comment(raw: &str) -> Option<&str> {
    let raw = raw.trim();
    let after = match raw.chars().next()? {
        '#' => return Some(raw),
        '"' => {
            let mut escaped = false;
            let end = raw[1..].find(|c| {
                let close = c == '"' && !escaped;
                escaped = c == '\\' && !escaped;
                close
            })?;
            &raw[end + 2..]
        }
        '\'' => {
            // `''` inside stands for one quote; a closing one is followed by no other.
            let bytes = raw.as_bytes();
            let mut i = 1;
            loop {
                match bytes.get(i)? {
                    b'\'' if bytes.get(i + 1) == Some(&b'\'') => i += 2,
                    b'\'' => break &raw[i + 1..],
                    _ => i += 1,
                }
            }
        }
        _ => {
            let start = raw.find(" #").or_else(|| raw.find("\t#"))?;
            &raw[start..]
        }
    };
    let after = after.trim();
    after.starts_with('#').then_some(after)
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
    fn set_changes_only_the_key_line() {
        let text = "# Schnipsle settings\nhistory_keep_all_days: 7   # a week\n\
                    future_key: [1, 2]\nchord_notation: 'english' # names\n";
        assert_eq!(
            set(text, "history_keep_all_days", Some("14")),
            "# Schnipsle settings\nhistory_keep_all_days: 14  # a week\n\
             future_key: [1, 2]\nchord_notation: 'english' # names\n"
        );
        assert_eq!(
            set(text, "chord_notation", Some("german")),
            "# Schnipsle settings\nhistory_keep_all_days: 7   # a week\n\
             future_key: [1, 2]\nchord_notation: german    # names\n"
        );
    }

    #[test]
    fn set_adds_a_missing_key_at_the_end() {
        assert_eq!(set("", "k", Some("1")), "k: 1\n");
        assert_eq!(set("a: x", "k", Some("1")), "a: x\nk: 1\n");
        assert_eq!(set("a: x\n", "k", Some("1")), "a: x\nk: 1\n");
        assert_eq!(set("\u{feff}", "k", Some("1")), "\u{feff}k: 1\n");
        // An indented key or a longer name is another key.
        assert_eq!(
            set("  k: 2\nkk: 3\n", "k", Some("1")),
            "  k: 2\nkk: 3\nk: 1\n"
        );
    }

    #[test]
    fn set_none_removes_the_key_and_its_indented_lines() {
        let text = "a: 1\nk:\n  - nested\n  - more\nb: 2\n";
        assert_eq!(set(text, "k", None), "a: 1\nb: 2\n");
        assert_eq!(set(text, "missing", None), text);
        assert_eq!(set("k: 1", "k", None), "");
    }

    #[test]
    fn set_keeps_the_rest_byte_for_byte() {
        let text = "\u{feff}a: 1\r\nk: 2 # c\r\n\r\n# end\r\n";
        assert_eq!(
            set(text, "k", Some("3")),
            "\u{feff}a: 1\r\nk: 3 # c\r\n\r\n# end\r\n"
        );
        // Only the first line of a repeated key changes; readers use that one.
        assert_eq!(set("k: 1\nk: 2\n", "k", Some("5")), "k: 5\nk: 2\n");
    }

    #[test]
    fn comments_after_any_kind_of_value() {
        assert_eq!(comment(" 7 # c"), Some("# c"));
        assert_eq!(comment(" 7\t# c"), Some("# c"));
        assert_eq!(comment(" a#b"), None);
        assert_eq!(comment(" # only"), Some("# only"));
        assert_eq!(comment(" \"a \\\" # b\" # c"), Some("# c"));
        assert_eq!(comment(" 'it''s # x' # c"), Some("# c"));
        assert_eq!(comment(" \"open # c"), None);
        assert_eq!(comment(" 'x' y"), None);
        assert_eq!(comment(""), None);
    }

    #[test]
    fn set_values_read_back() {
        let text = set("x: 1\n", "k", Some("german"));
        assert_eq!(value(&text, "k").as_deref(), Some("german"));
        assert_eq!(value(&set(&text, "k", None), "k"), None);
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
