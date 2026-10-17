// SPDX-License-Identifier: Apache-2.0
//! Relative links in notes and keeping them working when a note moves
//! (format.md §5.5, ADR-0016).
//!
//! Links to attachments and other files are relative to the note file. When a
//! note lands in another folder (restored from the trash into `notes/`, moved
//! in from the vault root), [`Vault::relink_after_move`] rewrites the links
//! that resolved before so they point to the same files again. The previous
//! text goes into the note's history first. Only inline links and images are
//! rewritten (`[text](dest)`, `![alt](dest)`); URLs, absolute paths, note links
//! (`[[Title]]`), links in code and links that did not resolve stay as written,
//! and so do reference definitions (`[ref]: dest`), which the app never writes.

use std::fmt::Write as _;
use std::fs;
use std::ops::Range;

use comrak::nodes::NodeValue;
use comrak::{Arena, Options, parse_document};

use crate::attachments::percent_decode;
use crate::front_matter;
use crate::stamp::{is_ulid, new_ulid};
use crate::vault::{Access, Result, Vault, placeholder_of, sha256_hex};

/// A link destination as written in a note.
#[derive(Debug, PartialEq, Eq)]
struct Destination {
    /// Byte range in the note text, without `<` and `>`.
    range: Range<usize>,
    /// Written as `<dest>`: spaces allowed, no percent-encoding.
    angle: bool,
}

/// The destinations of the inline links and images in `text`, in order.
fn destinations(text: &str) -> Vec<Destination> {
    let mut options = Options::default();
    options.extension.table = true;
    options.extension.tasklist = true;
    options.extension.strikethrough = true;
    options.extension.autolink = true;
    options.extension.footnotes = true;
    options.extension.math_dollars = true;
    options.extension.wikilinks_title_after_pipe = true;
    options.extension.front_matter_delimiter = Some("---".to_owned());
    let arena = Arena::new();
    let root = parse_document(&arena, text, &options);

    let mut line_starts = vec![0];
    line_starts.extend(text.match_indices('\n').map(|(i, _)| i + 1));
    let offset = |line: usize, column: usize| -> Option<usize> {
        line_starts
            .get(line.checked_sub(1)?)?
            .checked_add(column.checked_sub(1)?)
    };

    let mut found = Vec::new();
    for node in root.descendants() {
        let data = node.data.borrow();
        let (NodeValue::Link(link) | NodeValue::Image(link)) = &data.value else {
            continue;
        };
        let pos = data.sourcepos;
        let (Some(start), Some(end)) = (
            offset(pos.start.line, pos.start.column),
            offset(pos.end.line, pos.end.column),
        ) else {
            continue;
        };
        // Positions are checked against the text: a link whose destination
        // cannot be found exactly is left alone.
        if let Some(slice) = text.get(start..=end)
            && let Some(dest) = destination_in(slice, &link.url)
        {
            found.push(Destination {
                range: start + dest.range.start..start + dest.range.end,
                angle: dest.angle,
            });
        }
    }
    found
}

/// Finds the destination `url` in the source of an inline link, `[…](dest …)`.
fn destination_in(slice: &str, url: &str) -> Option<Destination> {
    if !slice.ends_with(')') {
        return None; // a reference link or an autolink
    }
    for (bracket, _) in slice.match_indices("](") {
        let after = bracket + 2;
        let rest = &slice[after..];
        let skipped = rest.len() - rest.trim_start().len();
        let start = after + skipped;
        let candidate = &slice[start..];
        let dest = if let Some(inner) = candidate.strip_prefix('<') {
            let len = inner.find(['>', '\n'])?;
            Destination {
                range: start + 1..start + 1 + len,
                angle: true,
            }
        } else {
            let mut depth = 0_u32;
            let mut len = candidate.len();
            for (i, c) in candidate.char_indices() {
                match c {
                    '(' => depth += 1,
                    ')' if depth == 0 => {
                        len = i;
                        break;
                    }
                    ')' => depth -= 1,
                    c if c.is_whitespace() || c.is_control() => {
                        len = i;
                        break;
                    }
                    _ => {}
                }
            }
            Destination {
                range: start..start + len,
                angle: false,
            }
        };
        if unescape(&slice[dest.range.clone()]) == url {
            return Some(dest);
        }
    }
    None
}

/// A destination with its backslash escapes removed, as comrak reports it.
fn unescape(dest: &str) -> String {
    let mut out = String::with_capacity(dest.len());
    let mut chars = dest.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, chars.peek()) {
            ('\\', Some(next)) if next.is_ascii_punctuation() => {
                out.push(*next);
                chars.next();
            }
            _ => out.push(c),
        }
    }
    out
}

/// Folders and file of a vault path, below the vault root.
type VaultPath = Vec<String>;

/// Resolves the relative link path `link` (decoded, without `?` or `#`) from
/// the folder `dir`; `None` if it is not relative, leaves the vault or
/// reaches a hidden file.
fn resolve_from(dir: &[String], link: &str) -> Option<VaultPath> {
    if link.is_empty() || link.starts_with('/') || link.contains('\\') || link.contains(':') {
        return None;
    }
    let mut parts = dir.to_vec();
    for part in link.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            hidden if hidden.starts_with('.') => return None,
            name => parts.push(name.to_owned()),
        }
    }
    (!parts.is_empty()).then_some(parts)
}

/// The relative path from the folder `dir` to `target`.
fn relative(dir: &[String], target: &[String]) -> String {
    let common = dir.iter().zip(target).take_while(|(a, b)| a == b).count();
    let mut parts: Vec<&str> = vec![".."; dir.len() - common];
    parts.extend(target[common..].iter().map(String::as_str));
    parts.join("/")
}

/// Percent-encodes what a plain link destination cannot hold.
fn encode(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for c in path.chars() {
        if c.is_control()
            || c.is_whitespace()
            || matches!(
                c,
                '%' | '(' | ')' | '<' | '>' | '\\' | '"' | '#' | '?' | '[' | ']' | '`'
            )
        {
            let mut buf = [0; 4];
            for b in c.encode_utf8(&mut buf).bytes() {
                let _ = write!(out, "%{b:02X}");
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Rewrites the relative links of a note that moved from the folder `old_dir`
/// to `new_dir` (both below the vault root). `map` tells where a file of the
/// old tree is now (unchanged unless it moved along), `exists` whether a path
/// of the new tree exists. Returns `None` if no link changes.
fn rewrite(
    text: &str,
    old_dir: &[String],
    new_dir: &[String],
    map: &dyn Fn(&[String]) -> VaultPath,
    exists: &dyn Fn(&[String]) -> bool,
) -> Option<String> {
    let mut out = String::with_capacity(text.len());
    let mut copied = 0;
    let mut changed = false;
    for dest in destinations(text) {
        let written = &text[dest.range.clone()];
        let split = written.find(['?', '#']).unwrap_or(written.len());
        let (path, suffix) = written.split_at(split);
        let path = unescape(path);
        let decoded = if dest.angle {
            Some(path.clone())
        } else {
            percent_decode(&path)
        };
        let Some(target) = decoded.as_deref().and_then(|p| resolve_from(old_dir, p)) else {
            continue;
        };
        let target = map(&target);
        if !exists(&target) {
            continue; // did not resolve before: leave it as the user wrote it
        }
        let now = relative(new_dir, &target);
        if resolve_from(new_dir, decoded.as_deref().unwrap_or_default()).as_deref()
            == Some(target.as_slice())
        {
            continue; // still right
        }
        let replacement = if dest.angle { now } else { encode(&now) };
        out.push_str(&text[copied..dest.range.start]);
        out.push_str(&replacement);
        out.push_str(suffix);
        copied = dest.range.end;
        changed = true;
    }
    out.push_str(&text[copied..]);
    changed.then_some(out)
}

impl Vault {
    /// After the note now at `path` (relative to `notes/`) moved here from
    /// the folder `old_dir` (below the vault root), rewrites its relative links
    /// that resolved there so they resolve to the same files (format.md §5.5).
    /// `map` tells where a file of the old tree is now, for files that moved
    /// along with the note. The previous text is stored as a snapshot first;
    /// a note without `id` gets one for it. Returns whether the note changed.
    ///
    /// # Errors
    /// [`crate::VaultError::Conflict`] if the note changed meanwhile; I/O
    /// errors. The note stays as it was then.
    pub(crate) fn relink_after_move(
        &self,
        path: &str,
        old_dir: &[String],
        map: &dyn Fn(&[String]) -> VaultPath,
    ) -> Result<bool> {
        let full = self.resolve(path)?;
        let bytes = self.coordinated(Access::Read, &full, |file| Ok(fs::read(file)?))?;
        let text = String::from_utf8_lossy(&bytes);
        let mut new_dir = vec![crate::layout::NOTES_DIR.to_owned()];
        new_dir.extend(path.split('/').map(str::to_owned));
        new_dir.pop();
        let root = self.root();
        let exists = |parts: &[String]| {
            let file = parts.iter().fold(root.to_owned(), |p, part| p.join(part));
            file.exists() || placeholder_of(&file).exists()
        };
        let Some(rewritten) = rewrite(&text, old_dir, &new_dir, map, &exists) else {
            return Ok(false);
        };
        // The snapshot is filed under the note's id, so a note without one gets it now.
        let (id, rewritten) =
            if let Some(id) = front_matter::get(&text, "id").filter(|id| is_ulid(id)) {
                (id, rewritten)
            } else {
                let id = new_ulid()?;
                let with_id = front_matter::set(&rewritten, "id", &id);
                (id, with_id)
            };
        self.snapshot(&id, &bytes)?;
        self.write_note(path, &rewritten, &sha256_hex(&bytes))?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir(path: &str) -> VaultPath {
        path.split('/')
            .filter(|p| !p.is_empty())
            .map(str::to_owned)
            .collect()
    }

    fn found(text: &str) -> Vec<&str> {
        destinations(text)
            .iter()
            .map(|d| &text[d.range.clone()])
            .collect()
    }

    #[test]
    fn finds_inline_destinations_only() {
        let text = "---\nimage: x.png\n---\n# T\n\n![a](../attachments/ab/x.png \"Title\") and \
                    [b](<Bild 1.png>) and [c](a\\(1\\).png)\n\n> - ![q](q.png)\n\n\
                    `[code](no.png)`\n\n```\n[fenced](no.png)\n```\n\n[[Note link]] [ref][r] \
                    <https://x.org/a.png> [w](https://x.org/b.png)\n\n[r]: ref.png\n\n\
                    | t |\n|---|\n| ![cell](c.png) |\n\nÄußerst ![ü](ü.png)\n";
        assert_eq!(
            found(text),
            [
                "../attachments/ab/x.png",
                "Bild 1.png",
                "a\\(1\\).png",
                "q.png",
                "https://x.org/b.png",
                "c.png",
                "ü.png"
            ]
        );
        assert!(destinations(text)[1].angle);
    }

    #[test]
    fn relative_paths() {
        assert_eq!(
            relative(&dir("notes"), &dir("attachments/ab/x.png")),
            "../attachments/ab/x.png"
        );
        assert_eq!(relative(&dir("notes/P"), &dir("notes/P/b.png")), "b.png");
        assert_eq!(
            relative(&dir("notes/P/q"), &dir("notes/R/b.png")),
            "../../R/b.png"
        );
        assert_eq!(relative(&dir(""), &dir("attachments/x")), "attachments/x");
        assert_eq!(encode("a b(1)%.png"), "a%20b%281%29%25.png");
        assert_eq!(encode("Größe.png"), "Größe.png");
    }

    #[test]
    fn rewrites_only_links_that_resolved() {
        let text = "![a](../../attachments/ab/x.png#frag) ![b](../../attachments/ab/gone.png)\n\
                    [c](https://x.org) [d](/abs.png) [e](<../../Bilder/a b.png>) \
                    [f](../../Bilder/a%20b.png?x) [g](../Projekte/b.png) [h](#top)\n";
        let existing = [
            dir("attachments/ab/x.png"),
            dir("Bilder/a b.png"),
            dir("notes/Projekte/b.png"),
        ];
        let exists = |p: &[String]| existing.iter().any(|e| e == p);
        let identity = |p: &[String]| p.to_vec();
        let moved = rewrite(
            text,
            &dir("notes/Projekte"),
            &dir("notes"),
            &identity,
            &exists,
        );
        assert_eq!(
            moved.as_deref(),
            Some(
                "![a](../attachments/ab/x.png#frag) ![b](../../attachments/ab/gone.png)\n\
                 [c](https://x.org) [d](/abs.png) [e](<../Bilder/a b.png>) \
                 [f](../Bilder/a%20b.png?x) [g](Projekte/b.png) [h](#top)\n"
            )
        );
        // Nothing to do where the note was.
        assert_eq!(
            rewrite(
                text,
                &dir("notes/Projekte"),
                &dir("notes/Projekte"),
                &identity,
                &exists
            ),
            None
        );
    }

    #[test]
    fn files_that_moved_along_keep_their_links() {
        // The folder `P` moved from the vault root into notes/.
        let text = "![in](bild.png) ![out](../attachments/x.png)";
        let map = |p: &[String]| {
            if p.first().is_some_and(|f| f == "P") {
                std::iter::once("notes".to_owned())
                    .chain(p.iter().cloned())
                    .collect()
            } else {
                p.to_vec()
            }
        };
        let existing = [dir("notes/P/bild.png"), dir("attachments/x.png")];
        let exists = |p: &[String]| existing.iter().any(|e| e == p);
        assert_eq!(
            rewrite(text, &dir("P"), &dir("notes/P"), &map, &exists).as_deref(),
            Some("![in](bild.png) ![out](../../attachments/x.png)")
        );
    }

    /// format.md §5.5: the golden files in `tests/fixtures/format/5.5-move/`.
    #[test]
    fn golden_move() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/5.5-move");
        let before = fs::read_to_string(fixtures.join("before.md")).unwrap();
        let after = fs::read_to_string(fixtures.join("after.md")).unwrap();
        let existing = [
            "attachments/3f/3f9a.png",
            "notes/Projekte/foto.png",
            "notes/Projekte/Foto 2.png",
            "notes/Projekte/Daten blatt.pdf",
            "notes/Musik/kanon.abc",
        ]
        .map(dir);
        let exists = |p: &[String]| existing.iter().any(|e| e == p);
        let moved = rewrite(
            &before,
            &dir("notes/Projekte"),
            &dir("notes"),
            &|p| p.to_vec(),
            &exists,
        );
        assert_eq!(moved.as_deref(), Some(after.as_str()));
    }
}
