// SPDX-License-Identifier: Apache-2.0
//! The search index (ADR-0009): SQLite with FTS5, a per-device cache that can
//! always be rebuilt from the vault.
//!
//! [`analyze`] turns a note into what the index stores: title, `id` and dates
//! from the front matter (format.md §4), plain text without Markdown syntax for
//! full-text search, tags (§5.3, with their hierarchy ancestors), note links
//! (`[[Title]]` and relative `.md` links), linked files (attachments, §5.5)
//! and chord names; `chordpro` blocks contribute their lyrics and song
//! details instead of their source (§5.7 rule 8).
//! [`Index`] keeps that per note, keyed by its path below the vault root
//! (`notes/…` or `.notesapp/trash/…`).
//!
//! The file lives outside the vault (the app passes a path in its cache
//! folder). A file with another schema version, or one SQLite cannot read, is
//! deleted and created afresh: it holds nothing that is not in the vault.

use std::collections::HashMap;
use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, parse_document};
use rusqlite::{Connection, OptionalExtension as _, params};

use crate::attachments::percent_decode;
use crate::chordpro::Song;
use crate::dates::parse_rfc3339;
use crate::front_matter;
use crate::links::resolve_from;
use crate::stamp::is_ulid;
use crate::tags::{Tag, extract_tags, parse_options};
use crate::vault::{title_of, title_sort_key};

/// Version of the tables below; a file with another version is rebuilt.
pub const SCHEMA_VERSION: i64 = 4;

const SCHEMA: &str = "
CREATE TABLE notes (
    rowid INTEGER PRIMARY KEY,
    path TEXT NOT NULL UNIQUE,      -- below the vault root: notes/… or .notesapp/trash/…
    state TEXT NOT NULL,            -- 'active' or 'trash'
    id TEXT,                        -- front matter id (format.md §4), upper case
    title TEXT NOT NULL,
    title_key TEXT NOT NULL,        -- the title without case and accents, for sorting
    created TEXT,
    modified TEXT,
    mtime_ms INTEGER NOT NULL,      -- file modification time, for the sync
    size INTEGER NOT NULL,
    hash TEXT NOT NULL,             -- SHA-256 of the file
    created_ms INTEGER NOT NULL,    -- front matter `created`, else the file time
    modified_ms INTEGER NOT NULL,   -- front matter `modified`, else the file time
    pinned INTEGER NOT NULL,
    has_task INTEGER NOT NULL,
    has_image INTEGER NOT NULL,
    has_link INTEGER NOT NULL
);
CREATE INDEX notes_id ON notes(id);
-- rowid = notes.rowid; stores the text, so results can show snippets
CREATE VIRTUAL TABLE notes_fts USING fts5(
    title, body, tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TABLE note_tags (
    note INTEGER NOT NULL REFERENCES notes(rowid) ON DELETE CASCADE,
    key TEXT NOT NULL,              -- normalized (format.md §5.3)
    name TEXT,                      -- as written; NULL for an ancestor (#a of #a/b)
    PRIMARY KEY (note, key)
) WITHOUT ROWID;
CREATE INDEX note_tags_key ON note_tags(key);
CREATE TABLE note_links (
    note INTEGER NOT NULL REFERENCES notes(rowid) ON DELETE CASCADE,
    kind TEXT NOT NULL,             -- 'title' ([[Title]]) or 'path' (relative .md link)
    target TEXT NOT NULL            -- the title, or the path below the vault root
);
CREATE INDEX note_links_target ON note_links(kind, target);
CREATE TABLE note_files (
    note INTEGER NOT NULL REFERENCES notes(rowid) ON DELETE CASCADE,
    path TEXT NOT NULL              -- linked file below the vault root, e.g. attachments/…
);
CREATE INDEX note_files_path ON note_files(path);
CREATE TABLE note_chords (
    note INTEGER NOT NULL REFERENCES notes(rowid) ON DELETE CASCADE,
    chord TEXT NOT NULL,            -- normalized: chordpro::normalize_chord
    PRIMARY KEY (note, chord)
) WITHOUT ROWID;
CREATE INDEX note_chords_chord ON note_chords(chord);
";

/// Errors of the index. All of them can be cured by rebuilding it.
#[derive(Debug)]
pub enum IndexError {
    /// SQLite reported an error.
    Sqlite(rusqlite::Error),
    /// The index file or its folder could not be created or removed.
    Io(io::Error),
}

impl fmt::Display for IndexError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Sqlite(e) => write!(f, "search index: {e}"),
            Self::Io(e) => write!(f, "search index file: {e}"),
        }
    }
}

impl std::error::Error for IndexError {}

impl From<rusqlite::Error> for IndexError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Sqlite(e)
    }
}

impl From<io::Error> for IndexError {
    fn from(e: io::Error) -> Self {
        Self::Io(e)
    }
}

/// Result type of this module.
pub type Result<T> = std::result::Result<T, IndexError>;

/// Where a note is: in `notes/` or in the trash (format.md §8).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteState {
    /// In `notes/`.
    Active,
    /// In `.notesapp/trash/`; still searchable (F-15).
    Trash,
}

impl NoteState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Active => "active",
            Self::Trash => "trash",
        }
    }
}

/// What the sync compares to find changed files: modification time and size
/// first, the content hash only when they differ.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileStamp {
    /// Modification time, milliseconds since the Unix epoch.
    pub mtime_ms: u64,
    /// Size in bytes.
    pub size: u64,
    /// SHA-256 of the content as hex.
    pub hash: String,
}

/// A link from a note to another note.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NoteLink {
    /// `[[Title]]` or `[[Title|label]]` (format.md §5.4): the title.
    Title(String),
    /// A relative Markdown link to a `.md` file: its path below the vault root.
    Path(String),
}

/// What the index stores about a note, see [`analyze`].
#[derive(Debug, Clone, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)] // independent facts, one index column each
pub struct NoteFacts {
    /// The front matter `id`, upper case, if valid.
    pub id: Option<String>,
    /// Title per format.md §5.2.
    pub title: String,
    /// Front matter `created`, as written.
    pub created: Option<String>,
    /// Front matter `modified`, as written.
    pub modified: Option<String>,
    /// The visible text, without Markdown syntax, front matter or link
    /// destinations; code is included, `chordpro` blocks as their
    /// [`Song::index_text`].
    pub body: String,
    /// Tags in order of first appearance (format.md §5.3).
    pub tags: Vec<Tag>,
    /// Links to other notes.
    pub links: Vec<NoteLink>,
    /// Other linked files below the vault root (attachments, images).
    pub files: Vec<String>,
    /// Chord names in `chordpro` blocks, each once ([`Song::chords`]).
    pub chords: Vec<String>,
    /// The note has a task list item.
    pub has_task: bool,
    /// The note has an image.
    pub has_image: bool,
    /// The note has a link (to a note, a file or the web).
    pub has_link: bool,
    /// Front matter `pinned: true` (format.md §4).
    pub pinned: bool,
}

/// Analyzes the note at `path` (below the vault root, e.g. `notes/a/b.md`)
/// with the content `text`.
#[must_use]
pub fn analyze(path: &str, text: &str) -> NoteFacts {
    let mut dir: Vec<String> = path.split('/').map(str::to_owned).collect();
    let name = dir.pop().unwrap_or_default();
    let stem = name.get(..name.len().saturating_sub(3)).unwrap_or(&name);

    let arena = Arena::new();
    let root = parse_document(&arena, text, &parse_options());
    let mut walk = Walk {
        dir: &dir,
        body: String::new(),
        links: Vec::new(),
        files: Vec::new(),
        chords: Vec::new(),
        has_task: false,
        has_image: false,
        has_link: false,
    };
    walk.node(root);

    let field = |key| front_matter::get(text, key).filter(|v| !v.is_empty());
    NoteFacts {
        id: field("id")
            .filter(|id| is_ulid(id))
            .map(|id| id.to_ascii_uppercase()),
        title: title_of(text, stem),
        created: field("created"),
        modified: field("modified"),
        body: walk.body.trim().to_owned(),
        tags: extract_tags(text),
        links: walk.links,
        files: walk.files,
        chords: walk.chords,
        has_task: walk.has_task,
        has_image: walk.has_image,
        has_link: walk.has_link,
        pinned: field("pinned").is_some_and(|v| v.eq_ignore_ascii_case("true")),
    }
}

struct Walk<'d> {
    dir: &'d [String],
    body: String,
    links: Vec<NoteLink>,
    files: Vec<String>,
    chords: Vec<String>,
    has_task: bool,
    has_image: bool,
    has_link: bool,
}

impl Walk<'_> {
    fn node<'a>(&mut self, node: &'a AstNode<'a>) {
        let value = &node.data.borrow().value;
        match value {
            NodeValue::FrontMatter(_) | NodeValue::HtmlBlock(_) | NodeValue::HtmlInline(_) => {}
            NodeValue::Text(text) => self.body.push_str(text),
            NodeValue::Code(code) => self.body.push_str(&code.literal),
            NodeValue::Math(math) => self.body.push_str(&math.literal),
            NodeValue::CodeBlock(block) => {
                let lang = block.info.split_whitespace().next().unwrap_or_default();
                if lang.eq_ignore_ascii_case("chordpro") {
                    let song = Song::parse(&block.literal);
                    self.body.push_str(&song.index_text());
                    for chord in song.chords() {
                        if !self.chords.contains(&chord) {
                            self.chords.push(chord);
                        }
                    }
                } else {
                    self.body.push_str(&block.literal);
                    self.body.push('\n');
                }
            }
            NodeValue::SoftBreak | NodeValue::LineBreak => self.body.push('\n'),
            NodeValue::Link(link) | NodeValue::Image(link) => {
                if matches!(value, NodeValue::Image(_)) {
                    self.has_image = true;
                } else {
                    self.has_link = true;
                }
                self.destination(&link.url);
                self.children(node);
            }
            NodeValue::WikiLink(link) => {
                self.has_link = true;
                let title = link.url.trim();
                if !title.is_empty() {
                    self.links.push(NoteLink::Title(title.to_owned()));
                }
                self.children(node);
            }
            NodeValue::TaskItem(_) => {
                self.has_task = true;
                self.children(node);
                self.body.push('\n');
            }
            _ if value.block() => {
                self.children(node);
                if !self.body.ends_with('\n') {
                    self.body.push('\n');
                }
            }
            _ => self.children(node),
        }
    }

    fn children<'a>(&mut self, node: &'a AstNode<'a>) {
        for child in node.children() {
            self.node(child);
        }
    }

    /// Records a relative link destination that stays inside the vault.
    fn destination(&mut self, url: &str) {
        let path = url.split(['#', '?']).next().unwrap_or_default();
        let Some(target) = percent_decode(path).and_then(|p| resolve_from(self.dir, &p)) else {
            return; // a URL, absolute, outside the vault or broken
        };
        let target = target.join("/");
        if target.to_ascii_lowercase().ends_with(".md") {
            let link = NoteLink::Path(target);
            if !self.links.contains(&link) {
                self.links.push(link);
            }
        } else if !self.files.contains(&target) {
            self.files.push(target);
        }
    }
}

/// The search index of one vault.
#[derive(Debug)]
pub struct Index {
    db: Connection,
}

impl Index {
    /// Opens the index file at `file`, creating it and its folder if needed. A
    /// file of another schema version, or one SQLite cannot use, is replaced
    /// by an empty index.
    ///
    /// # Errors
    /// If the file can be neither opened nor recreated.
    pub fn open(file: &Path) -> Result<Self> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        match Self::open_existing(file) {
            Ok(Some(index)) => return Ok(index),
            Ok(None) => {}
            Err(e) => eprintln!("search index {} is rebuilt: {e}", file.display()),
        }
        remove_database(file)?;
        let index = Self::prepare(Connection::open(file)?)?;
        index.create()?;
        Ok(index)
    }

    /// An index in memory, for tests and tools.
    ///
    /// # Errors
    /// If SQLite cannot create it.
    pub fn in_memory() -> Result<Self> {
        let index = Self::prepare(Connection::open_in_memory()?)?;
        index.create()?;
        Ok(index)
    }

    /// The index at `file` if it is usable as it is; `None` if it is empty or
    /// of another schema version.
    fn open_existing(file: &Path) -> Result<Option<Self>> {
        if !file.exists() {
            return Ok(None);
        }
        let index = Self::prepare(Connection::open(file)?)?;
        let version: i64 = index
            .db
            .query_row("PRAGMA user_version", [], |row| row.get(0))?;
        if version != SCHEMA_VERSION {
            return Ok(None);
        }
        // Reads a table, so a file that is not a database fails here.
        index
            .db
            .query_row("SELECT count(*) FROM notes", [], |row| row.get::<_, i64>(0))?;
        Ok(Some(index))
    }

    fn prepare(db: Connection) -> Result<Self> {
        db.execute_batch(
            "PRAGMA journal_mode = WAL; PRAGMA synchronous = NORMAL; PRAGMA foreign_keys = ON;",
        )?;
        Ok(Self { db })
    }

    fn create(&self) -> Result<()> {
        self.db.execute_batch(&format!(
            "BEGIN; {SCHEMA} PRAGMA user_version = {SCHEMA_VERSION}; COMMIT;"
        ))?;
        Ok(())
    }

    /// Stores `facts` for the note at `path` (below the vault root), replacing
    /// what was stored for that path.
    ///
    /// # Errors
    /// SQLite errors; the index is unchanged then.
    pub fn put(
        &mut self,
        path: &str,
        state: NoteState,
        stamp: &FileStamp,
        facts: &NoteFacts,
    ) -> Result<()> {
        let tx = self.db.transaction()?;
        let old: Option<i64> = tx
            .query_row("SELECT rowid FROM notes WHERE path = ?1", [path], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(rowid) = old {
            tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", [rowid])?;
            tx.execute("DELETE FROM notes WHERE rowid = ?1", [rowid])?;
        }
        let mtime = i64::try_from(stamp.mtime_ms).unwrap_or(i64::MAX);
        let time_of =
            |field: &Option<String>| field.as_deref().and_then(parse_rfc3339).unwrap_or(mtime);
        tx.execute(
            "INSERT INTO notes (path, state, id, title, title_key, created, modified, mtime_ms,
                                size, hash, created_ms, modified_ms, pinned, has_task,
                                has_image, has_link)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16)",
            params![
                path,
                state.as_str(),
                facts.id,
                facts.title,
                title_sort_key(&facts.title),
                facts.created,
                facts.modified,
                i64::try_from(stamp.mtime_ms).unwrap_or(i64::MAX),
                i64::try_from(stamp.size).unwrap_or(i64::MAX),
                stamp.hash,
                time_of(&facts.created),
                time_of(&facts.modified),
                facts.pinned,
                facts.has_task,
                facts.has_image,
                facts.has_link,
            ],
        )?;
        let rowid = tx.last_insert_rowid();
        tx.execute(
            "INSERT INTO notes_fts (rowid, title, body) VALUES (?1, ?2, ?3)",
            params![rowid, facts.title, facts.body],
        )?;
        {
            let mut tag = tx.prepare(
                "INSERT INTO note_tags (note, key, name) VALUES (?1, ?2, ?3)
                 ON CONFLICT (note, key) DO UPDATE SET name = coalesce(name, excluded.name)",
            )?;
            for t in &facts.tags {
                for key in t.self_and_ancestor_keys() {
                    let name = (key == t.key).then_some(t.name.as_str());
                    tag.execute(params![rowid, key, name])?;
                }
            }
            let mut link =
                tx.prepare("INSERT INTO note_links (note, kind, target) VALUES (?1, ?2, ?3)")?;
            for l in &facts.links {
                let (kind, target) = match l {
                    NoteLink::Title(t) => ("title", t),
                    NoteLink::Path(p) => ("path", p),
                };
                link.execute(params![rowid, kind, target])?;
            }
            let mut file = tx.prepare("INSERT INTO note_files (note, path) VALUES (?1, ?2)")?;
            for f in &facts.files {
                file.execute(params![rowid, f])?;
            }
            let mut chord = tx.prepare("INSERT INTO note_chords (note, chord) VALUES (?1, ?2)")?;
            for c in &facts.chords {
                chord.execute(params![rowid, c])?;
            }
        }
        tx.commit()?;
        Ok(())
    }

    /// Removes the note at `path`; `false` if it was not in the index.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn remove(&mut self, path: &str) -> Result<bool> {
        let tx = self.db.transaction()?;
        let old: Option<i64> = tx
            .query_row("SELECT rowid FROM notes WHERE path = ?1", [path], |row| {
                row.get(0)
            })
            .optional()?;
        if let Some(rowid) = old {
            tx.execute("DELETE FROM notes_fts WHERE rowid = ?1", [rowid])?;
            tx.execute("DELETE FROM notes WHERE rowid = ?1", [rowid])?;
        }
        tx.commit()?;
        Ok(old.is_some())
    }

    /// The stamps of all indexed files by path, for the sync.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn stamps(&self) -> Result<HashMap<String, FileStamp>> {
        let mut query = self
            .db
            .prepare("SELECT path, mtime_ms, size, hash FROM notes")?;
        let rows = query.query_map([], |row| {
            Ok((
                row.get::<_, String>(0)?,
                FileStamp {
                    mtime_ms: u64::try_from(row.get::<_, i64>(1)?).unwrap_or(0),
                    size: u64::try_from(row.get::<_, i64>(2)?).unwrap_or(0),
                    hash: row.get(3)?,
                },
            ))
        })?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// Updates the stamp of the note at `path` only: its file was touched
    /// but its content is unchanged. `false` if it is not in the index.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn set_stamp(&mut self, path: &str, stamp: &FileStamp) -> Result<bool> {
        let changed = self.db.execute(
            "UPDATE notes SET mtime_ms = ?2, size = ?3, hash = ?4 WHERE path = ?1",
            params![
                path,
                i64::try_from(stamp.mtime_ms).unwrap_or(i64::MAX),
                i64::try_from(stamp.size).unwrap_or(i64::MAX),
                stamp.hash
            ],
        )?;
        Ok(changed > 0)
    }

    /// Number of indexed notes, the trash included.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn len(&self) -> Result<usize> {
        let n: i64 = self
            .db
            .query_row("SELECT count(*) FROM notes", [], |row| row.get(0))?;
        Ok(usize::try_from(n).unwrap_or(0))
    }

    /// `true` if no note is indexed.
    ///
    /// # Errors
    /// SQLite errors.
    pub fn is_empty(&self) -> Result<bool> {
        Ok(self.len()? == 0)
    }

    /// Paths of the notes (not in the trash) whose front matter has `id`,
    /// sorted; more than one means copies (format.md §4).
    ///
    /// # Errors
    /// SQLite errors.
    pub fn paths_with_id(&self, id: &str) -> Result<Vec<String>> {
        let mut query = self
            .db
            .prepare("SELECT path FROM notes WHERE id = ?1 AND state = 'active' ORDER BY path")?;
        let rows = query.query_map([id.to_ascii_uppercase()], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<_>>()?)
    }

    /// The connection, for the queries of later modules in this crate.
    #[allow(dead_code)] // used by the tag and search queries (M2)
    pub(crate) fn db(&self) -> &Connection {
        &self.db
    }
}

/// Removes a database file with its write-ahead log and shared-memory file.
fn remove_database(file: &Path) -> io::Result<()> {
    for suffix in ["", "-wal", "-shm"] {
        let mut name = file.as_os_str().to_owned();
        name.push(suffix);
        match fs::remove_file(PathBuf::from(name)) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const ID: &str = "01JA7Q5Y0Z8H3K2M6N4P9R1S2T";

    fn stamp(hash: &str) -> FileStamp {
        FileStamp {
            mtime_ms: 1_000,
            size: 10,
            hash: hash.to_owned(),
        }
    }

    fn matches(index: &Index, query: &str) -> Vec<String> {
        let mut q = index
            .db()
            .prepare(
                "SELECT n.path FROM notes_fts f JOIN notes n ON n.rowid = f.rowid
                 WHERE notes_fts MATCH ?1 ORDER BY n.path",
            )
            .unwrap();
        q.query_map([query], |row| row.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap()
    }

    #[test]
    fn analyzes_a_note() {
        let text = format!(
            "---\nid: {}\ncreated: 2026-10-13T21:10:00Z\nmodified: 2026-10-14T08:00:00Z\n---\n\
             # Antennentuner #radio/qrp\n\nLötzinn *bestellen* und `code` lesen. Siehe [[Morserino]] \
             und [Tuner](../Projekte/tuner.md#bau).\n\n- [ ] Gehäuse #Radio\n\n\
             ![Plan](../../attachments/ab/x.png) [Web](https://example.org) [weg](../../../x.png)\n\n\
             ```sh\nmake #nocode\n```\n<div>html #nohtml</div>\n",
            ID.to_lowercase()
        );
        let facts = analyze("notes/Projekte/a.md", &text);
        assert_eq!(facts.id.as_deref(), Some(ID));
        assert_eq!(facts.title, "Antennentuner #radio/qrp");
        assert_eq!(facts.created.as_deref(), Some("2026-10-13T21:10:00Z"));
        assert!(
            facts.body.contains("Lötzinn bestellen und code lesen."),
            "{}",
            facts.body
        );
        assert!(
            facts.body.contains("make #nocode"),
            "code blocks are searchable"
        );
        assert!(!facts.body.contains("example.org"), "no link destinations");
        assert!(!facts.body.contains("nohtml"), "no raw HTML");
        assert!(!facts.body.contains("id:"), "no front matter");
        let tags: Vec<_> = facts.tags.iter().map(|t| t.name.as_str()).collect();
        assert_eq!(tags, ["radio/qrp", "Radio"]);
        assert_eq!(
            facts.links,
            [
                NoteLink::Title("Morserino".into()),
                NoteLink::Path("notes/Projekte/tuner.md".into())
            ]
        );
        assert_eq!(facts.files, ["attachments/ab/x.png"]);
        assert!(facts.has_task && facts.has_image);

        let plain = analyze("notes/b.md", "just text");
        assert_eq!((plain.id, plain.title.as_str()), (None, "just text"));
        assert!(!plain.has_task && !plain.has_image);
    }

    #[test]
    fn chordpro_blocks_give_lyrics_and_chords() {
        let text = "# Lieder\n\n```chordpro notation=german\n{title: Abendlied}\n\
                    {key: G}\n# geheim\nDer [G]Mond ist aufge[D]gangen\n```\n\n\
                    ```ChordPro\n[Em]Zwei[G]tes\n```\n\n```text\n[Am]bleibt\n```\n";
        let facts = analyze("notes/a.md", text);
        assert_eq!(
            facts.body,
            "Lieder\nAbendlied\nDer Mond ist aufgegangen\nZweites\n[Am]bleibt"
        );
        assert_eq!(facts.chords, ["G", "D", "Em"]);

        let mut index = Index::in_memory().unwrap();
        index
            .put("notes/a.md", NoteState::Active, &stamp("h"), &facts)
            .unwrap();
        assert_eq!(matches(&index, "aufgegangen"), ["notes/a.md"]);
        assert!(matches(&index, "geheim").is_empty(), "no ChordPro comments");
        let chords: i64 = index
            .db()
            .query_row("SELECT count(*) FROM note_chords", [], |r| r.get(0))
            .unwrap();
        assert_eq!(chords, 3);
    }

    #[test]
    fn stores_replaces_and_removes_notes() {
        let mut index = Index::in_memory().unwrap();
        let a = analyze(
            "notes/a.md",
            &format!("---\nid: {ID}\n---\n# Äpfel #obst\nGrüne Äpfel\n"),
        );
        index
            .put("notes/a.md", NoteState::Active, &stamp("h1"), &a)
            .unwrap();
        index
            .put(
                "notes/b.md",
                NoteState::Active,
                &stamp("h2"),
                &analyze("notes/b.md", "# Birnen\n"),
            )
            .unwrap();
        assert_eq!(index.len().unwrap(), 2);

        // Diacritics are folded, prefixes match.
        assert_eq!(matches(&index, "apfel"), ["notes/a.md"]);
        assert_eq!(matches(&index, "grun*"), ["notes/a.md"]);
        assert_eq!(matches(&index, "title:birnen"), ["notes/b.md"]);

        // Replacing leaves no trace of the old text.
        let changed = analyze("notes/a.md", "# Kirschen #obst/rot\n");
        index
            .put("notes/a.md", NoteState::Active, &stamp("h3"), &changed)
            .unwrap();
        assert_eq!(index.len().unwrap(), 2);
        assert!(matches(&index, "apfel").is_empty());
        assert_eq!(matches(&index, "kirschen"), ["notes/a.md"]);
        assert_eq!(index.stamps().unwrap()["notes/a.md"], stamp("h3"));

        assert!(index.remove("notes/a.md").unwrap());
        assert!(!index.remove("notes/a.md").unwrap());
        assert!(matches(&index, "kirschen").is_empty());
        let tags: i64 = index
            .db()
            .query_row("SELECT count(*) FROM note_tags", [], |r| r.get(0))
            .unwrap();
        assert_eq!(tags, 0, "tags go with their note");
    }

    #[test]
    fn tags_count_for_their_ancestors() {
        let mut index = Index::in_memory().unwrap();
        let facts = analyze(
            "notes/a.md",
            "#Projekte/Tuner and #projekte/tuner/bau and #radio",
        );
        index
            .put("notes/a.md", NoteState::Active, &stamp("h"), &facts)
            .unwrap();
        let mut q = index
            .db()
            .prepare("SELECT key, name FROM note_tags ORDER BY key")
            .unwrap();
        let rows: Vec<(String, Option<String>)> = q
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(
            rows,
            [
                ("projekte".into(), None),
                ("projekte/tuner".into(), Some("Projekte/Tuner".into())),
                (
                    "projekte/tuner/bau".into(),
                    Some("projekte/tuner/bau".into())
                ),
                ("radio".into(), Some("radio".into())),
            ]
        );
    }

    #[test]
    fn finds_copies_with_the_same_id_but_not_in_the_trash() {
        let mut index = Index::in_memory().unwrap();
        let text = format!("---\nid: {ID}\n---\nx");
        for (path, state) in [
            ("notes/a.md", NoteState::Active),
            ("notes/sub/a 2.md", NoteState::Active),
            (".notesapp/trash/X.md", NoteState::Trash),
        ] {
            index
                .put(path, state, &stamp("h"), &analyze(path, &text))
                .unwrap();
        }
        assert_eq!(
            index.paths_with_id(&ID.to_lowercase()).unwrap(),
            ["notes/a.md", "notes/sub/a 2.md"]
        );
    }

    #[test]
    fn a_file_of_another_version_or_no_database_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("cache/index/vault.sqlite");
        {
            let mut index = Index::open(&file).unwrap();
            index
                .put(
                    "notes/a.md",
                    NoteState::Active,
                    &stamp("h"),
                    &analyze("notes/a.md", "x"),
                )
                .unwrap();
        }
        assert_eq!(
            Index::open(&file).unwrap().len().unwrap(),
            1,
            "kept across opens"
        );

        Connection::open(&file)
            .unwrap()
            .execute_batch("PRAGMA user_version = 999")
            .unwrap();
        assert!(Index::open(&file).unwrap().is_empty().unwrap());

        remove_database(&file).unwrap();
        fs::write(&file, b"this is not a database").unwrap();
        let index = Index::open(&file).unwrap();
        assert!(index.is_empty().unwrap());
    }
}
