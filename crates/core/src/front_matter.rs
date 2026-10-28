// SPDX-License-Identifier: Apache-2.0
//! The front matter keys the app owns: `id`, `created`, `modified` and
//! `pinned` (format.md §4).
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

/// The raw front matter block of `text`, from the opening to the closing
/// `---` line inclusive (with a leading byte order mark), if it has one.
#[must_use]
pub fn block(text: &str) -> Option<&str> {
    let parts = split(text);
    parts.lines?;
    Some(&text[..text.len() - parts.body.len()])
}

/// `text` without its front matter block.
#[must_use]
pub fn body(text: &str) -> &str {
    split(text).body
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

/// Returns `text` with top-level `key` set to `value`: the line is replaced if
/// the key exists, else added at the top of the block (which is created if
/// needed). Nothing else changes.
#[must_use]
pub fn set(text: &str, key: &str, value: &str) -> String {
    let parts = split(text);
    let nl = parts.newline;
    let mut lines: Vec<String> = parts
        .lines
        .unwrap_or_default()
        .into_iter()
        .map(str::to_owned)
        .collect();
    if let Some(line) = lines.iter_mut().find(|l| value_of(l, key).is_some()) {
        *line = format!("{key}: {value}");
    } else {
        lines.insert(0, format!("{key}: {value}"));
    }
    join(parts.bom, &lines, nl, parts.body)
}

/// `true` if the front matter says `pinned: true` (format.md §4).
#[must_use]
pub fn is_pinned(text: &str) -> bool {
    get(text, "pinned").is_some_and(|v| v.eq_ignore_ascii_case("true"))
}

/// Returns `text` pinned or not (format.md §4): pinning sets `pinned: true`,
/// replacing an existing `pinned` line or adding one at the end of the block
/// (created if needed); unpinning removes the key, with any lines indented
/// below it. Nothing else changes; a note already in that state is returned
/// as is.
#[must_use]
pub fn with_pinned(text: &str, pinned: bool) -> String {
    let unchanged = if pinned {
        is_pinned(text)
    } else {
        get(text, "pinned").is_none()
    };
    if unchanged {
        return text.to_owned();
    }
    let parts = split(text);
    let nl = parts.newline;
    let mut lines: Vec<String> = Vec::new();
    let mut in_pinned = false;
    let mut placed = false;
    for line in parts.lines.unwrap_or_default() {
        if in_pinned && line.starts_with([' ', '\t']) {
            continue; // a value continued below the key
        }
        in_pinned = value_of(line, "pinned").is_some();
        if !in_pinned {
            lines.push(line.to_owned());
        } else if pinned && !placed {
            lines.push("pinned: true".to_owned());
            placed = true;
        }
    }
    if pinned && !placed {
        lines.push("pinned: true".to_owned());
    }
    join(parts.bom, &lines, nl, parts.body)
}

fn join(bom: &str, lines: &[String], nl: &str, body: &str) -> String {
    let mut out = String::with_capacity(body.len() + 100);
    out.push_str(bom);
    out.push_str("---");
    out.push_str(nl);
    for line in lines {
        out.push_str(line);
        out.push_str(nl);
    }
    out.push_str("---");
    out.push_str(nl);
    out.push_str(body);
    out
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
    join(parts.bom, &lines, nl, parts.body)
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
    fn set_replaces_or_adds() {
        assert_eq!(
            set("---\na: 1\nid: OLD # c\n---\nB", "id", ID),
            format!("---\na: 1\nid: {ID}\n---\nB")
        );
        assert_eq!(set("B", "id", ID), format!("---\nid: {ID}\n---\nB"));
    }

    #[test]
    fn block_and_body() {
        let note = "---\nid: X\n---\n# Body\n";
        assert_eq!(block(note), Some("---\nid: X\n---\n"));
        assert_eq!(body(note), "# Body\n");
        assert_eq!(block("# No block\n"), None);
        assert_eq!(body("# No block\n"), "# No block\n");
        assert_eq!(body("\u{feff}---\r\na: 1\r\n---\r\nB"), "B");
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

    #[test]
    fn pinning_touches_only_the_pinned_key() {
        let note = "---\nid: X\n# keep\nmodified: M\n---\nBody\n";
        let pinned = with_pinned(note, true);
        assert_eq!(
            pinned,
            "---\nid: X\n# keep\nmodified: M\npinned: true\n---\nBody\n"
        );
        assert!(is_pinned(&pinned));
        assert_eq!(with_pinned(&pinned, true), pinned);
        assert_eq!(with_pinned(&pinned, false), note);
        assert_eq!(with_pinned(note, false), note);

        // An existing line keeps its place; an indented value goes with it.
        let odd = "---\npinned: no\nid: X\n---\nB";
        assert_eq!(with_pinned(odd, true), "---\npinned: true\nid: X\n---\nB");
        let nested = "---\npinned:\n  true\nid: X\n---\nB";
        assert_eq!(with_pinned(nested, false), "---\nid: X\n---\nB");
        assert!(is_pinned("---\npinned: True\n---\n"));

        // No block: pinning adds one, unpinning leaves the note alone.
        assert_eq!(with_pinned("Body", true), "---\npinned: true\n---\nBody");
        assert_eq!(with_pinned("Body", false), "Body");
        // Line endings and byte order mark are kept.
        assert_eq!(
            with_pinned("\u{feff}---\r\nid: X\r\n---\r\nB", true),
            "\u{feff}---\r\nid: X\r\npinned: true\r\n---\r\nB"
        );
    }
}
