// SPDX-License-Identifier: Apache-2.0
//! Renaming and merging tags (F-11, ADR-0040).
//!
//! A rename changes every tag whose key is the old tag's or one of its
//! subtags', in note bodies and in smart-folder queries. A merge is a rename
//! into a tag that already exists. Only what format.md §5.3 takes as a tag is
//! changed: occurrences of `#old` are found in the note's source, and the
//! ones the tag extraction of [`crate::tags`] does not count (code, links,
//! `\#`, front matter, …) stay as they are. So does every other byte.
//!
//! [`Vault::plan_retag`] says what would change; [`Vault::apply_retag`]
//! writes it note by note, each after a snapshot and only if unchanged since
//! the plan; [`Vault::undo_retag`] puts back what it wrote.

use std::collections::HashSet;
use std::fmt;
use std::ops::Range;

use serde::{Deserialize, Serialize};
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::smart_folders::SmartFolderError;
use crate::tags::{extract_tags, normalize};
use crate::vault::{Result, Vault, VaultError};

/// Why a rename cannot be done.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InvalidRename {
    /// The name is not a tag by format.md §5.3.
    NotATag(String),
    /// The new name is a subtag of the old one, which would rename forever.
    IntoItself,
}

impl fmt::Display for InvalidRename {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotATag(name) if name.is_empty() => write!(f, "Enter a name for the tag"),
            Self::NotATag(name) => write!(
                f,
                "#{name} is not a tag: use letters, digits, _, - and / between levels"
            ),
            Self::IntoItself => write!(f, "A tag cannot be moved into one of its own subtags"),
        }
    }
}

impl std::error::Error for InvalidRename {}

/// A rename of the tag `old` (and its subtags) to `new`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TagRename {
    old: String,
    old_key: String,
    old_levels: usize,
    new: String,
    new_key: String,
}

/// A note's text after a rename.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Renamed {
    /// The whole text, with the tags renamed.
    pub text: String,
    /// How many tags were changed.
    pub occurrences: usize,
}

impl TagRename {
    /// The rename of `old` to `new`, both written with or without `#`.
    ///
    /// # Errors
    /// [`InvalidRename`] if either is not a tag name, or `new` is a subtag of `old`.
    pub fn new(old: &str, new: &str) -> std::result::Result<Self, InvalidRename> {
        let old = tag_name(old)?;
        let new = tag_name(new)?;
        let old_key = normalize(&old);
        let new_key = normalize(&new);
        if new_key.starts_with(&format!("{old_key}/")) {
            return Err(InvalidRename::IntoItself);
        }
        Ok(Self {
            old_levels: old_key.split('/').count(),
            old,
            old_key,
            new,
            new_key,
        })
    }

    /// The old tag, without `#`.
    #[must_use]
    pub fn old(&self) -> &str {
        &self.old
    }

    /// The old tag's key (format.md §5.3).
    #[must_use]
    pub fn old_key(&self) -> &str {
        &self.old_key
    }

    /// The new tag, without `#`, NFC-normalized.
    #[must_use]
    pub fn new_name(&self) -> &str {
        &self.new
    }

    /// The new tag's key (format.md §5.3).
    #[must_use]
    pub fn new_key(&self) -> &str {
        &self.new_key
    }

    /// The tag name `name` renamed, if it is the old tag or one of its
    /// subtags: its first levels are replaced by the new name.
    fn renamed_name(&self, name: &str) -> Option<String> {
        let key = normalize(name);
        if key != self.old_key && !key.starts_with(&format!("{}/", self.old_key)) {
            return None;
        }
        Some(
            match name.splitn(self.old_levels + 1, '/').nth(self.old_levels) {
                Some(rest) => format!("{}/{rest}", self.new),
                None => self.new.clone(),
            },
        )
    }

    /// `markdown` with the tags renamed; `None` if no tag changes.
    #[must_use]
    pub fn rename_text(&self, markdown: &str) -> Option<Renamed> {
        let found: Vec<(Range<usize>, String)> = candidates(markdown)
            .into_iter()
            .filter_map(|range| {
                self.renamed_name(&markdown[range.clone()])
                    .map(|name| (range, name))
            })
            .collect();
        if found.is_empty() {
            return None;
        }
        let real = real_tags(markdown, found.iter().map(|(range, _)| range.clone()));
        let mut text = String::with_capacity(markdown.len());
        let mut last = 0;
        let mut occurrences = 0;
        for ((range, name), is_tag) in found.into_iter().zip(real) {
            if !is_tag || markdown[range.clone()] == name {
                continue;
            }
            text.push_str(&markdown[last..range.start]);
            text.push_str(&name);
            last = range.end;
            occurrences += 1;
        }
        if occurrences == 0 {
            return None;
        }
        text.push_str(&markdown[last..]);
        Some(Renamed { text, occurrences })
    }

    /// The smart-folder `query` with its tags renamed, `#tag` and `tag:tag`
    /// alike (`docs/manual/query-language.md`); `None` if none changes.
    /// Phrases in quotes stay as they are.
    #[must_use]
    pub fn rename_query(&self, query: &str) -> Option<String> {
        let chars: Vec<(usize, char)> = query.char_indices().collect();
        let byte = |i: usize| chars.get(i).map_or(query.len(), |&(b, _)| b);
        let mut out = String::new();
        let mut last = 0;
        let mut i = 0;
        while i < chars.len() {
            match chars[i].1 {
                c if c.is_whitespace() || matches!(c, '(' | ')' | '-') => i += 1,
                '"' => {
                    i += 1;
                    while i < chars.len() && chars[i].1 != '"' {
                        i += 1;
                    }
                    i += 1;
                }
                _ => {
                    let start = i;
                    while i < chars.len()
                        && !chars[i].1.is_whitespace()
                        && !matches!(chars[i].1, '(' | ')' | '"')
                    {
                        i += 1;
                    }
                    let word = &query[byte(start)..byte(i)];
                    let Some(offset) = tag_in_query_word(word) else {
                        continue;
                    };
                    let name = word[offset..].trim_end_matches(['/', '-']);
                    if let Some(renamed) = self.renamed_name(name)
                        && renamed != name
                    {
                        let at = byte(start) + offset;
                        out.push_str(&query[last..at]);
                        out.push_str(&renamed);
                        last = at + name.len();
                    }
                }
            }
        }
        if last == 0 {
            return None;
        }
        out.push_str(&query[last..]);
        Some(out)
    }
}

/// `name` without `#` and surrounding spaces, NFC-normalized, if it is a tag
/// name by format.md §5.3.
fn tag_name(name: &str) -> std::result::Result<String, InvalidRename> {
    let name: String = name.trim().trim_start_matches('#').nfc().collect();
    let found = extract_tags(&format!("#{name}"));
    if found.len() == 1 && found[0].name == name {
        Ok(name)
    } else {
        Err(InvalidRename::NotATag(name))
    }
}

/// Where the tag of a query word starts, if it has one: after `#`, or after
/// `tag:` and any `#`.
fn tag_in_query_word(word: &str) -> Option<usize> {
    if word.starts_with('#') {
        return Some(1);
    }
    let (name, value) = word.split_once(':')?;
    if !name.eq_ignore_ascii_case("tag") {
        return None;
    }
    Some(name.len() + 1 + (value.len() - value.trim_start_matches('#').len()))
}

/// A character that may continue a tag in the source. Combining marks count,
/// so a letter written decomposed stays one tag (§5.3 scans NFC text).
fn is_tag_char(c: char) -> bool {
    c.is_alphanumeric() || c == '_' || c == '-' || is_combining_mark(c)
}

/// The byte ranges of every name following a `#` in `text`, scanned as
/// §5.3 scans visible text but without its word-boundary rule: whether one
/// is a tag is for [`real_tags`] to say.
fn candidates(text: &str) -> Vec<Range<usize>> {
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let byte = |i: usize| chars.get(i).map_or(text.len(), |&(b, _)| b);
    let mut found = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i].1 != '#' {
            i += 1;
            continue;
        }
        let start = i + 1;
        let mut end = start;
        loop {
            while end < chars.len() && is_tag_char(chars[end].1) {
                end += 1;
            }
            if end + 1 < chars.len() && chars[end].1 == '/' && is_tag_char(chars[end + 1].1) {
                end += 1;
            } else {
                break;
            }
        }
        let name = text[byte(start)..byte(end)].trim_end_matches(['-', '/']);
        if !name.is_empty() {
            found.push(byte(start)..byte(start) + name.len());
        }
        i = end.max(i + 1);
    }
    found
}

/// For each of `ranges` (names after a `#`, in order), whether §5.3 takes it
/// as a tag. Each name is replaced by a distinct made-up tag and the text
/// parsed again: those found are tags. The made-up names are letters only,
/// so they change nothing else the parser looks at.
fn real_tags(text: &str, ranges: impl Iterator<Item = Range<usize>>) -> Vec<bool> {
    let lowercase = text.to_lowercase();
    let mut prefix = String::from("schnipsleretag");
    while lowercase.contains(&prefix) {
        prefix.push('x');
    }
    let mut probe = String::with_capacity(text.len());
    let mut last = 0;
    let mut names = Vec::new();
    for (n, range) in ranges.enumerate() {
        let name = format!("{prefix}{}", letters(n));
        probe.push_str(&text[last..range.start]);
        probe.push_str(&name);
        last = range.end;
        names.push(name);
    }
    probe.push_str(&text[last..]);
    let found: HashSet<String> = extract_tags(&probe).into_iter().map(|t| t.key).collect();
    names.iter().map(|name| found.contains(name)).collect()
}

/// `n` written with the letters a–z, never empty: 0 → `a`, 26 → `ba`.
fn letters(mut n: usize) -> String {
    let mut out = Vec::new();
    loop {
        out.push(b'a' + u8::try_from(n % 26).unwrap_or(0));
        n /= 26;
        if n == 0 {
            break;
        }
    }
    out.reverse();
    String::from_utf8(out).unwrap_or_default()
}

/// What a rename will change, for the user to confirm. Serialized with
/// camelCase names; given back to [`Vault::apply_retag`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RetagPlan {
    /// The tag renamed, without `#`.
    pub old: String,
    /// Its new name, without `#`.
    pub new: String,
    /// `true` if the new name is a tag already, so the two are merged.
    pub merge: bool,
    /// The notes that change, in the order given.
    pub notes: Vec<PlannedNote>,
    /// The smart folders whose query changes.
    pub folders: Vec<PlannedFolder>,
    /// Notes that could not be read, which keep their tags.
    pub skipped: Vec<SkippedNote>,
}

/// A note a rename changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedNote {
    /// Path relative to `notes/`.
    pub path: String,
    /// Its title (format.md §5.2).
    pub title: String,
    /// The hash of the content the plan was made from.
    pub hash: String,
    /// How many tags change in it.
    pub occurrences: usize,
}

/// A smart folder a rename changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PlannedFolder {
    /// The folder's `id`.
    pub id: String,
    /// Its `name`.
    pub name: String,
    /// Its query now.
    pub query: String,
    /// Its query after the rename.
    pub renamed: String,
}

/// A note left as it is, and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedNote {
    /// Path relative to `notes/`.
    pub path: String,
    /// Its title, or the path if it could not be read.
    pub title: String,
    /// Why, for the user.
    pub reason: String,
}

/// What a rename (or its undo) did. Serialized with camelCase names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RetagOutcome {
    /// The notes written.
    pub changed: Vec<String>,
    /// The notes left as they are.
    pub skipped: Vec<SkippedNote>,
    /// How many smart folders changed.
    pub folders: usize,
    /// Why the smart folders could not be changed, if they could not.
    pub folders_error: Option<String>,
}

/// What [`Vault::undo_retag`] needs to undo a rename: the text each note had
/// before, and the hash it had after. Kept by the app, not in the vault.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RetagUndo {
    notes: Vec<(String, String, String)>,
    folders: Vec<PlannedFolder>,
}

impl RetagUndo {
    /// `true` if the rename wrote nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.notes.is_empty() && self.folders.is_empty()
    }
}

impl Vault {
    /// What renaming per `rename` changes in the notes at `paths` (relative
    /// to `notes/`, e.g. those the index has under the old tag) and in the
    /// smart folders. `merge` says whether the new tag exists already. Nothing
    /// is written.
    ///
    /// # Errors
    /// [`SmartFolderError`] if the smart folders cannot be read.
    pub fn plan_retag(
        &self,
        rename: &TagRename,
        paths: &[String],
        merge: bool,
    ) -> std::result::Result<RetagPlan, SmartFolderError> {
        let mut notes = Vec::new();
        let mut skipped = Vec::new();
        for path in paths {
            match self.read_note_text(path) {
                Ok(note) => {
                    if let Some(renamed) = rename.rename_text(&note.text) {
                        notes.push(PlannedNote {
                            path: note.path,
                            title: note.title,
                            hash: note.hash,
                            occurrences: renamed.occurrences,
                        });
                    }
                }
                Err(e) => skipped.push(SkippedNote {
                    path: path.clone(),
                    title: path.clone(),
                    reason: e.to_string(),
                }),
            }
        }
        let folders = self
            .smart_folders()?
            .list()
            .into_iter()
            .filter_map(|folder| {
                rename
                    .rename_query(&folder.query)
                    .map(|renamed| PlannedFolder {
                        id: folder.id,
                        name: folder.name,
                        query: folder.query,
                        renamed,
                    })
            })
            .collect();
        Ok(RetagPlan {
            old: rename.old().to_owned(),
            new: rename.new_name().to_owned(),
            merge,
            notes,
            folders,
            skipped,
        })
    }

    /// Carries out `plan`. Each note is renamed only if it still has the
    /// content the plan was made from, after a snapshot of it (format.md §7);
    /// a note changed since, or that cannot be written, is skipped and the
    /// others are still renamed. The smart folders are renamed on their
    /// current content (format.md §9 rule 7). Returns what was done and what
    /// [`Vault::undo_retag`] needs.
    ///
    /// # Errors
    /// [`InvalidRename`] (as an I/O error of kind `InvalidInput`) if the plan
    /// names no valid rename, or [`VaultError::ReadOnly`]; nothing is written
    /// then.
    pub fn apply_retag(&self, plan: &RetagPlan) -> Result<(RetagOutcome, RetagUndo)> {
        if let Some(why) = self.read_only() {
            return Err(VaultError::ReadOnly(why.to_owned()));
        }
        let rename = TagRename::new(&plan.old, &plan.new)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidInput, e))?;
        let mut outcome = RetagOutcome::default();
        let mut undo = RetagUndo::default();
        for planned in &plan.notes {
            let skip = |reason: String| SkippedNote {
                path: planned.path.clone(),
                title: planned.title.clone(),
                reason,
            };
            match self.retag_note(&rename, planned) {
                Ok(Some((before, after_hash))) => {
                    outcome.changed.push(planned.path.clone());
                    undo.notes.push((planned.path.clone(), before, after_hash));
                }
                Ok(None) => {}
                Err(VaultError::Conflict { .. }) => outcome
                    .skipped
                    .push(skip("It changed since the rename was planned".to_owned())),
                Err(e) => outcome.skipped.push(skip(e.to_string())),
            }
        }
        let renamed = self.edit_smart_folders(|folders| {
            let mut changed = Vec::new();
            for folder in folders.list() {
                if let Some(query) = rename.rename_query(&folder.query) {
                    folders.update(&folder.id, None, Some(&query), None)?;
                    changed.push(PlannedFolder {
                        id: folder.id,
                        name: folder.name,
                        query: folder.query,
                        renamed: query,
                    });
                }
            }
            Ok(changed)
        });
        match renamed {
            Ok(changed) => {
                outcome.folders = changed.len();
                undo.folders = changed;
            }
            Err(e) => outcome.folders_error = Some(e.to_string()),
        }
        Ok((outcome, undo))
    }

    /// Renames the tags of one planned note; the text it had and the hash it
    /// has now, or `None` if nothing changed.
    fn retag_note(
        &self,
        rename: &TagRename,
        planned: &PlannedNote,
    ) -> Result<Option<(String, String)>> {
        let note = self.read_note_text(&planned.path)?;
        if note.hash != planned.hash {
            return Err(VaultError::Conflict {
                path: planned.path.clone(),
                current_hash: Some(note.hash),
            });
        }
        let Some(renamed) = rename.rename_text(&note.text) else {
            return Ok(None);
        };
        self.snapshot_note(&note.path, Some(&note.text))?;
        let saved = self.write_note(&note.path, &renamed.text, &note.hash)?;
        Ok(Some((note.text, saved.hash)))
    }

    /// Undoes a rename: each note that still has the content the rename wrote
    /// gets its earlier text back, after a snapshot of the current one; a note
    /// changed since is kept and listed. Smart folders whose query is still
    /// the renamed one get the old query back.
    ///
    /// # Errors
    /// [`VaultError::ReadOnly`]; nothing is written then.
    pub fn undo_retag(&self, undo: &RetagUndo) -> Result<RetagOutcome> {
        if let Some(why) = self.read_only() {
            return Err(VaultError::ReadOnly(why.to_owned()));
        }
        let mut outcome = RetagOutcome::default();
        for (path, before, after_hash) in &undo.notes {
            let restored = self.read_note_text(path).and_then(|note| {
                if note.hash != *after_hash {
                    return Err(VaultError::Conflict {
                        path: path.clone(),
                        current_hash: Some(note.hash),
                    });
                }
                self.snapshot_note(path, Some(&note.text))?;
                self.write_note(path, before, after_hash)
            });
            match restored {
                Ok(_) => outcome.changed.push(path.clone()),
                Err(e) => outcome.skipped.push(SkippedNote {
                    path: path.clone(),
                    title: path.clone(),
                    reason: match e {
                        VaultError::Conflict { .. } => {
                            "It changed after the rename, so it is kept".to_owned()
                        }
                        e => e.to_string(),
                    },
                }),
            }
        }
        if !undo.folders.is_empty() {
            let restored = self.edit_smart_folders(|folders| {
                let mut count = 0;
                for folder in folders.list() {
                    if let Some(old) = undo
                        .folders
                        .iter()
                        .find(|f| f.id == folder.id && f.renamed == folder.query)
                    {
                        folders.update(&folder.id, None, Some(&old.query), None)?;
                        count += 1;
                    }
                }
                Ok(count)
            });
            match restored {
                Ok(count) => outcome.folders = count,
                Err(e) => outcome.folders_error = Some(e.to_string()),
            }
        }
        Ok(outcome)
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::smart_folders::FolderSort;

    fn rename(old: &str, new: &str) -> TagRename {
        TagRename::new(old, new).unwrap()
    }

    fn renamed(old: &str, new: &str, text: &str) -> Option<String> {
        rename(old, new).rename_text(text).map(|r| r.text)
    }

    #[test]
    fn names_are_checked() {
        assert_eq!(rename("#radio", " Funk ").new_name(), "Funk");
        assert_eq!(rename("Gru\u{308}n", "x").old(), "Grün");
        for bad in ["", "#", "123", "a b", "00f", "radio/"] {
            assert!(
                matches!(TagRename::new("radio", bad), Err(InvalidRename::NotATag(_))),
                "{bad}"
            );
        }
        assert_eq!(
            TagRename::new("projects", "Projects/old"),
            Err(InvalidRename::IntoItself)
        );
        // Into its parent, or a spelling of itself, is fine.
        rename("projects/tuner", "projects");
        rename("radio", "Radio");
    }

    #[test]
    fn tags_and_subtags_are_renamed() {
        assert_eq!(
            renamed(
                "projects",
                "Work",
                "Tuner #Projects/Tuner and #projects, not #projectsX.\n"
            )
            .as_deref(),
            Some("Tuner #Work/Tuner and #Work, not #projectsX.\n")
        );
        assert_eq!(
            renamed("radio/qrp", "kits", "#Radio/QRP/40m #radio #radio/qrp-").as_deref(),
            Some("#kits/40m #radio #kits-")
        );
        assert_eq!(
            renamed("a", "b", "#a #a #A").map(|t| t.len()),
            Some("#b #b #b".len())
        );
        let r = rename("a", "b").rename_text("#a x #A").unwrap();
        assert_eq!(r.occurrences, 2);
        assert_eq!(renamed("radio", "funk", "no tags here"), None);
    }

    #[test]
    fn only_tags_change() {
        let text = "---\nid: x\nnote: '#radio'\n---\n\
            # Heading #radio\n\n\
            Real #radio, `#radio`, \\#radio, page#radio, [#radio](x.md#radio), \
            [x](https://e.org/#radio), C#radio, <span title=\"#radio\">#radio</span>\n\n\
            ```\n#radio\n```\n\n    #radio\n\n\
            ![#radio](p.png) $#radio$ **#radio** \\_#radio\n";
        let expected = "---\nid: x\nnote: '#radio'\n---\n\
            # Heading #funk\n\n\
            Real #funk, `#radio`, \\#radio, page#radio, [#funk](x.md#radio), \
            [x](https://e.org/#radio), C#radio, <span title=\"#radio\">#funk</span>\n\n\
            ```\n#radio\n```\n\n    #radio\n\n\
            ![#radio](p.png) $#radio$ **#funk** \\_#funk\n";
        assert_eq!(renamed("radio", "funk", text).as_deref(), Some(expected));
        assert_eq!(
            extract_tags(expected)
                .into_iter()
                .map(|t| t.key)
                .collect::<Vec<_>>(),
            ["funk"]
        );
    }

    // The example table of format.md §5.3: each tag found there is renamed,
    // and the rows without tags stay as they are.
    #[test]
    fn spec_examples() {
        let rows = [
            (
                "Antenna test #radio #projects/tuner",
                "projects",
                Some("Antenna test #radio #x/tuner"),
            ),
            ("# Heading", "heading", None),
            ("Colour #00ff00 and issue #123", "radio", None),
            ("Grüße an #Österreich", "österreich", Some("Grüße an #x")),
            ("`#notatag`", "notatag", None),
            ("https://example.com/page#anchor", "anchor", None),
            ("C# and F#", "radio", None),
            ("#cw_training, #m32.", "m32", Some("#cw_training, #x.")),
            (
                "Ideas to #add for the #decade",
                "decade",
                Some("Ideas to #add for the #x"),
            ),
            ("Not a tag: \\#literal", "literal", None),
        ];
        for (text, old, expected) in rows {
            assert_eq!(renamed(old, "x", text).as_deref(), expected, "{text}");
        }
    }

    #[test]
    fn decomposed_letters_stay_one_tag() {
        assert_eq!(
            renamed("grün", "green", "Farbe #Gru\u{308}n/hell.").as_deref(),
            Some("Farbe #green/hell.")
        );
    }

    #[test]
    fn a_spelling_rename_changes_only_other_spellings() {
        let r = rename("radio", "Radio")
            .rename_text("#Radio #radio #RADIO")
            .unwrap();
        assert_eq!(r.text, "#Radio #Radio #Radio");
        assert_eq!(r.occurrences, 2);
        assert_eq!(rename("radio", "Radio").rename_text("#Radio"), None);
    }

    #[test]
    fn queries_are_renamed() {
        let r = rename("radio", "Funk");
        assert_eq!(
            r.rename_query("#radio -#Radio/QRP (tag:radio OR tag:#radio/kits) \"#radio\" #radiox")
                .as_deref(),
            Some("#Funk -#Funk/QRP (tag:Funk OR tag:#Funk/kits) \"#radio\" #radiox")
        );
        assert_eq!(r.rename_query("#radio/"), Some("#Funk/".to_owned()));
        assert_eq!(r.rename_query("radio has:task"), None);
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    fn all(vault: &Vault) -> Vec<String> {
        vault
            .list_notes()
            .unwrap()
            .into_iter()
            .map(|n| n.path)
            .collect()
    }

    #[test]
    fn a_rename_is_planned_applied_and_undone() {
        let (dir, vault) = vault();
        vault
            .create_note("tuner.md", "# Tuner\n\n#Radio/QRP kit\n")
            .unwrap();
        vault
            .create_note("log.md", "# Log\n\n`#radio` only\n")
            .unwrap();
        vault.create_note("other.md", "# Other\n\n#cw\n").unwrap();
        vault
            .edit_smart_folders(|f| {
                f.add("QRP", "#radio/qrp -#done", None, FolderSort::default())?;
                f.add("CW", "#cw", None, FolderSort::default())
            })
            .unwrap();

        let rename = rename("radio", "Funk");
        let plan = vault.plan_retag(&rename, &all(&vault), false).unwrap();
        assert_eq!(plan.notes.len(), 1);
        assert_eq!(plan.notes[0].path, "tuner.md");
        assert_eq!(plan.notes[0].title, "Tuner");
        assert_eq!(plan.notes[0].occurrences, 1);
        assert_eq!(plan.folders.len(), 1);
        assert_eq!(plan.folders[0].renamed, "#Funk/qrp -#done");
        assert!(
            vault
                .read_note_text("tuner.md")
                .unwrap()
                .text
                .contains("#Radio/QRP"),
            "a plan writes nothing"
        );

        let before = vault.read_note_text("tuner.md").unwrap().text;
        let (outcome, undo) = vault.apply_retag(&plan).unwrap();
        assert_eq!(outcome.changed, ["tuner.md"]);
        assert!(outcome.skipped.is_empty());
        assert_eq!(outcome.folders, 1);
        let after = vault.read_note_text("tuner.md").unwrap().text;
        assert!(after.contains("#Funk/QRP kit"), "{after}");
        let versions = vault.versions("tuner.md").unwrap();
        assert!(!versions.is_empty(), "the old version is in the history");
        let queries: Vec<String> = vault
            .smart_folders()
            .unwrap()
            .list()
            .into_iter()
            .map(|f| f.query)
            .collect();
        assert_eq!(queries, ["#Funk/qrp -#done", "#cw"]);

        let undone = vault.undo_retag(&undo).unwrap();
        assert_eq!(undone.changed, ["tuner.md"]);
        assert_eq!(undone.folders, 1);
        let back = vault.read_note_text("tuner.md").unwrap().text;
        assert!(back.contains("#Radio/QRP kit"), "{back}");
        assert_eq!(
            front_matter_body(&back),
            front_matter_body(&before),
            "the body is as before"
        );
        let queries: Vec<String> = vault
            .smart_folders()
            .unwrap()
            .list()
            .into_iter()
            .map(|f| f.query)
            .collect();
        assert_eq!(queries, ["#radio/qrp -#done", "#cw"]);
        drop(dir);
    }

    fn front_matter_body(text: &str) -> &str {
        text.splitn(3, "---\n").nth(2).unwrap_or(text)
    }

    #[test]
    fn notes_changed_meanwhile_are_left_alone() {
        let (_dir, vault) = vault();
        vault.create_note("a.md", "# A\n\n#radio\n").unwrap();
        vault.create_note("b.md", "# B\n\n#radio\n").unwrap();
        let plan = vault
            .plan_retag(&rename("radio", "funk"), &all(&vault), false)
            .unwrap();
        assert_eq!(plan.notes.len(), 2);

        // Changed elsewhere after the plan: it is not renamed.
        let b = vault.note_file("b.md").unwrap();
        let changed = fs::read_to_string(&b).unwrap() + "more #radio\n";
        fs::write(&b, &changed).unwrap();
        let (outcome, undo) = vault.apply_retag(&plan).unwrap();
        assert_eq!(outcome.changed, ["a.md"]);
        assert_eq!(outcome.skipped.len(), 1);
        assert_eq!(outcome.skipped[0].path, "b.md");
        assert_eq!(fs::read_to_string(&b).unwrap(), changed);

        // Changed after the rename: the undo keeps it.
        let a = vault.note_file("a.md").unwrap();
        let edited = fs::read_to_string(&a).unwrap() + "edited\n";
        fs::write(&a, &edited).unwrap();
        let undone = vault.undo_retag(&undo).unwrap();
        assert!(undone.changed.is_empty());
        assert_eq!(undone.skipped.len(), 1);
        assert_eq!(fs::read_to_string(&a).unwrap(), edited);
    }

    #[test]
    fn a_merge_keeps_every_occurrence() {
        let (_dir, vault) = vault();
        vault
            .create_note("a.md", "# A\n\n#ham #amateurfunk\n")
            .unwrap();
        let plan = vault
            .plan_retag(&rename("ham", "Amateurfunk"), &all(&vault), true)
            .unwrap();
        assert!(plan.merge);
        vault.apply_retag(&plan).unwrap();
        let text = vault.read_note_text("a.md").unwrap().text;
        assert!(text.contains("#Amateurfunk #amateurfunk"), "{text}");
    }

    #[test]
    fn letters_count_on() {
        assert_eq!(letters(0), "a");
        assert_eq!(letters(25), "z");
        assert_eq!(letters(26), "ba");
    }
}
