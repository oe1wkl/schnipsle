// SPDX-License-Identifier: Apache-2.0
//! Turning an imported note into the text of its file (format.md §12.1):
//! clean-up, attachment links, the final tag line and the front matter.
//! Pure functions; the pipeline decides what to call them with.

use std::fmt::Write as _;

use schnipsle_core::{extract_tags, front_matter, tags::normalize};
use unicode_normalization::UnicodeNormalization;

use crate::note::Source;

/// Line endings to LF, NFC, no byte order mark: the only changes besides
/// links and tags (§12.1 rule 3).
pub(crate) fn clean(body: &str) -> String {
    let body = body.strip_prefix('\u{feff}').unwrap_or(body);
    body.replace("\r\n", "\n")
        .replace('\r', "\n")
        .nfc()
        .collect()
}

/// `body` with every Markdown link or image destination equal to `from`
/// changed to `to`: `](from)`, `](from "title")` and `](<from>)`.
pub(crate) fn rewrite_link(body: &str, from: &str, to: &str) -> String {
    let mut out = body.to_owned();
    for (old, new) in [
        (format!("]({from})"), format!("]({to})")),
        (format!("]({from} "), format!("]({to} ")),
        (format!("](<{from}>)"), format!("]({to})")),
    ] {
        out = out.replace(&old, &new);
    }
    out
}

/// One tag level made from a folder or tag name (§12.1 rule 4): NFC,
/// characters a tag cannot hold become `-`, runs of `-` collapse, no `-` at
/// either end. May be empty.
fn level(name: &str) -> String {
    let mut out = String::new();
    for c in name.nfc() {
        let c = if c.is_alphanumeric() || c == '_' {
            c
        } else {
            '-'
        };
        if c != '-' || !out.ends_with('-') {
            out.push(c);
        }
    }
    out.trim_matches('-').to_owned()
}

/// `true` if `#name` is a tag by format.md §5.3, as a whole.
fn recognized(name: &str) -> bool {
    let tags = extract_tags(&format!("#{name}"));
    tags.len() == 1 && tags[0].name == name
}

/// The tag for a source's folder path, without `#` (§12.1 rule 4):
/// `["Radio", "QRP Kits"]` → `Radio/QRP-Kits`; `None` if it gives no tag.
#[must_use]
pub fn folder_tag(folders: &[String]) -> Option<String> {
    let levels: Vec<String> = folders
        .iter()
        .map(|f| level(f))
        .filter(|l| !l.is_empty())
        .collect();
    let name = levels.join("/");
    recognized(&name).then_some(name)
}

/// A tag the source keeps outside the text, normalized like a folder level
/// (§12.1 rule 5); `None` if that is not a tag.
#[must_use]
pub fn tag_from(name: &str) -> Option<String> {
    let name = level(name);
    recognized(&name).then_some(name)
}

/// The tags for the final line: the folder tag, then the source's tags,
/// without those the text already carries and without repeats.
pub(crate) fn final_tags(body: &str, folders: &[String], tags: &[String]) -> Vec<String> {
    let mut seen: Vec<String> = extract_tags(body).into_iter().map(|t| t.key).collect();
    let mut out = Vec::new();
    for tag in folder_tag(folders)
        .into_iter()
        .chain(tags.iter().filter_map(|t| tag_from(t)))
    {
        let key = normalize(&tag);
        if !seen.contains(&key) {
            seen.push(key);
            out.push(tag);
        }
    }
    out
}

/// Tags at the start of a line that goes on with words that are no tags,
/// such as `#define KEYER_WPM 20` or `#include <x.h>`: probably code, which
/// §5.3 reads as tags all the same. Outside code blocks; each once.
pub(crate) fn code_like_tags(body: &str) -> Vec<String> {
    let is_tag = |word: &str| {
        let tags = extract_tags(word);
        tags.len() == 1 && word.strip_prefix('#') == Some(tags[0].name.as_str())
    };
    let mut out = Vec::new();
    let mut fence = false;
    for line in front_matter::body(body).lines().map(str::trim) {
        if line.starts_with("```") || line.starts_with("~~~") {
            fence = !fence;
            continue;
        }
        let mut words = line.split_whitespace();
        let Some(first) = words.next() else {
            continue;
        };
        if fence || !is_tag(first) || words.all(is_tag) {
            continue;
        }
        let tag = first.to_owned();
        if !out.contains(&tag) {
            out.push(tag);
        }
    }
    out
}

/// The report line for [`code_like_tags`] in `body`, if there are any.
pub(crate) fn code_like_tags_problem(body: &str) -> Option<String> {
    let tags = code_like_tags(body);
    (!tags.is_empty()).then(|| {
        format!(
            "{} at the start of a line become tags; change the text where they are not meant as \
             tags.",
            tags.join(", ")
        )
    })
}

/// `body` with `tags` on one final line after an empty line (§12.1 rule 5).
pub(crate) fn append_tags(body: &str, tags: &[String]) -> String {
    if tags.is_empty() {
        return body.to_owned();
    }
    let line = tags
        .iter()
        .map(|t| format!("#{t}"))
        .collect::<Vec<_>>()
        .join(" ");
    if body.trim().is_empty() {
        format!("{line}\n")
    } else if body.ends_with('\n') {
        format!("{body}\n{line}\n")
    } else {
        format!("{body}\n\n{line}\n")
    }
}

/// The front matter keys of an imported note (format.md §4, §12.1).
pub(crate) struct Keys<'a> {
    pub id: &'a str,
    pub created: &'a str,
    pub modified: &'a str,
    pub pinned: bool,
    pub source: Source,
    pub source_id: &'a str,
}

/// `body` with the front matter of `keys`. A block of another tool in
/// `body` is kept with its keys; `id` and `created` already in it win.
pub(crate) fn with_front_matter(body: &str, keys: &Keys<'_>) -> String {
    if front_matter::block(body).is_none() {
        let mut out = String::from("---\n");
        let _ = writeln!(out, "id: {}", keys.id);
        let _ = writeln!(out, "created: {}", keys.created);
        let _ = writeln!(out, "modified: {}", keys.modified);
        if keys.pinned {
            out.push_str("pinned: true\n");
        }
        let _ = writeln!(out, "source: {}", keys.source.id());
        let _ = writeln!(out, "source_id: {}", quote(keys.source_id));
        out.push_str("---\n");
        out.push_str(body);
        return out;
    }
    // `set` and `stamp` add keys at the top, so in reverse order: the
    // result starts with `id`, …, `modified`, `source`, `source_id`. An `id`
    // the text brings along is replaced: the note's id is the one in the
    // import log (§12.1 rule 1).
    let text = front_matter::set(body, "source_id", &quote(keys.source_id));
    let text = front_matter::set(&text, "source", keys.source.id());
    let text = front_matter::stamp(&text, keys.id, keys.created, keys.modified);
    let text = front_matter::set(&text, "id", keys.id);
    if keys.pinned {
        front_matter::with_pinned(&text, true)
    } else {
        text
    }
}

/// A YAML double-quoted scalar.
fn quote(s: &str) -> String {
    let mut out = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if c.is_control() => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// The folder in `notes/` for a note: the source's folder, then the
/// source's folders (§12.1 rule 2), each made safe as a file name.
pub(crate) fn placement(source: Source, folders: &[String]) -> String {
    let mut parts = vec![source.folder().to_owned()];
    parts.extend(folders.iter().filter_map(|f| path_part(f)));
    parts.join("/")
}

/// A folder name as a path component: NFC, no separators or control
/// characters, no leading `.` (hidden folders are ignored, format.md §1);
/// `None` if nothing is left.
fn path_part(name: &str) -> Option<String> {
    let name: String = name
        .nfc()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':') {
                '-'
            } else {
                c
            }
        })
        .filter(|c| !c.is_control())
        .collect();
    let name = name.trim().trim_start_matches('.').trim();
    (!name.is_empty() && name != "..").then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strings(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| (*s).to_owned()).collect()
    }

    #[test]
    fn folder_tags() {
        for (folders, tag) in [
            (&["Radio"][..], Some("Radio")),
            (&["Radio", "QRP Kits"], Some("Radio/QRP-Kits")),
            (&["Amateurfunk & Ham Radio"], Some("Amateurfunk-Ham-Radio")),
            (&["Brot und Germgebäck"], Some("Brot-und-Germgebäck")),
            (&["✱ Baking"], Some("Baking")),
            (&["Cantaremos (Chor)"], Some("Cantaremos-Chor")),
            (&["Reisen", "2024"], Some("Reisen/2024")),
            (&["Reisen", "✱", "Wien"], Some("Reisen/Wien")),
            (&["2024"], None),
            (&["a0b1c2"], None),
            (&["✱"], None),
            (&[], None),
        ] {
            assert_eq!(folder_tag(&strings(folders)).as_deref(), tag, "{folders:?}");
        }
        assert_eq!(tag_from("Ö Club").as_deref(), Some("Ö-Club"));
        assert_eq!(tag_from(""), None);
        assert_eq!(tag_from("a/b").as_deref(), Some("a-b"));
    }

    #[test]
    fn final_tag_line() {
        let tags = final_tags(
            "Repeaters #radio\n",
            &strings(&["Funk"]),
            &strings(&["radio", "Ö Club", "", "funk"]),
        );
        assert_eq!(tags, ["Funk", "Ö-Club"]);
        assert_eq!(append_tags("Text  \n", &tags), "Text  \n\n#Funk #Ö-Club\n");
        assert_eq!(append_tags("Text", &tags), "Text\n\n#Funk #Ö-Club\n");
        assert_eq!(append_tags("", &tags), "#Funk #Ö-Club\n");
        assert_eq!(append_tags("Text\n", &[]), "Text\n");
    }

    #[test]
    fn tags_that_look_like_code() {
        let body = "#recipe #baking\n#define WPM 20\n#include <Keyer.h>\n#define X\n\
                    Text with #tag inside\n```\n#ifdef Y\n```\n# Heading\n";
        assert_eq!(code_like_tags(body), ["#define", "#include"]);
    }

    #[test]
    fn clean_up_and_links() {
        assert_eq!(
            clean("\u{feff}Cafe\u{301}\r\nnext\rlast"),
            "Café\nnext\nlast"
        );
        let body = "![a](Attachments/x.png) [b](Attachments/x.png \"t\") ![c](<Attachments/x.png>) \
                    `](Attachments/x.png.bak)`";
        assert_eq!(
            rewrite_link(body, "Attachments/x.png", "../attachments/ab/ab.png"),
            "![a](../attachments/ab/ab.png) [b](../attachments/ab/ab.png \"t\") \
             ![c](../attachments/ab/ab.png) `](Attachments/x.png.bak)`"
        );
    }

    #[test]
    fn front_matter_of_imported_notes() {
        let keys = Keys {
            id: "01JB2C3E0F1G2H3J4K5M6N7P8Q",
            created: "2024-03-02T09:14:00Z",
            modified: "2025-11-20T18:40:12Z",
            pinned: true,
            source: Source::AppleNotes,
            source_id: "x-coredata://A/ICNote/p1",
        };
        assert_eq!(
            with_front_matter("Body\n", &keys),
            "---\nid: 01JB2C3E0F1G2H3J4K5M6N7P8Q\ncreated: 2024-03-02T09:14:00Z\n\
             modified: 2025-11-20T18:40:12Z\npinned: true\nsource: apple-notes\n\
             source_id: \"x-coredata://A/ICNote/p1\"\n---\nBody\n"
        );
        let other = "---\ntitle: Tuner\ncreated: 2001-01-01T00:00:00Z\n---\nBody\n";
        let text = with_front_matter(
            other,
            &Keys {
                pinned: false,
                ..keys
            },
        );
        assert_eq!(
            front_matter::get(&text, "created").as_deref(),
            Some("2001-01-01T00:00:00Z")
        );
        assert_eq!(front_matter::get(&text, "title").as_deref(), Some("Tuner"));
        assert_eq!(
            front_matter::get(&text, "source").as_deref(),
            Some("apple-notes")
        );
        assert_eq!(
            front_matter::get(&text, "source_id").as_deref(),
            Some("x-coredata://A/ICNote/p1")
        );
        assert!(text.ends_with("---\nBody\n"));
        // An id the text brings along gives way to the note's.
        let copied = "---\nid: 01AAAAAAAAAAAAAAAAAAAAAAAA\nsource: html\n---\nBody\n";
        let text = with_front_matter(copied, &keys);
        assert_eq!(front_matter::get(&text, "id").as_deref(), Some(keys.id));
        assert_eq!(text.matches("id: ").count(), 2); // id and source_id
        assert_eq!(
            front_matter::get(&text, "source").as_deref(),
            Some("apple-notes")
        );
    }

    #[test]
    fn placements() {
        assert_eq!(
            placement(
                Source::AppleNotes,
                &strings(&["Amateurfunk & Radio", "QRP Kits"])
            ),
            "Apple Notes/Amateurfunk & Radio/QRP Kits"
        );
        assert_eq!(
            placement(
                Source::Markdown,
                &strings(&[".hidden", "a/b: c", "  ", ".."])
            ),
            "Markdown/hidden/a-b- c"
        );
    }
}
