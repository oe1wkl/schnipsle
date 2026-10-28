// SPDX-License-Identifier: Apache-2.0
//! Tag folders (ADR-0008), answered by the search index (ADR-0009).
//!
//! A folder is a set of tag keys (format.md §5.3, normalized); its notes are
//! those with all of them. The top level holds every top-level tag. Inside a
//! folder are:
//! - its hierarchy subfolders: `#projekte` → `#projekte/tuner`, which takes
//!   the place of the last tag (a note tagged `#a/b` counts for `#a`);
//! - its combinations: every other top-level tag that occurs together with
//!   it, as long as the folder has fewer than [`MAX_COMBINATION_DEPTH`] tags.
//!
//! Each folder has a count; empty ones are not listed. Notes in the trash do
//! not count. A tag shows in the spelling used most often; one used only as
//! a parent (`#projekte` of `#projekte/tuner`) takes its children's.

use std::collections::HashMap;

use rusqlite::params_from_iter;
use serde::Serialize;

use crate::index::{Index, Result};
use crate::layout::NOTES_DIR;
use crate::vault::{NoteEntry, SortOrder, sort_notes, title_sort_key};

/// Most tags in a combination folder (ADR-0008: default 3).
pub const MAX_COMBINATION_DEPTH: usize = 3;

/// How a folder relates to the folder it is listed in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum FolderKind {
    /// A top-level tag.
    Tag,
    /// A hierarchy level below the folder's last tag (`a/b` below `a`).
    Hierarchy,
    /// Another tag combined with the folder's tags (`a` › `b`).
    Combination,
}

/// A tag folder. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TagFolder {
    /// The tag keys whose notes the folder holds; pass them back to list its
    /// subfolders or notes.
    pub keys: Vec<String>,
    /// The folder's own tag as shown, e.g. `Projekte/Tuner`.
    pub name: String,
    /// How it relates to the folder it is listed in.
    pub kind: FolderKind,
    /// Notes in it.
    pub count: usize,
}

/// The common table expression `m` of the active notes having all `keys`.
fn matching(keys: &[String]) -> String {
    if keys.is_empty() {
        return "m(note) AS (SELECT rowid FROM notes WHERE state = 'active')".to_owned();
    }
    let marks = vec!["?"; keys.len()].join(", ");
    format!(
        "m(note) AS (SELECT t.note FROM note_tags t
                     JOIN notes n ON n.rowid = t.note AND n.state = 'active'
                     WHERE t.key IN ({marks}) GROUP BY t.note HAVING count(*) = {})",
        keys.len()
    )
}

/// A folder for `keys`, named after its last key.
fn folder(
    names: &HashMap<String, String>,
    keys: Vec<String>,
    kind: FolderKind,
    count: usize,
) -> TagFolder {
    let key = keys.last().cloned().unwrap_or_default();
    TagFolder {
        name: names.get(&key).cloned().unwrap_or(key),
        keys,
        kind,
        count,
    }
}

/// The top-level segment of a key: `projekte/tuner` → `projekte`.
fn root_of(key: &str) -> &str {
    key.split('/').next().unwrap_or(key)
}

impl Index {
    /// The subfolders of the folder with `keys`; the top level for none.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn tag_folders(&self, keys: &[String]) -> Result<Vec<TagFolder>> {
        let names = self.tag_names()?;
        let mut folders = Vec::new();
        match keys.split_last() {
            None => {
                for (key, count) in self.co_occurring(&[], &[])? {
                    folders.push(folder(&names, vec![key], FolderKind::Tag, count));
                }
            }
            Some((last, rest)) => {
                for (key, count) in self.hierarchy_children(rest, last)? {
                    let mut path = rest.to_vec();
                    path.push(key);
                    folders.push(folder(&names, path, FolderKind::Hierarchy, count));
                }
                if keys.len() < MAX_COMBINATION_DEPTH {
                    let roots: Vec<&str> = keys.iter().map(|k| root_of(k)).collect();
                    for (key, count) in self.co_occurring(keys, &roots)? {
                        let mut path = keys.to_vec();
                        path.push(key);
                        folders.push(folder(&names, path, FolderKind::Combination, count));
                    }
                }
            }
        }
        folders.sort_by_cached_key(|f| (f.kind != FolderKind::Hierarchy, title_sort_key(&f.name)));
        Ok(folders)
    }

    /// Notes (not in the trash) without any tag.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn untagged_count(&self) -> Result<usize> {
        let n: i64 = self.db().query_row(
            "SELECT count(*) FROM notes n WHERE n.state = 'active'
             AND NOT EXISTS (SELECT 1 FROM note_tags t WHERE t.note = n.rowid)",
            [],
            |row| row.get(0),
        )?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    /// The notes in the folder with `keys` (paths relative to `notes/`), in
    /// `order`. With no keys: the notes without any tag.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn notes_in(&self, keys: &[String], order: SortOrder) -> Result<Vec<NoteEntry>> {
        let sql = if keys.is_empty() {
            "SELECT n.path, n.title, n.mtime_ms, n.pinned FROM notes n WHERE n.state = 'active'
             AND NOT EXISTS (SELECT 1 FROM note_tags t WHERE t.note = n.rowid)"
                .to_owned()
        } else {
            format!(
                "WITH {} SELECT n.path, n.title, n.mtime_ms, n.pinned FROM m JOIN notes n ON n.rowid = m.note",
                matching(keys)
            )
        };
        let mut query = self.db().prepare(&sql)?;
        let prefix = format!("{NOTES_DIR}/");
        let rows = query.query_map(params_from_iter(keys), |row| {
            let path: String = row.get(0)?;
            Ok(NoteEntry {
                path: path.strip_prefix(&prefix).unwrap_or(&path).to_owned(),
                title: row.get(1)?,
                modified_ms: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                available: true,
                pinned: row.get(3)?,
            })
        })?;
        let mut notes: Vec<NoteEntry> = rows.collect::<rusqlite::Result<_>>()?;
        sort_notes(&mut notes, order);
        Ok(notes)
    }

    /// Top-level keys on notes having all `keys`, except those under
    /// `excluded_roots`, with the number of such notes.
    fn co_occurring(
        &self,
        keys: &[String],
        excluded_roots: &[&str],
    ) -> Result<Vec<(String, usize)>> {
        let sql = format!(
            "WITH {} SELECT t.key, count(*) FROM note_tags t JOIN m ON m.note = t.note
             WHERE instr(t.key, '/') = 0 GROUP BY t.key",
            matching(keys)
        );
        let mut query = self.db().prepare(&sql)?;
        let rows = query.query_map(params_from_iter(keys), |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (key, count) = row?;
            if !excluded_roots.contains(&key.as_str()) {
                out.push((key, usize::try_from(count).unwrap_or(0)));
            }
        }
        Ok(out)
    }

    /// The keys one level below `parent` on notes having all of `keys`, with
    /// the number of such notes.
    fn hierarchy_children(&self, keys: &[String], parent: &str) -> Result<Vec<(String, usize)>> {
        let n = keys.len();
        let sql = format!(
            "WITH {} SELECT t.key, count(*) FROM note_tags t JOIN m ON m.note = t.note
             WHERE substr(t.key, 1, length(?{p}) + 1) = ?{p} || '/'
               AND instr(substr(t.key, length(?{p}) + 2), '/') = 0
             GROUP BY t.key",
            matching(keys),
            p = n + 1
        );
        let mut query = self.db().prepare(&sql)?;
        let rows = query.query_map(
            params_from_iter(keys.iter().map(String::as_str).chain([parent])),
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
        )?;
        rows.map(|row| row.map(|(k, c)| (k, usize::try_from(c).unwrap_or(0))))
            .collect::<rusqlite::Result<_>>()
            .map_err(Into::into)
    }

    /// The display name of every key: the spelling used most often (ties: the
    /// first in sort order); for a key used only as a parent, the matching
    /// part of a child's name.
    fn tag_names(&self) -> Result<HashMap<String, String>> {
        let mut query = self.db().prepare(
            "SELECT t.key, t.name, count(*) AS uses FROM note_tags t
             JOIN notes n ON n.rowid = t.note AND n.state = 'active'
             WHERE t.name IS NOT NULL GROUP BY t.key, t.name ORDER BY t.key, uses DESC, t.name",
        )?;
        let rows = query.query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut names: HashMap<String, String> = HashMap::new();
        for row in rows {
            let (key, name) = row?;
            names.entry(key).or_insert(name); // the first row per key is the most used
        }
        // A parent used only as such takes the spelling of its first named descendant.
        let mut spelled: Vec<(String, String)> =
            names.iter().map(|(k, n)| (k.clone(), n.clone())).collect();
        spelled.sort();
        for (key, name) in spelled {
            let mut end = 0;
            let segments: Vec<&str> = key.split('/').collect();
            for (i, segment) in segments.iter().enumerate().take(segments.len() - 1) {
                end += segment.len() + usize::from(i > 0);
                let parent = &key[..end];
                if let Some(display) = name.get(..end) {
                    names
                        .entry(parent.to_owned())
                        .or_insert_with(|| display.to_owned());
                }
            }
        }
        Ok(names)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{FileStamp, NoteState, analyze};

    fn index(notes: &[(&str, &str)]) -> Index {
        let mut index = Index::in_memory().unwrap();
        for (path, text) in notes {
            let state = if path.starts_with(".notesapp") {
                NoteState::Trash
            } else {
                NoteState::Active
            };
            let stamp = FileStamp {
                mtime_ms: 1,
                size: 1,
                hash: (*path).to_owned(),
            };
            index
                .put(path, state, &stamp, &analyze(path, text))
                .unwrap();
        }
        index
    }

    fn keys(k: &[&str]) -> Vec<String> {
        k.iter().map(|s| (*s).to_owned()).collect()
    }

    fn listing(folders: &[TagFolder]) -> Vec<(String, FolderKind, usize)> {
        folders
            .iter()
            .map(|f| (f.name.clone(), f.kind, f.count))
            .collect()
    }

    fn sample() -> Index {
        index(&[
            ("notes/a.md", "#Radio #cw #projekte/Tuner"),
            ("notes/b.md", "#radio #CW"),
            ("notes/c.md", "#radio #chor"),
            ("notes/d.md", "#Projekte/tuner/bau #chor"),
            ("notes/e.md", "no tags at all"),
            (".notesapp/trash/X.md", "#radio #geheim"),
        ])
    }

    #[test]
    fn top_level_lists_top_level_tags_with_counts() {
        let index = sample();
        assert_eq!(
            listing(&index.tag_folders(&[]).unwrap()),
            [
                ("chor".into(), FolderKind::Tag, 2),
                ("CW".into(), FolderKind::Tag, 2), // "CW" and "cw" once each: sort order decides
                ("projekte".into(), FolderKind::Tag, 2),
                ("radio".into(), FolderKind::Tag, 3),
            ]
        );
        assert_eq!(index.untagged_count().unwrap(), 1);
    }

    #[test]
    fn folders_hold_hierarchy_levels_and_combinations() {
        let index = sample();
        // projekte: the hierarchy level first, then the tags it occurs with.
        assert_eq!(
            listing(&index.tag_folders(&keys(&["projekte"])).unwrap()),
            [
                ("projekte/Tuner".into(), FolderKind::Hierarchy, 2),
                ("chor".into(), FolderKind::Combination, 1),
                ("CW".into(), FolderKind::Combination, 1),
                ("radio".into(), FolderKind::Combination, 1), // "radio" twice, "Radio" once
            ]
        );
        // A hierarchy level replaces the last tag; combinations keep the rest.
        assert_eq!(
            listing(&index.tag_folders(&keys(&["radio", "projekte"])).unwrap()),
            [
                ("projekte/Tuner".into(), FolderKind::Hierarchy, 1),
                ("CW".into(), FolderKind::Combination, 1),
            ]
        );
        let tuner = index.tag_folders(&keys(&["projekte/tuner"])).unwrap();
        assert_eq!(tuner[0].keys, keys(&["projekte/tuner/bau"]));
        assert_eq!(tuner[0].name, "Projekte/tuner/bau");
    }

    #[test]
    fn combinations_are_intersections_and_stop_at_the_depth_limit() {
        let index = sample();
        let radio_cw = index.tag_folders(&keys(&["radio", "cw"])).unwrap();
        assert_eq!(
            listing(&radio_cw),
            [("projekte".into(), FolderKind::Combination, 1)]
        );
        let paths: Vec<_> = index
            .notes_in(&keys(&["cw", "radio"]), SortOrder::Modified) // same mtime: by path
            .unwrap()
            .into_iter()
            .map(|n| n.path)
            .collect();
        assert_eq!(paths, ["a.md", "b.md"], "order of the keys does not matter");
        assert!(
            index
                .tag_folders(&keys(&["radio", "cw", "projekte"]))
                .unwrap()
                .iter()
                .all(|f| f.kind == FolderKind::Hierarchy),
            "no combinations at depth {MAX_COMBINATION_DEPTH}"
        );
    }

    #[test]
    fn names_follow_the_most_used_spelling_and_the_trash_does_not_count() {
        let index = index(&[
            ("notes/a.md", "#Radio"),
            ("notes/b.md", "#radio"),
            ("notes/c.md", "#radio"),
            ("notes/d.md", "#Ham/QRP"),
            (".notesapp/trash/X.md", "#Radio #Radio #trash"),
        ]);
        let top = index.tag_folders(&[]).unwrap();
        assert_eq!(
            listing(&top),
            [
                ("Ham".into(), FolderKind::Tag, 1), // spelled like its child
                ("radio".into(), FolderKind::Tag, 3),
            ]
        );
        let untagged = index.notes_in(&[], SortOrder::Title).unwrap();
        assert!(untagged.is_empty());
    }

    #[test]
    fn pinned_notes_come_first_in_a_folder() {
        let index = index(&[
            ("notes/a.md", "# Apfel #obst"),
            ("notes/b.md", "---\npinned: true\n---\n# Birne #obst"),
            ("notes/c.md", "# Clementine"),
            ("notes/d.md", "---\npinned: true\n---\n# Dattel"),
        ]);
        let titles = |keys: &[String]| -> Vec<(String, bool)> {
            index
                .notes_in(keys, SortOrder::Title)
                .unwrap()
                .into_iter()
                .map(|n| (n.title, n.pinned))
                .collect()
        };
        assert_eq!(
            titles(&keys(&["obst"])),
            [("Birne #obst".into(), true), ("Apfel #obst".into(), false)]
        );
        assert_eq!(
            titles(&[]),
            [("Dattel".into(), true), ("Clementine".into(), false)]
        );
    }
}
