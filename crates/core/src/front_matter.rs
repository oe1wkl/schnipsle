// SPDX-License-Identifier: Apache-2.0
//! The front matter keys the app owns: `id`, `created`, `modified` (format.md §4).
//!
//! The block is read and edited line by line instead of being parsed and
//! re-serialized, so unknown keys, comments, quoting and order survive exactly
//! (format.md §4: unknown keys MUST be preserved unchanged).

/// The front matter of a note, split into its parts.
struct Split<'a> {
    /// Byte order mark, if any.
    bom: &'a str,
    /// Lines between the two `---` lines, without line endings; `None` if the
    /// note has no (terminated) front matter.
    lines: Option<Vec<&'a str>>,
    /// Everything after the closing `---` line (or the whole text).
    body: &'a str,
    /// `"\r\n"` if the note uses Windows line endings, else `"\n"`.
    newline: &'static str,
}

fn split(text: &str) -> Split<'_> {
    let (bom, rest) = text
        .strip_prefix('\u{feff}')
        .map_or(("", text), |r| ("\u{feff}", r));
    let newline = if rest
        .find('\n')
        .is_some_and(|i| i > 0 && rest.as_bytes()[i - 1] == b'\r')
    {
        "\r\n"
    } else {
        "\n"
    };
    let none = Split {
        bom,
        lines: None,
        body: rest,
        newline,
    };
    let Some(after_open) = rest
        .strip_prefix("---\n")
        .or_else(|| rest.strip_prefix("---\r\n"))
    else {
        return none;
    };
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in after_open.split_inclusive('\n') {
        offset += line.len();
        let content = line.trim_end_matches('\n').trim_end_matches('\r');
        if content.trim_end() == "---" {
            return Split {
                bom,
                lines: Some(lines),
                body: &after_open[offset..],
                newline,
            };
        }
        lines.push(content);
    }
    none // unterminated: not front matter
}

/// The value of top-level `key` in the front matter, without surrounding quotes.
#[must_use]
pub fn get(text: &str, key: &str) -> Option<String> {
    split(text)
        .lines?
        .iter()
        .find_map(|line| value_of(line, key))
        .map(str::to_owned)
}

fn value_of<'a>(line: &'a str, key: &str) -> Option<&'a str> {
    let value = line.strip_prefix(key)?.strip_prefix(':')?.trim();
    Some(
        value
            .strip_prefix('"')
            .and_then(|v| v.strip_suffix('"'))
            .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
            .unwrap_or(value),
    )
}

/// Returns `text` with `id` and `created` added if missing and `modified` set.
/// Existing `id` and `created` values are kept; nothing else changes.
#[must_use]
pub fn stamp(text: &str, id: &str, created: &str, modified: &str) -> String {
    let parts = split(text);
    let nl = parts.newline;
    let mut lines: Vec<String> = parts
        .lines
        .unwrap_or_default()
        .into_iter()
        .map(str::to_owned)
        .collect();
    let has = |lines: &[String], key: &str| lines.iter().any(|l| value_of(l, key).is_some());

    let mut missing = Vec::new();
    if !has(&lines, "id") {
        missing.push(format!("id: {id}"));
    }
    if !has(&lines, "created") {
        missing.push(format!("created: {created}"));
    }
    if let Some(line) = lines.iter_mut().find(|l| value_of(l, "modified").is_some()) {
        *line = format!("modified: {modified}");
    } else {
        missing.push(format!("modified: {modified}"));
    }
    lines.splice(0..0, missing);

    let mut out = String::with_capacity(text.len() + 100);
    out.push_str(parts.bom);
    out.push_str("---");
    out.push_str(nl);
    for line in &lines {
        out.push_str(line);
        out.push_str(nl);
    }
    out.push_str("---");
    out.push_str(nl);
    out.push_str(parts.body);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    const T1: &str = "2026-10-15T21:29:37Z";
    const T2: &str = "2026-10-16T08:00:00Z";

    #[test]
    fn adds_a_block_to_a_plain_note() {
        assert_eq!(
            stamp("# Title\n\nText\n", ID, T1, T2),
            format!("---\nid: {ID}\ncreated: {T1}\nmodified: {T2}\n---\n# Title\n\nText\n")
        );
        assert_eq!(
            stamp("", ID, T1, T1),
            format!("---\nid: {ID}\ncreated: {T1}\nmodified: {T1}\n---\n")
        );
    }

    #[test]
    fn keeps_everything_it_does_not_own() {
        let note = "---\n# comment\ntitle: \"Hi: there\"\nid: 'KEEP'\nextra:\n  - a\n  - b\nmodified: old # note\n---\nBody\n";
        let stamped = stamp(note, ID, T1, T2);
        assert_eq!(
            stamped,
            format!(
                "---\ncreated: {T1}\n# comment\ntitle: \"Hi: there\"\nid: 'KEEP'\nextra:\n  - a\n  - b\nmodified: {T2}\n---\nBody\n"
            )
        );
        assert_eq!(get(&stamped, "id").as_deref(), Some("KEEP"));
        assert_eq!(get(&stamped, "title").as_deref(), Some("Hi: there"));
        // Indented lines are not top-level keys.
        assert_eq!(get("---\nx:\n  id: nested\n---\n", "id"), None);
    }

    #[test]
    fn keeps_line_endings_and_bom() {
        let note = "\u{feff}---\r\nid: X\r\n---\r\nBody\r\n";
        assert_eq!(
            stamp(note, ID, T1, T2),
            format!("\u{feff}---\r\ncreated: {T1}\r\nmodified: {T2}\r\nid: X\r\n---\r\nBody\r\n")
        );
    }

    #[test]
    fn unterminated_block_is_body() {
        let note = "---\nnot closed\n";
        assert_eq!(get(note, "not"), None);
        assert!(stamp(note, ID, T1, T1).ends_with("---\n---\nnot closed\n"));
    }

    #[test]
    fn idempotent_except_modified() {
        let once = stamp("Text", ID, T1, T1);
        let twice = stamp(&once, "01BX5ZZKBKACTAV9WEVGEMMVRZ", T2, T2);
        assert_eq!(
            twice,
            once.replace(&format!("modified: {T1}"), &format!("modified: {T2}"))
        );
    }
}
