// SPDX-License-Identifier: Apache-2.0
//! Comparing two versions of a note (ADR-0038).
//!
//! [`compare`] takes the complete text of two versions, the older first, and
//! returns a [`Comparison`] the UI only has to draw:
//!
//! - the front matter compared key by key, as a list of [`KeyChange`]s;
//! - the body compared line by line (patience diff), in one column: removed
//!   lines, then the lines added in their place, each split into [`Part`]s
//!   with the words that changed marked when the two lines are alike;
//! - unchanged lines more than [`CONTEXT`] away from a change folded into one
//!   [`Row::Folded`], which carries them so they can be shown on request.
//!
//! The diff gives up refining after [`TIME_LIMIT`]; the result is then
//! coarser (larger blocks) but still correct. Line endings are not compared.

use std::ops::Range;
use std::time::{Duration, Instant};

use serde::Serialize;
use similar::{Algorithm, ChangeTag, DiffTag, TextDiff};

use crate::front_matter;

/// Unchanged lines shown before and after a change; more are folded.
pub const CONTEXT: usize = 3;

/// How long the diff may take before it settles for a coarser result.
pub const TIME_LIMIT: Duration = Duration::from_millis(500);

/// How alike a removed and an added line must be (0 to 1, by words) for the
/// changed words to be marked; below it the whole line counts as changed.
const MIN_RATIO: f32 = 0.5;

/// What changed between two versions of a note. Serialized with camelCase
/// names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Comparison {
    /// Front matter keys whose value differs, in the order of the newer
    /// version, then the keys only the older one has.
    pub front_matter: Vec<KeyChange>,
    /// Lines of the body only the newer version has.
    pub added: usize,
    /// Lines of the body only the older version has.
    pub removed: usize,
    /// The body, in reading order.
    pub rows: Vec<Row>,
}

impl Comparison {
    /// `true` if the bodies are the same line for line.
    #[must_use]
    pub fn same_body(&self) -> bool {
        self.added == 0 && self.removed == 0
    }
}

/// A front matter key whose value differs between the versions.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct KeyChange {
    /// The key as written.
    pub key: String,
    /// Its value in the older version (lines below the key included), or
    /// `None` if only the newer one has it.
    pub before: Option<String>,
    /// Its value in the newer version, or `None` if it was removed.
    pub after: Option<String>,
}

/// A row of the compared body. Line numbers count from 1 at the top of the
/// file, front matter included, as in the editor's source view. Serialized
/// with a `kind` tag in camelCase.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum Row {
    /// A line both versions have.
    Same {
        /// Its number in the older version.
        before: usize,
        /// Its number in the newer version.
        after: usize,
        /// The line, without its line ending.
        text: String,
    },
    /// A line only the older version has.
    Removed {
        /// Its number in the older version.
        before: usize,
        /// The line in pieces; `changed` marks the words the newer version
        /// changed. Empty for an empty line.
        parts: Vec<Part>,
    },
    /// A line only the newer version has.
    Added {
        /// Its number in the newer version.
        after: usize,
        /// As for [`Row::Removed`].
        parts: Vec<Part>,
    },
    /// Unchanged lines left out of the view.
    Folded {
        /// The number of the first of them in the older version.
        before: usize,
        /// The number of the first of them in the newer version.
        after: usize,
        /// The lines, without line endings.
        lines: Vec<String>,
    },
}

/// A piece of a removed or added line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Part {
    /// The text.
    pub text: String,
    /// `true` for words the other version does not have.
    pub changed: bool,
}

/// Compares `before` (the older version) with `after`, both complete note
/// files.
#[must_use]
pub fn compare(before: &str, after: &str) -> Comparison {
    let deadline = Instant::now() + TIME_LIMIT;
    let old: Vec<&str> = front_matter::body(before).lines().collect();
    let new: Vec<&str> = front_matter::body(after).lines().collect();
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .deadline(deadline)
        .diff_slices(&old, &new);
    let blocks = blocks(diff.ops().iter().map(similar::DiffOp::as_tag_tuple));
    let mut body = Body {
        rows: Vec::new(),
        old_base: body_start(before),
        new_base: body_start(after),
        deadline,
    };
    let (mut added, mut removed) = (0, 0);
    for (i, block) in blocks.iter().enumerate() {
        match block {
            Block::Same(old_range, new_start) => body.same(
                &old[old_range.clone()],
                old_range.start,
                *new_start,
                i > 0,
                i + 1 < blocks.len(),
            ),
            Block::Changed(old_range, new_range) => {
                removed += old_range.len();
                added += new_range.len();
                body.changed(
                    &old[old_range.clone()],
                    &new[new_range.clone()],
                    old_range.start,
                    new_range.start,
                );
            }
        }
    }
    Comparison {
        front_matter: key_changes(&entries(before), &entries(after)),
        added,
        removed,
        rows: body.rows,
    }
}

/// A run of the body: unchanged lines (old range, start in the new version),
/// or lines replaced (old range, new range; either may be empty).
enum Block {
    Same(Range<usize>, usize),
    Changed(Range<usize>, Range<usize>),
}

/// The diff's operations, with neighbouring deletes and inserts joined into
/// one changed block.
fn blocks(ops: impl Iterator<Item = (DiffTag, Range<usize>, Range<usize>)>) -> Vec<Block> {
    let mut blocks: Vec<Block> = Vec::new();
    for (tag, old, new) in ops {
        if tag == DiffTag::Equal {
            blocks.push(Block::Same(old, new.start));
        } else if let Some(Block::Changed(last_old, last_new)) = blocks.last_mut() {
            last_old.end = old.end;
            last_new.end = new.end;
        } else {
            blocks.push(Block::Changed(old, new));
        }
    }
    blocks
}

/// The body's rows as they are built.
struct Body {
    rows: Vec<Row>,
    /// Lines above the body (front matter) in each version.
    old_base: usize,
    new_base: usize,
    deadline: Instant,
}

impl Body {
    /// Unchanged `lines`, starting at `old_at` and `new_at` (0-based in the
    /// bodies), shown next to the changes before and after them, else folded.
    fn same(&mut self, lines: &[&str], old_at: usize, new_at: usize, prev: bool, next: bool) {
        let lead = if prev { CONTEXT } else { 0 };
        let trail = if next { CONTEXT } else { 0 };
        let line = |k: usize| {
            (
                self.old_base + old_at + k + 1,
                self.new_base + new_at + k + 1,
            )
        };
        let shown = |rows: &mut Vec<Row>, range: Range<usize>| {
            for k in range {
                let (before, after) = line(k);
                rows.push(Row::Same {
                    before,
                    after,
                    text: lines[k].to_owned(),
                });
            }
        };
        // Folding a single line would hide nothing.
        if lines.len() <= lead + trail + 1 {
            shown(&mut self.rows, 0..lines.len());
            return;
        }
        shown(&mut self.rows, 0..lead);
        let (before, after) = line(lead);
        self.rows.push(Row::Folded {
            before,
            after,
            lines: lines[lead..lines.len() - trail]
                .iter()
                .map(|l| (*l).to_owned())
                .collect(),
        });
        shown(&mut self.rows, lines.len() - trail..lines.len());
    }

    /// `old` lines replaced by `new` ones: the removed lines, then the added.
    /// The lines are paired in order; a pair that is alike gets its changed
    /// words marked.
    fn changed(&mut self, old: &[&str], new: &[&str], old_at: usize, new_at: usize) {
        let mut added = Vec::with_capacity(new.len());
        for (k, line) in old.iter().enumerate() {
            let (old_parts, new_parts) = match new.get(k) {
                Some(other) => words(line, other, self.deadline),
                None => (whole(line), Vec::new()),
            };
            self.rows.push(Row::Removed {
                before: self.old_base + old_at + k + 1,
                parts: old_parts,
            });
            if k < new.len() {
                added.push(new_parts);
            }
        }
        for (k, line) in new.iter().enumerate() {
            let parts = if k < added.len() {
                std::mem::take(&mut added[k])
            } else {
                whole(line)
            };
            self.rows.push(Row::Added {
                after: self.new_base + new_at + k + 1,
                parts,
            });
        }
    }
}

/// The pieces of `old` and `new` with the words that differ marked, or both
/// whole if they have too little in common.
fn words(old: &str, new: &str, deadline: Instant) -> (Vec<Part>, Vec<Part>) {
    let diff = TextDiff::configure()
        .algorithm(Algorithm::Patience)
        .deadline(deadline)
        .diff_unicode_words(old, new);
    if diff.ratio() < MIN_RATIO {
        return (whole(old), whole(new));
    }
    let (mut old_parts, mut new_parts) = (Vec::new(), Vec::new());
    for change in diff.iter_all_changes() {
        let text = change.value();
        match change.tag() {
            ChangeTag::Equal => {
                push(&mut old_parts, text, false);
                push(&mut new_parts, text, false);
            }
            ChangeTag::Delete => push(&mut old_parts, text, true),
            ChangeTag::Insert => push(&mut new_parts, text, true),
        }
    }
    (old_parts, new_parts)
}

/// Appends `text` to `parts`, joined to the last piece if marked the same.
fn push(parts: &mut Vec<Part>, text: &str, changed: bool) {
    match parts.last_mut() {
        Some(last) if last.changed == changed => last.text.push_str(text),
        _ if text.is_empty() => {}
        _ => parts.push(Part {
            text: text.to_owned(),
            changed,
        }),
    }
}

/// `line` as one unmarked piece (none for an empty line).
fn whole(line: &str) -> Vec<Part> {
    let mut parts = Vec::new();
    push(&mut parts, line, false);
    parts
}

/// Lines above the body: the front matter with its two `---` lines.
fn body_start(text: &str) -> usize {
    front_matter::lines(text).map_or(0, |lines| lines.len() + 2)
}

/// The front matter's top-level keys with their values; lines indented below
/// a key (lists, nested maps, comments) belong to its value.
fn entries(text: &str) -> Vec<(String, String)> {
    let mut entries: Vec<(String, Vec<&str>)> = Vec::new();
    for line in front_matter::lines(text).unwrap_or_default() {
        let starts_key = !line.starts_with([' ', '\t', '-', '#']) && line.contains(':');
        if starts_key && let Some((key, value)) = line.split_once(':') {
            let value = value.trim();
            let lines = if value.is_empty() {
                vec![]
            } else {
                vec![value]
            };
            entries.push((key.trim_end().to_owned(), lines));
        } else if let Some((_, lines)) = entries.last_mut() {
            lines.push(line);
        } else {
            // Above the first key, e.g. a comment: its own entry.
            entries.push((line.trim().to_owned(), Vec::new()));
        }
    }
    entries
        .into_iter()
        .map(|(key, lines)| (key, lines.join("\n").trim_end().to_owned()))
        .collect()
}

/// The keys whose values differ, matched by name (a repeated key with its
/// repeat).
fn key_changes(old: &[(String, String)], new: &[(String, String)]) -> Vec<KeyChange> {
    let mut matched = vec![false; old.len()];
    let mut changes = Vec::new();
    for (key, value) in new {
        let found = old
            .iter()
            .enumerate()
            .find(|(i, (old_key, _))| !matched[*i] && old_key == key);
        match found {
            Some((i, (_, old_value))) => {
                matched[i] = true;
                if old_value != value {
                    changes.push(KeyChange {
                        key: key.clone(),
                        before: Some(old_value.clone()),
                        after: Some(value.clone()),
                    });
                }
            }
            None => changes.push(KeyChange {
                key: key.clone(),
                before: None,
                after: Some(value.clone()),
            }),
        }
    }
    for ((key, value), _) in old.iter().zip(matched).filter(|(_, m)| !m) {
        changes.push(KeyChange {
            key: key.clone(),
            before: Some(value.clone()),
            after: None,
        });
    }
    changes
}

#[cfg(test)]
mod tests {
    use super::*;

    fn same(before: usize, after: usize, text: &str) -> Row {
        Row::Same {
            before,
            after,
            text: text.into(),
        }
    }

    fn part(text: &str, changed: bool) -> Part {
        Part {
            text: text.into(),
            changed,
        }
    }

    fn removed(before: usize, parts: Vec<Part>) -> Row {
        Row::Removed { before, parts }
    }

    fn added(after: usize, parts: Vec<Part>) -> Row {
        Row::Added { after, parts }
    }

    #[test]
    fn marks_the_changed_words_of_a_line() {
        let c = compare(
            "# Soup\n\nTwo onions, one leek.\n",
            "# Soup\n\nThree onions, one leek.\n",
        );
        assert!(c.front_matter.is_empty());
        assert_eq!((c.added, c.removed), (1, 1));
        assert_eq!(
            c.rows,
            vec![
                same(1, 1, "# Soup"),
                same(2, 2, ""),
                removed(
                    3,
                    vec![part("Two", true), part(" onions, one leek.", false)]
                ),
                added(
                    3,
                    vec![part("Three", true), part(" onions, one leek.", false)]
                ),
            ]
        );
    }

    #[test]
    fn lines_with_little_in_common_are_changed_whole() {
        let c = compare("a\nThe tuner is done.\nb\n", "a\nPick up bread rolls!\nb\n");
        assert_eq!(
            c.rows,
            vec![
                same(1, 1, "a"),
                removed(2, vec![part("The tuner is done.", false)]),
                added(2, vec![part("Pick up bread rolls!", false)]),
                same(3, 3, "b"),
            ]
        );
    }

    #[test]
    fn removed_lines_come_before_the_added_ones() {
        let c = compare("a\nold one\nold two\nz\n", "a\nnew one\nz\n");
        assert_eq!((c.added, c.removed), (1, 2));
        let kinds: Vec<_> = c
            .rows
            .iter()
            .map(|r| match r {
                Row::Same { .. } => '=',
                Row::Removed { .. } => '-',
                Row::Added { .. } => '+',
                Row::Folded { .. } => '~',
            })
            .collect();
        assert_eq!(kinds, ['=', '-', '-', '+', '=']);
        assert_eq!(c.rows[2], removed(3, vec![part("old two", false)]));
        assert_eq!(
            c.rows[3],
            added(2, vec![part("new", true), part(" one", false)])
        );
    }

    #[test]
    fn folds_unchanged_lines_away_from_changes() {
        let lines: Vec<String> = (1..=20).map(|n| format!("line {n}")).collect();
        let before = lines.join("\n");
        let mut changed = lines.clone();
        changed[9] = "line ten".into();
        let after = changed.join("\n");
        let c = compare(&before, &after);
        let Row::Folded {
            before: 1,
            after: 1,
            lines: head,
        } = &c.rows[0]
        else {
            panic!("{:?}", c.rows[0]);
        };
        assert_eq!(head.len(), 6); // lines 1–6, then 7–9 as context
        assert_eq!(c.rows[1], same(7, 7, "line 7"));
        assert_eq!(c.rows[3], same(9, 9, "line 9"));
        assert!(matches!(c.rows[4], Row::Removed { before: 10, .. }));
        assert!(matches!(c.rows[5], Row::Added { after: 10, .. }));
        assert_eq!(c.rows[8], same(13, 13, "line 13"));
        let Row::Folded {
            before: 14,
            lines: tail,
            ..
        } = &c.rows[9]
        else {
            panic!("{:?}", c.rows[9]);
        };
        assert_eq!(tail.len(), 7);
        assert_eq!(c.rows.len(), 10);
    }

    #[test]
    fn a_single_line_between_contexts_is_not_folded() {
        let before = "1\n2\n3\n4\n5\n6\n7\n8\n9\n";
        let after = "1\nzwei\n3\n4\n5\n6\n7\n8\nneun\n";
        let c = compare(before, after);
        assert!(!c.rows.iter().any(|r| matches!(r, Row::Folded { .. })));
        let after = "zwei\n2\n3\n4\n5\n6\n7\n8\n9\n10\nzehn\n";
        let c = compare(before, after);
        assert!(c.rows.iter().any(|r| matches!(r, Row::Folded { .. })));
    }

    #[test]
    fn identical_versions_fold_everything() {
        let text = "---\nid: x\n---\n# Same\n\nText\n";
        let c = compare(text, text);
        assert!(c.same_body() && c.front_matter.is_empty());
        assert_eq!(
            c.rows,
            vec![Row::Folded {
                before: 4,
                after: 4,
                lines: vec!["# Same".into(), String::new(), "Text".into()]
            }]
        );
        let empty = compare("", "");
        assert!(empty.rows.is_empty() && empty.same_body());
    }

    #[test]
    fn an_empty_side_adds_or_removes_every_line() {
        let c = compare("", "# New\n\nText\n");
        assert_eq!((c.added, c.removed), (3, 0));
        assert_eq!(c.rows[1], added(2, vec![]));
        let c = compare("# Old\n", "");
        assert_eq!(c.rows, vec![removed(1, vec![part("# Old", false)])]);
    }

    #[test]
    fn numbers_lines_from_the_top_of_the_file() {
        let before = "---\nid: x\n---\nText\n";
        let after = "---\nid: x\ntitle: y\n---\nText!\n";
        let c = compare(before, after);
        assert_eq!(c.rows[0], removed(4, vec![part("Text", false)]));
        assert_eq!(
            c.rows[1],
            added(5, vec![part("Text", false), part("!", true)])
        );
    }

    #[test]
    fn line_endings_do_not_count() {
        let c = compare("a\r\nb\r\n", "a\nb");
        assert!(c.same_body());
    }

    #[test]
    fn words_of_any_script() {
        let c = compare("Grüße aus Wien\n", "Grüße aus Graz\n");
        assert_eq!(
            c.rows[0],
            removed(1, vec![part("Grüße aus ", false), part("Wien", true)])
        );
        let c = compare("日本語のテキスト\n", "日本語の文章\n");
        assert_eq!((c.added, c.removed), (1, 1));
    }

    #[test]
    fn front_matter_is_compared_key_by_key() {
        let before = "---\nid: 01ARZ\nmodified: 2026-11-17T10:02:00Z\naliases:\n  - Soup\nold: gone\n---\nText\n";
        let after = "---\nid: 01ARZ\nmodified: 2026-11-17T10:31:00Z\naliases:\n  - Soup\n  - Broth\npinned: true\n---\nText\n";
        let c = compare(before, after);
        assert!(c.same_body());
        let change = |key: &str, before: Option<&str>, after: Option<&str>| KeyChange {
            key: key.into(),
            before: before.map(Into::into),
            after: after.map(Into::into),
        };
        assert_eq!(
            c.front_matter,
            vec![
                change(
                    "modified",
                    Some("2026-11-17T10:02:00Z"),
                    Some("2026-11-17T10:31:00Z")
                ),
                change("aliases", Some("  - Soup"), Some("  - Soup\n  - Broth")),
                change("pinned", None, Some("true")),
                change("old", Some("gone"), None),
            ]
        );
    }

    #[test]
    fn front_matter_on_one_side_only() {
        let c = compare("Text\n", "---\nid: x\n---\nText\n");
        assert!(c.same_body());
        assert_eq!(c.front_matter.len(), 1);
        assert_eq!(c.front_matter[0].before, None);
        assert_eq!(c.rows, vec![same(1, 4, "Text")]);
    }

    #[test]
    fn a_huge_note_is_compared_in_time() {
        use std::fmt::Write as _;
        let (mut before, mut after) = (String::new(), String::new());
        for n in 0..30_000 {
            writeln!(before, "line {n} {}", n % 7).unwrap();
            writeln!(after, "line {} {}", n * 3 % 30_000, n % 5).unwrap();
        }
        let started = Instant::now();
        let c = compare(&before, &after);
        assert!(
            started.elapsed() < TIME_LIMIT * 8,
            "{:?}",
            started.elapsed()
        );
        assert!(c.added > 0 && c.removed > 0);
    }

    /// Golden files in `tests/fixtures/format/7-compare/`: `<case>.before.md`
    /// and `<case>.after.md` compared give `<case>.json`; `SCHNIPSLE_BLESS=1`
    /// writes the JSON anew. The history playground (`ui/dev/history.html`)
    /// shows the same cases.
    #[test]
    fn the_fixture_comparisons() {
        let fixtures = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/7-compare");
        let mut cases: Vec<String> = std::fs::read_dir(&fixtures)
            .unwrap()
            .filter_map(|e| {
                let name = e.unwrap().file_name().into_string().unwrap();
                name.strip_suffix(".before.md").map(str::to_owned)
            })
            .collect();
        cases.sort();
        assert!(cases.len() >= 4, "{cases:?}");
        for case in cases {
            let read = |ext: &str| std::fs::read_to_string(fixtures.join(format!("{case}{ext}")));
            let comparison = compare(&read(".before.md").unwrap(), &read(".after.md").unwrap());
            let output = serde_json::to_string_pretty(&comparison).unwrap() + "\n";
            let path = fixtures.join(format!("{case}.json"));
            if std::env::var_os("SCHNIPSLE_BLESS").is_some() {
                std::fs::write(&path, &output).unwrap();
            }
            assert_eq!(output, std::fs::read_to_string(&path).unwrap(), "{case}");
        }
    }
}
