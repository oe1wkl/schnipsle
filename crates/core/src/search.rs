// SPDX-License-Identifier: Apache-2.0
//! Full-text search (F-09) on the index (ADR-0009).
//!
//! A [`Query`] is compiled to SQL with every user term as a parameter: words
//! and phrases become FTS5 matches (`"word"*` for a prefix, `"a b"` for a
//! phrase, always quoted, so FTS5 syntax in the query has no effect), tags
//! become lookups in `note_tags` (which holds the ancestors, so `#a` finds
//! `#a/b`), dates become ranges on `created_ms` or `modified_ms` (days begin
//! at local midnight, from the UTC offset the app passes), `has:` and `is:`
//! become flags, and `-`, `OR` and parentheses become `NOT`, `OR` and
//! parentheses in SQL. The trash is searched only by a query that mentions
//! `in:trash`.
//!
//! Results are ranked with `bm25` over the words and phrases that must match,
//! the title counting five times the body, and carry a snippet with the hits
//! marked. A query with only tags or exclusions sorts by modification time.

use rusqlite::params_from_iter;
use rusqlite::types::Value;
use serde::Serialize;

use crate::index::{Index, Result};
use crate::layout::NOTES_DIR;
use crate::query::{DateBound, DateField, Feature, Flag, Query};
use crate::stamp::now_ms;

/// Most results returned.
pub const MAX_HITS: usize = 200;

/// Marks the start and end of a hit in a snippet; control characters, so
/// they cannot occur in the text and need no escaping.
const HIT_START: char = '\u{2}';
const HIT_END: char = '\u{3}';

/// A piece of a snippet. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnippetPart {
    /// The text.
    pub text: String,
    /// `true` for a word that matched.
    pub hit: bool,
}

/// A note found by [`Index::search`]. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    /// Path relative to `notes/`.
    pub path: String,
    /// Title per format.md §5.2.
    pub title: String,
    /// Last modification, milliseconds since the Unix epoch.
    pub modified_ms: u64,
    /// Text around the hits; empty if the query has no words.
    pub snippet: Vec<SnippetPart>,
    /// In the trash: `path` is then below the vault root
    /// (`.notesapp/trash/…`) and the note must be restored to be opened.
    pub in_trash: bool,
}

/// What dates mean: now, and local time minus UTC in minutes.
#[derive(Debug, Clone, Copy)]
struct Clock {
    now_ms: i64,
    offset_minutes: i64,
}

/// An FTS5 query for one word (as a prefix) or phrase, quoted.
fn fts_term(term: &Query) -> Option<String> {
    let (text, prefix) = match term {
        Query::Word(w) => (w, true),
        Query::Phrase(p) => (p, false),
        _ => return None,
    };
    let quoted = format!("\"{}\"", text.replace('"', "\"\""));
    Some(if prefix { format!("{quoted}*") } else { quoted })
}

/// Appends the SQL condition for `query` on the note `n` to `sql`, its
/// parameters to `params`.
fn compile(query: &Query, clock: Clock, sql: &mut String, params: &mut Vec<Value>) {
    match query {
        Query::Word(_) | Query::Phrase(_) => {
            sql.push_str("n.rowid IN (SELECT rowid FROM notes_fts WHERE notes_fts MATCH ?)");
            params.extend(fts_term(query).map(Value::Text));
        }
        Query::Tag(key) => {
            sql.push_str("n.rowid IN (SELECT note FROM note_tags WHERE key = ?)");
            params.push(Value::Text(key.clone()));
        }
        Query::Date(field, bound) => {
            let column = match field {
                DateField::Created => "n.created_ms",
                DateField::Modified => "n.modified_ms",
            };
            let period =
                |spec: crate::dates::DateSpec| spec.period(clock.now_ms, clock.offset_minutes);
            let (from, to) = match *bound {
                DateBound::Within(spec) => {
                    let (a, b) = period(spec);
                    (Some(a), Some(b))
                }
                // `7d` and the like name a moment (7 days ago): `>` and `<`
                // compare with it, whether inclusive or not.
                DateBound::After(spec, inclusive) => {
                    let (a, b) = period(spec);
                    let relative = matches!(spec, crate::dates::DateSpec::Ago(..));
                    (Some(if inclusive || relative { a } else { b }), None)
                }
                DateBound::Before(spec, inclusive) => {
                    let (a, b) = period(spec);
                    let relative = matches!(spec, crate::dates::DateSpec::Ago(..));
                    (None, Some(if inclusive && !relative { b } else { a }))
                }
                DateBound::Between(first, last) => {
                    (first.map(|s| period(s).0), last.map(|s| period(s).1))
                }
            };
            let mut parts = Vec::new();
            if let Some(from) = from {
                parts.push(format!("{column} >= ?"));
                params.push(Value::Integer(from));
            }
            if let Some(to) = to {
                parts.push(format!("{column} < ?"));
                params.push(Value::Integer(to));
            }
            sql.push('(');
            sql.push_str(&parts.join(" AND "));
            sql.push(')');
        }
        Query::Has(feature) => sql.push_str(match feature {
            Feature::Image => "n.has_image = 1",
            Feature::Task => "n.has_task = 1",
            Feature::Link => "n.has_link = 1",
        }),
        Query::Is(Flag::Pinned) => sql.push_str("n.pinned = 1"),
        Query::Is(Flag::Untagged) => {
            sql.push_str("NOT EXISTS (SELECT 1 FROM note_tags t WHERE t.note = n.rowid)");
        }
        Query::InTrash => sql.push_str("n.state = 'trash'"),
        Query::Not(inner) => {
            sql.push_str("NOT (");
            compile(inner, clock, sql, params);
            sql.push(')');
        }
        Query::And(parts) | Query::Or(parts) => {
            let join = if matches!(query, Query::And(_)) {
                " AND "
            } else {
                " OR "
            };
            sql.push('(');
            for (i, part) in parts.iter().enumerate() {
                if i > 0 {
                    sql.push_str(join);
                }
                compile(part, clock, sql, params);
            }
            sql.push(')');
        }
    }
}

/// Splits a snippet at the hit markers.
fn snippet_parts(snippet: &str) -> Vec<SnippetPart> {
    let mut parts = Vec::new();
    let mut hit = false;
    let mut text = String::new();
    for c in snippet.chars() {
        if c == HIT_START || c == HIT_END {
            if !text.is_empty() {
                parts.push(SnippetPart {
                    text: std::mem::take(&mut text),
                    hit,
                });
            }
            hit = c == HIT_START;
        } else {
            text.push(if c == '\n' { ' ' } else { c });
        }
    }
    if !text.is_empty() {
        parts.push(SnippetPart { text, hit });
    }
    parts
}

impl Index {
    /// The notes matching `query` (see [`Query::parse`]), best first, at most
    /// [`MAX_HITS`]; none for an empty query.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn search(&self, query: &str, offset_minutes: i32) -> Result<Vec<SearchHit>> {
        let now = i64::try_from(now_ms()).unwrap_or(i64::MAX);
        self.search_at(query, now, offset_minutes)
    }

    /// [`Index::search`] at the moment `now_ms`.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn search_at(
        &self,
        query: &str,
        now_ms: i64,
        offset_minutes: i32,
    ) -> Result<Vec<SearchHit>> {
        let Some(query) = Query::parse(query) else {
            return Ok(Vec::new());
        };
        let clock = Clock {
            now_ms,
            offset_minutes: i64::from(offset_minutes),
        };
        let mut condition = String::new();
        let mut params = Vec::new();
        compile(&query, clock, &mut condition, &mut params);
        // Without `in:trash` the trash is not searched.
        let scope = if query.mentions_trash() {
            ""
        } else {
            "n.state = 'active' AND "
        };

        let rank_terms: Vec<String> = query
            .positive_terms()
            .into_iter()
            .filter_map(fts_term)
            .collect();
        let rank_query = (!rank_terms.is_empty()).then(|| rank_terms.join(" OR "));
        // The ranking is computed once (MATERIALIZED), not per note; snippets
        // only for the hits returned, in a second query that goes through the
        // matches once: `+rowid` keeps the rowid list away from FTS5, which
        // would otherwise run the MATCH again for every rowid in it.
        let sql = if let Some(rank_query) = &rank_query {
            params.insert(0, Value::Text(rank_query.clone()));
            format!(
                "WITH r AS MATERIALIZED (
                     SELECT rowid, bm25(notes_fts, 5.0, 1.0) AS rank
                     FROM notes_fts WHERE notes_fts MATCH ?)
                 SELECT n.rowid, n.path, n.title, n.mtime_ms, n.state FROM notes n
                 LEFT JOIN r ON r.rowid = n.rowid
                 WHERE {scope}{condition}
                 ORDER BY r.rank IS NULL, r.rank, n.mtime_ms DESC LIMIT {MAX_HITS}"
            )
        } else {
            format!(
                "SELECT n.rowid, n.path, n.title, n.mtime_ms, n.state FROM notes n
                 WHERE {scope}{condition}
                 ORDER BY n.mtime_ms DESC LIMIT {MAX_HITS}"
            )
        };
        let mut statement = self.db().prepare(&sql)?;
        let prefix = format!("{NOTES_DIR}/");
        let rows = statement.query_map(params_from_iter(&params), |row| {
            let path: String = row.get(1)?;
            let in_trash = row.get::<_, String>(4)? == "trash";
            Ok((
                row.get::<_, i64>(0)?,
                SearchHit {
                    path: path.strip_prefix(&prefix).unwrap_or(&path).to_owned(),
                    title: row.get(2)?,
                    modified_ms: u64::try_from(row.get::<_, i64>(3)?).unwrap_or(0),
                    snippet: Vec::new(),
                    in_trash,
                },
            ))
        })?;
        let mut hits: Vec<(i64, SearchHit)> = rows.collect::<rusqlite::Result<_>>()?;
        if let Some(rank_query) = rank_query
            && !hits.is_empty()
        {
            let marks = vec!["?"; hits.len()].join(", ");
            let mut snippets = self.db().prepare(&format!(
                "SELECT rowid, snippet(notes_fts, -1, char(2), char(3), '…', 16) FROM notes_fts
                 WHERE notes_fts MATCH ? AND +rowid IN ({marks})"
            ))?;
            let args = std::iter::once(Value::Text(rank_query))
                .chain(hits.iter().map(|(rowid, _)| Value::Integer(*rowid)));
            let found: std::collections::HashMap<i64, String> = snippets
                .query_map(params_from_iter(args), |row| Ok((row.get(0)?, row.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?;
            for (rowid, hit) in &mut hits {
                if let Some(snippet) = found.get(rowid) {
                    hit.snippet = snippet_parts(snippet);
                }
            }
        }
        Ok(hits.into_iter().map(|(_, hit)| hit).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::{FileStamp, NoteState, analyze};

    fn index(notes: &[(&str, &str, u64)]) -> Index {
        let mut index = Index::in_memory().unwrap();
        for (path, text, mtime_ms) in notes {
            let state = if path.starts_with(".notesapp") {
                NoteState::Trash
            } else {
                NoteState::Active
            };
            let stamp = FileStamp {
                mtime_ms: *mtime_ms,
                size: 1,
                hash: (*path).to_owned(),
            };
            index
                .put(path, state, &stamp, &analyze(path, text))
                .unwrap();
        }
        index
    }

    fn sample() -> Index {
        index(&[
            (
                "notes/tuner.md",
                "# Antennentuner\nGehäuse bohren, Lötzinn kaufen. #radio/qrp",
                3,
            ),
            (
                "notes/test.md",
                "# Antennentest 40 m\nDipol auf dem Dach. #radio",
                2,
            ),
            (
                "notes/chor.md",
                "# Chorprobe\nDienstag, Antenne der Kirche? #chor",
                1,
            ),
            ("notes/einkauf.md", "# Einkauf\nMilch, Brot, Lötzinn", 4),
            (".notesapp/trash/X.md", "# Antenne im Müll #radio", 5),
        ])
    }

    fn paths(index: &Index, query: &str) -> Vec<String> {
        index
            .search(query, 0)
            .unwrap()
            .into_iter()
            .map(|h| h.path)
            .collect()
    }

    #[test]
    fn words_match_as_prefixes_without_case_or_diacritics() {
        let index = sample();
        let mut found = paths(&index, "ANTENNE");
        found.sort();
        assert_eq!(found, ["chor.md", "test.md", "tuner.md"]);
        assert_eq!(paths(&index, "gehause"), ["tuner.md"]);
        let mut lot = paths(&index, "lotzinn");
        lot.sort();
        assert_eq!(lot, ["einkauf.md", "tuner.md"]);
        assert!(
            paths(&index, "müll").is_empty(),
            "the trash is not searched"
        );
    }

    #[test]
    fn titles_rank_first() {
        let index = sample();
        let found = paths(&index, "antenne");
        assert_eq!(
            found.last().map(String::as_str),
            Some("chor.md"),
            "{found:?}"
        );
    }

    #[test]
    fn phrases_tags_exclusions_and_or() {
        let index = sample();
        assert_eq!(paths(&index, "\"dipol auf dem dach\""), ["test.md"]);
        assert!(paths(&index, "\"dach dipol\"").is_empty());
        let mut radio = paths(&index, "#radio");
        radio.sort();
        assert_eq!(
            radio,
            ["test.md", "tuner.md"],
            "#radio also finds #radio/qrp"
        );
        assert_eq!(paths(&index, "#radio/qrp"), ["tuner.md"]);
        assert_eq!(paths(&index, "antenne -#radio"), ["chor.md"]);
        assert_eq!(
            paths(&index, "lötzinn -(#radio OR brot)"),
            Vec::<String>::new()
        );
        let mut either = paths(&index, "dipol OR chorprobe");
        either.sort();
        assert_eq!(either, ["chor.md", "test.md"]);
        // Only tags or exclusions: newest first.
        assert_eq!(paths(&index, "-#radio"), ["einkauf.md", "chor.md"]);
    }

    #[test]
    fn fts_syntax_in_the_query_is_just_text() {
        let index = sample();
        for query in [
            "NEAR(a b)",
            "\"",
            "title:antenne",
            "a*b",
            "^x",
            "(",
            "-",
            "\"\"\"",
            "'; DROP TABLE notes; --",
        ] {
            assert!(index.search(query, 0).is_ok(), "{query}");
        }
        assert_eq!(index.len().unwrap(), 5);
    }

    #[test]
    fn snippets_mark_the_hits() {
        let index = sample();
        let hit = &index.search("dipol", 0).unwrap()[0];
        let marked: Vec<_> = hit
            .snippet
            .iter()
            .filter(|p| p.hit)
            .map(|p| p.text.as_str())
            .collect();
        assert_eq!(marked, ["Dipol"]);
        let all: String = hit.snippet.iter().map(|p| p.text.as_str()).collect();
        assert!(all.contains("Dipol auf dem Dach"), "{all}");
        assert!(index.search("#chor", 0).unwrap()[0].snippet.is_empty());
        assert!(index.search("", 0).unwrap().is_empty());
    }

    fn found(index: &Index, query: &str, now: i64, offset: i32) -> Vec<String> {
        let mut paths: Vec<String> = index
            .search_at(query, now, offset)
            .unwrap()
            .into_iter()
            .map(|h| h.path)
            .collect();
        paths.sort();
        paths
    }

    #[test]
    fn dates_flags_and_the_trash() {
        let ms = |s: &str| crate::dates::parse_rfc3339(s).unwrap();
        let fm = |created: &str, modified: &str, extra: &str| {
            format!("---\ncreated: {created}\nmodified: {modified}\n{extra}---\n")
        };
        let late = fm(
            "2025-12-31T23:30:00+01:00",
            "2026-10-22T09:00:00+02:00",
            "pinned: true\n",
        );
        let early = fm("2026-01-01T00:30:00+01:00", "2026-10-01T12:00:00+02:00", "");
        let notes = [
            (
                "notes/silvester.md",
                format!("{late}# Silvester\n- [ ] Sekt kaufen [[Einkauf]]"),
            ),
            (
                "notes/neujahr.md",
                format!("{early}# Neujahr\n![Feuerwerk](f.png) #fest"),
            ),
            // No front matter: the file time counts (2026-10-23 08:00 UTC).
            (
                "notes/plain.md",
                "# Ohne Kopf\n[Web](https://example.org)".to_owned(),
            ),
            (".notesapp/trash/X.md", format!("{early}# Weg #fest")),
        ];
        let mut index = Index::in_memory().unwrap();
        for (path, text) in &notes {
            let state = if path.starts_with(".notesapp") {
                NoteState::Trash
            } else {
                NoteState::Active
            };
            let stamp = FileStamp {
                mtime_ms: u64::try_from(ms("2026-10-23T08:00:00Z")).unwrap(),
                size: 1,
                hash: (*path).to_owned(),
            };
            index
                .put(path, state, &stamp, &analyze(path, text))
                .unwrap();
        }
        let now = ms("2026-10-23T10:00:00Z");

        // Days and years begin at local midnight.
        assert_eq!(found(&index, "created:2025", now, 60), ["silvester.md"]);
        assert_eq!(found(&index, "created:2026-01-01", now, 60), ["neujahr.md"]);
        assert_eq!(
            found(&index, "created:2025-12-31", now, 0),
            ["neujahr.md", "silvester.md"],
            "in UTC both are on Dec 31"
        );
        assert_eq!(
            found(&index, "created:>=2026", now, 60),
            ["neujahr.md", "plain.md"]
        );
        assert_eq!(found(&index, "created:<2026", now, 60), ["silvester.md"]);
        assert_eq!(
            found(&index, "modified:2026-10-01..2026-10-21", now, 120),
            ["neujahr.md"]
        );
        assert_eq!(
            found(&index, "modified:..2026-09", now, 120),
            Vec::<String>::new()
        );
        assert_eq!(found(&index, "modified:today", now, 120), ["plain.md"]);
        assert_eq!(
            found(&index, "modified:yesterday", now, 120),
            ["silvester.md"]
        );
        assert_eq!(
            found(&index, "modified:7d", now, 120),
            ["plain.md", "silvester.md"]
        );
        assert_eq!(found(&index, "modified:<7d", now, 120), ["neujahr.md"]);
        assert_eq!(
            found(&index, "modified:>7d", now, 120),
            ["plain.md", "silvester.md"]
        );

        assert_eq!(found(&index, "has:task", now, 0), ["silvester.md"]);
        assert_eq!(found(&index, "has:image", now, 0), ["neujahr.md"]);
        assert_eq!(
            found(&index, "has:link", now, 0),
            ["plain.md", "silvester.md"]
        );
        assert_eq!(found(&index, "is:pinned", now, 0), ["silvester.md"]);
        assert_eq!(
            found(&index, "is:untagged", now, 0),
            ["plain.md", "silvester.md"]
        );
        assert_eq!(found(&index, "tag:fest", now, 0), ["neujahr.md"]);

        // The trash only when asked, and marked.
        let trashed = index.search_at("#fest in:trash", now, 0).unwrap();
        assert_eq!(trashed.len(), 1);
        assert!(trashed[0].in_trash);
        assert_eq!(trashed[0].path, ".notesapp/trash/X.md");
        assert_eq!(found(&index, "#fest -in:trash", now, 0), ["neujahr.md"]);
    }
}
