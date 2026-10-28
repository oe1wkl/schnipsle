// SPDX-License-Identifier: Apache-2.0
//! Smart folders: saved searches in `.notesapp/smart-folders.yaml`
//! (format.md §9, ADR-0018).
//!
//! [`SmartFolders`] holds the file as YAML, so keys and entries this version
//! does not understand stay as they are when it is written again: unknown
//! keys, a `sort` it does not know, entries without `id`, `name` or `query`.
//! The order of `folders` is the order among folders with the same parent;
//! a `parent` that does not exist or would make a cycle counts as the top
//! level.
//!
//! The file is changed only through [`Vault::edit_smart_folders`]: it reads
//! the file, applies the edit and writes the result atomically, coordinated
//! and only if the file is still what it read (format.md §10). If it changed
//! in between, for example on another device, the edit is applied again to
//! the new content, so neither change is lost.

use std::fmt;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use yaml_rust2::yaml::Hash;
use yaml_rust2::{Yaml, YamlEmitter, YamlLoader};

use crate::layout::SMART_FOLDERS_FILE;
use crate::stamp::new_ulid;
use crate::vault::{Access, TempFile, Vault, VaultError, accessor_ran_twice, sha256_hex};

/// The `format` this version reads and writes.
pub const SMART_FOLDERS_FORMAT: i64 = 1;

/// How often an edit is applied again when the file keeps changing.
const ATTEMPTS: usize = 3;

/// The order of a smart folder's notes (format.md §9). Serialized as in the
/// file: `"modified-desc"`, `"modified-asc"`, `"created-desc"`, `"title-asc"`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum FolderSort {
    /// Most recently changed first; also for a missing or unknown `sort`.
    #[default]
    ModifiedDesc,
    /// Least recently changed first.
    ModifiedAsc,
    /// Most recently created first.
    CreatedDesc,
    /// By title, A to Z, ignoring case and accents.
    TitleAsc,
}

impl FolderSort {
    const ALL: [Self; 4] = [
        Self::ModifiedDesc,
        Self::ModifiedAsc,
        Self::CreatedDesc,
        Self::TitleAsc,
    ];

    /// The value in the file.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ModifiedDesc => "modified-desc",
            Self::ModifiedAsc => "modified-asc",
            Self::CreatedDesc => "created-desc",
            Self::TitleAsc => "title-asc",
        }
    }

    fn parse(value: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// Errors of smart folders.
#[derive(Debug)]
pub enum SmartFolderError {
    /// Reading or writing the file failed, or the vault is read-only.
    Vault(VaultError),
    /// The file is not YAML of the expected shape; it is left as it is.
    Damaged(String),
    /// The file has a newer `format`; it can be read but not changed.
    NewerFormat(i64),
    /// No smart folder has this id.
    UnknownFolder(String),
    /// The folder cannot go there: into itself or one of its subfolders.
    InvalidParent(String),
    /// A name or query is empty.
    Empty(&'static str),
    /// The file changed elsewhere every time the edit was about to be saved.
    KeepsChanging,
}

impl fmt::Display for SmartFolderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Vault(e) => write!(f, "{e}"),
            Self::Damaged(why) => write!(f, "{SMART_FOLDERS_FILE} cannot be read: {why}"),
            Self::NewerFormat(n) => write!(
                f,
                "{SMART_FOLDERS_FILE} was written by a newer version (format {n}); \
                 smart folders cannot be changed"
            ),
            Self::UnknownFolder(id) => write!(f, "no smart folder with id {id}"),
            Self::InvalidParent(id) => {
                write!(
                    f,
                    "a smart folder cannot go into itself or a subfolder: {id}"
                )
            }
            Self::Empty(what) => write!(f, "the {what} of a smart folder cannot be empty"),
            Self::KeepsChanging => write!(
                f,
                "{SMART_FOLDERS_FILE} kept changing elsewhere; try again in a moment"
            ),
        }
    }
}

impl std::error::Error for SmartFolderError {}

impl From<VaultError> for SmartFolderError {
    fn from(e: VaultError) -> Self {
        Self::Vault(e)
    }
}

impl From<io::Error> for SmartFolderError {
    fn from(e: io::Error) -> Self {
        Self::Vault(e.into())
    }
}

/// Result type of this module.
pub type Result<T> = std::result::Result<T, SmartFolderError>;

/// A smart folder as the UI shows it, in [`SmartFolders::list`]. Serialized
/// with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SmartFolder {
    /// The `id`.
    pub id: String,
    /// The `name`.
    pub name: String,
    /// The `query` (`docs/manual/query-language.md`).
    pub query: String,
    /// The parent in effect: `None` at the top level, also for a `parent`
    /// that does not exist or would make a cycle.
    pub parent: Option<String>,
    /// The `sort` in effect.
    pub sort: FolderSort,
    /// Levels below the top level, 0 there.
    pub depth: usize,
}

/// The content of `smart-folders.yaml`.
#[derive(Debug, Clone, PartialEq)]
pub struct SmartFolders {
    /// The top-level map; `folders` holds a null that marks its place, the
    /// entries are in `entries`.
    doc: Hash,
    /// The entries of `folders`, as written.
    entries: Vec<Yaml>,
}

impl Default for SmartFolders {
    fn default() -> Self {
        let mut doc = Hash::new();
        doc.insert(key("format"), Yaml::Integer(SMART_FOLDERS_FORMAT));
        doc.insert(key("folders"), Yaml::Null);
        Self {
            doc,
            entries: Vec::new(),
        }
    }
}

impl SmartFolders {
    /// Reads the content of `smart-folders.yaml`; an empty text means no
    /// smart folders.
    ///
    /// # Errors
    /// [`SmartFolderError::Damaged`] if `text` is not YAML, not a map, or
    /// its `folders` is not a list.
    pub fn parse(text: &str) -> Result<Self> {
        let docs = YamlLoader::load_from_str(text)
            .map_err(|e| SmartFolderError::Damaged(e.to_string()))?;
        let mut doc = match docs.into_iter().next() {
            None | Some(Yaml::Null) => return Ok(Self::default()),
            Some(Yaml::Hash(doc)) => doc,
            Some(_) => return Err(SmartFolderError::Damaged("not a map".into())),
        };
        let entries = match doc.get(&key("folders")) {
            None | Some(Yaml::Null) => Vec::new(),
            Some(Yaml::Array(entries)) => entries.clone(),
            Some(_) => return Err(SmartFolderError::Damaged("`folders` is not a list".into())),
        };
        doc.replace(key("folders"), Yaml::Null);
        if doc.get(&key("format")).is_none() {
            doc.insert_front(key("format"), Yaml::Integer(SMART_FOLDERS_FORMAT));
        }
        Ok(Self { doc, entries })
    }

    /// The file content: YAML with the keys in the order they were read,
    /// new keys at the end. Comments in the file read are not kept.
    #[must_use]
    pub fn to_yaml(&self) -> String {
        let mut doc = self.doc.clone();
        doc.replace(key("folders"), Yaml::Array(self.entries.clone()));
        let mut out = String::new();
        // Writing to a String cannot fail.
        let _ = YamlEmitter::new(&mut out).dump(&Yaml::Hash(doc));
        // The emitter writes null as `~` and quotes a string "~", so a line
        // ending in `~` is a null; written as in format.md §9.
        let mut text = String::with_capacity(out.len());
        for line in out.strip_prefix("---\n").unwrap_or(&out).lines() {
            match line.strip_suffix('~') {
                Some(head) if head.ends_with(": ") || head.trim() == "-" => {
                    text.push_str(head);
                    text.push_str("null");
                }
                _ => text.push_str(line),
            }
            text.push('\n');
        }
        text
    }

    /// The `format` of the file.
    #[must_use]
    pub fn format(&self) -> i64 {
        self.doc
            .get(&key("format"))
            .and_then(Yaml::as_i64)
            .unwrap_or(SMART_FOLDERS_FORMAT)
    }

    /// The smart folders depth first: each folder followed by its
    /// subfolders, siblings in file order. Entries without `id`, `name` or
    /// `query`, and entries repeating an earlier `id`, are left out.
    #[must_use]
    pub fn list(&self) -> Vec<SmartFolder> {
        let usable = self.usable();
        let parents: Vec<Option<usize>> = (0..usable.len())
            .map(|i| effective_parent(&usable, i))
            .collect();
        let mut out = Vec::with_capacity(usable.len());
        let mut stack: Vec<(usize, usize)> = (0..usable.len())
            .filter(|&i| parents[i].is_none())
            .rev()
            .map(|i| (i, 0))
            .collect();
        while let Some((i, depth)) = stack.pop() {
            let fields = &usable[i];
            out.push(SmartFolder {
                id: fields.id.to_owned(),
                name: fields.name.to_owned(),
                query: fields.query.to_owned(),
                parent: parents[i].map(|p| usable[p].id.to_owned()),
                sort: fields.sort,
                depth,
            });
            stack.extend(
                (0..usable.len())
                    .filter(|&c| parents[c] == Some(i))
                    .rev()
                    .map(|c| (c, depth + 1)),
            );
        }
        out
    }

    /// Adds a smart folder at the end of `parent`'s subfolders (the top
    /// level for `None`); returns its new id.
    ///
    /// # Errors
    /// [`SmartFolderError::Empty`], [`SmartFolderError::UnknownFolder`] for
    /// `parent`, or an I/O error if no id can be made.
    pub fn add(
        &mut self,
        name: &str,
        query: &str,
        parent: Option<&str>,
        sort: FolderSort,
    ) -> Result<String> {
        let id = new_ulid()?;
        self.add_with_id(&id, name, query, parent, sort)?;
        Ok(id)
    }

    fn add_with_id(
        &mut self,
        id: &str,
        name: &str,
        query: &str,
        parent: Option<&str>,
        sort: FolderSort,
    ) -> Result<()> {
        let (name, query) = (non_empty(name, "name")?, non_empty(query, "query")?);
        if let Some(parent) = parent {
            self.position(parent)?;
        }
        let mut entry = Hash::new();
        entry.insert(key("id"), Yaml::String(id.to_owned()));
        entry.insert(key("name"), Yaml::String(name.to_owned()));
        entry.insert(key("query"), Yaml::String(query.to_owned()));
        if let Some(parent) = parent {
            entry.insert(key("parent"), Yaml::String(parent.to_owned()));
        }
        entry.insert(key("sort"), Yaml::String(sort.as_str().to_owned()));
        self.entries.push(Yaml::Hash(entry));
        Ok(())
    }

    /// Changes name, query or sort of the folder `id`; `None` keeps a value.
    ///
    /// # Errors
    /// [`SmartFolderError::UnknownFolder`] or [`SmartFolderError::Empty`].
    pub fn update(
        &mut self,
        id: &str,
        name: Option<&str>,
        query: Option<&str>,
        sort: Option<FolderSort>,
    ) -> Result<()> {
        let name = name.map(|n| non_empty(n, "name")).transpose()?;
        let query = query.map(|q| non_empty(q, "query")).transpose()?;
        let at = self.position(id)?;
        let Yaml::Hash(entry) = &mut self.entries[at] else {
            unreachable!("position() only finds maps");
        };
        for (field, value) in [
            ("name", name),
            ("query", query),
            ("sort", sort.map(FolderSort::as_str)),
        ] {
            if let Some(value) = value {
                entry.replace(key(field), Yaml::String(value.to_owned()));
            }
        }
        Ok(())
    }

    /// Removes the folder `id`. Its subfolders move up to its parent.
    ///
    /// # Errors
    /// [`SmartFolderError::UnknownFolder`].
    pub fn remove(&mut self, id: &str) -> Result<()> {
        let at = self.position(id)?;
        let parent = self
            .list()
            .into_iter()
            .find(|f| f.id == id)
            .and_then(|f| f.parent);
        let children: Vec<String> = self
            .list()
            .into_iter()
            .filter(|f| f.parent.as_deref() == Some(id))
            .map(|f| f.id)
            .collect();
        self.entries.remove(at);
        for child in children {
            self.set_parent(&child, parent.as_deref())?;
        }
        Ok(())
    }

    /// Moves the folder `id` into `parent` (the top level for `None`) at
    /// `index` among its subfolders; an `index` past the end puts it last.
    ///
    /// # Errors
    /// [`SmartFolderError::UnknownFolder`] for either id, or
    /// [`SmartFolderError::InvalidParent`] if `parent` is `id` itself or one
    /// of its subfolders.
    pub fn move_to(&mut self, id: &str, parent: Option<&str>, index: usize) -> Result<()> {
        let at = self.position(id)?;
        if let Some(parent) = parent {
            self.position(parent)?;
            let mut ancestor = Some(parent.to_owned());
            let list = self.list();
            while let Some(current) = ancestor {
                if current == id {
                    return Err(SmartFolderError::InvalidParent(parent.to_owned()));
                }
                ancestor = list
                    .iter()
                    .find(|f| f.id == current)
                    .and_then(|f| f.parent.clone());
            }
        }
        let entry = self.entries.remove(at);
        // Where the new siblings are in the file, in order.
        let mut siblings: Vec<usize> = self
            .list()
            .iter()
            .filter(|f| f.parent.as_deref() == parent)
            .filter_map(|f| self.position(&f.id).ok())
            .collect();
        siblings.sort_unstable();
        let target = match siblings.get(index) {
            Some(&before) => before,
            None => siblings.last().map_or(self.entries.len(), |&last| last + 1),
        };
        self.entries.insert(target, entry);
        self.set_parent(id, parent)
    }

    fn set_parent(&mut self, id: &str, parent: Option<&str>) -> Result<()> {
        let at = self.position(id)?;
        let Yaml::Hash(entry) = &mut self.entries[at] else {
            unreachable!("position() only finds maps");
        };
        match parent {
            Some(parent) => {
                entry.replace(key("parent"), Yaml::String(parent.to_owned()));
            }
            None => {
                entry.remove(&key("parent"));
            }
        }
        Ok(())
    }

    /// Index in `entries` of the usable folder `id`.
    fn position(&self, id: &str) -> Result<usize> {
        let mut seen = Vec::new();
        for (at, entry) in self.entries.iter().enumerate() {
            if let Some(fields) = Fields::of(entry)
                && !seen.contains(&fields.id)
            {
                if fields.id == id {
                    return Ok(at);
                }
                seen.push(fields.id);
            }
        }
        Err(SmartFolderError::UnknownFolder(id.to_owned()))
    }

    /// The usable entries in file order, each id once.
    fn usable(&self) -> Vec<Fields<'_>> {
        let mut out: Vec<Fields<'_>> = Vec::new();
        for fields in self.entries.iter().filter_map(Fields::of) {
            if !out.iter().any(|f| f.id == fields.id) {
                out.push(fields);
            }
        }
        out
    }
}

/// The fields of a usable entry.
struct Fields<'a> {
    id: &'a str,
    name: &'a str,
    query: &'a str,
    parent: Option<&'a str>,
    sort: FolderSort,
}

impl<'a> Fields<'a> {
    fn of(entry: &'a Yaml) -> Option<Self> {
        let Yaml::Hash(map) = entry else {
            return None;
        };
        let text = |name: &str| match map.get(&key(name)) {
            Some(Yaml::String(s)) if !s.trim().is_empty() => Some(s.as_str()),
            _ => None,
        };
        Some(Self {
            id: text("id")?,
            name: text("name")?,
            query: text("query")?,
            parent: text("parent"),
            sort: text("sort").and_then(FolderSort::parse).unwrap_or_default(),
        })
    }
}

/// The parent of `usable[i]` in effect: `None` if it has none, it does not
/// exist, or following parents from it comes back to it. A parent caught in
/// a cycle above is at the top level itself, so it stays the parent.
fn effective_parent(usable: &[Fields<'_>], i: usize) -> Option<usize> {
    let parent_of = |j: usize| {
        usable[j]
            .parent
            .and_then(|p| usable.iter().position(|f| f.id == p))
    };
    let first = parent_of(i)?;
    let mut current = Some(first);
    for _ in 0..usable.len() {
        match current {
            None => return Some(first),
            Some(j) if j == i => return None,
            Some(j) => current = parent_of(j),
        }
    }
    Some(first) // a cycle further up that does not include `i`
}

fn key(name: &str) -> Yaml {
    Yaml::String(name.to_owned())
}

fn non_empty<'a>(value: &'a str, what: &'static str) -> Result<&'a str> {
    let value = value.trim();
    if value.is_empty() {
        Err(SmartFolderError::Empty(what))
    } else {
        Ok(value)
    }
}

impl Vault {
    /// Path of `smart-folders.yaml`.
    fn smart_folders_file(&self) -> PathBuf {
        self.root().join(SMART_FOLDERS_FILE)
    }

    /// Reads the smart folders (format.md §9); none if the file is missing.
    ///
    /// # Errors
    /// I/O errors or [`SmartFolderError::Damaged`].
    pub fn smart_folders(&self) -> Result<SmartFolders> {
        let (text, _) = self.read_smart_folders()?;
        SmartFolders::parse(&text.unwrap_or_default())
    }

    /// Applies `edit` to the smart folders and saves them, see the module
    /// documentation. `edit` may run more than once, on newer content each
    /// time; nothing is written if it fails or changes nothing.
    ///
    /// # Errors
    /// The error of `edit`; [`SmartFolderError::Damaged`] or
    /// [`SmartFolderError::NewerFormat`], when nothing is written; a
    /// read-only vault or I/O errors; [`SmartFolderError::KeepsChanging`].
    pub fn edit_smart_folders<T>(
        &self,
        mut edit: impl FnMut(&mut SmartFolders) -> Result<T>,
    ) -> Result<T> {
        if let Some(why) = self.read_only() {
            return Err(VaultError::ReadOnly(why.to_owned()).into());
        }
        let file = self.smart_folders_file();
        for _ in 0..ATTEMPTS {
            let (text, read_hash) = self.read_smart_folders()?;
            let mut folders = SmartFolders::parse(text.as_deref().unwrap_or_default())?;
            if folders.format() > SMART_FOLDERS_FORMAT {
                return Err(SmartFolderError::NewerFormat(folders.format()));
            }
            let result = edit(&mut folders)?;
            let new_text = folders.to_yaml();
            if text.as_deref() == Some(new_text.as_str())
                || (text.is_none() && folders == SmartFolders::default())
            {
                return Ok(result);
            }
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            let mut tmp = Some(TempFile::write(&file, new_text.as_bytes())?);
            let replaced = self.coordinated(Access::Write, &file, |file| {
                if read_hash != current_hash(file)? {
                    return Ok(false);
                }
                tmp.take().ok_or_else(accessor_ran_twice)?.replace(file)?;
                Ok(true)
            })?;
            if replaced {
                return Ok(result);
            }
        }
        Err(SmartFolderError::KeepsChanging)
    }

    /// The file's text and hash, `None` for both if it is missing.
    fn read_smart_folders(&self) -> Result<(Option<String>, Option<String>)> {
        let file = self.smart_folders_file();
        let bytes = self.coordinated(Access::Read, &file, |file| match fs::read(file) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        })?;
        let Some(bytes) = bytes else {
            return Ok((None, None));
        };
        let hash = sha256_hex(&bytes);
        let text =
            String::from_utf8(bytes).map_err(|_| SmartFolderError::Damaged("not UTF-8".into()))?;
        let text = text
            .strip_prefix('\u{feff}')
            .map(str::to_owned)
            .unwrap_or(text);
        Ok((Some(text), Some(hash)))
    }
}

fn current_hash(file: &std::path::Path) -> std::result::Result<Option<String>, VaultError> {
    match fs::read(file) {
        Ok(bytes) => Ok(Some(sha256_hex(&bytes))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: &str = "01JA7R0B2C3D4E5F6G7H8J9K0A";
    const B: &str = "01JA7R0B2C3D4E5F6G7H8J9K0B";
    const C: &str = "01JA7R0B2C3D4E5F6G7H8J9K0C";

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../tests/fixtures/format/9-smart-folders")
    }

    fn read(name: &str) -> String {
        fs::read_to_string(fixtures().join(name)).unwrap()
    }

    fn tree(folders: &SmartFolders) -> Vec<String> {
        folders
            .list()
            .iter()
            .map(|f| format!("{}{} [{}]", "  ".repeat(f.depth), f.name, f.sort.as_str()))
            .collect()
    }

    fn three() -> SmartFolders {
        let mut folders = SmartFolders::default();
        folders
            .add_with_id(A, "Radio", "#radio", None, FolderSort::ModifiedDesc)
            .unwrap();
        folders
            .add_with_id(
                B,
                "Aufgaben",
                "#radio has:task",
                Some(A),
                FolderSort::TitleAsc,
            )
            .unwrap();
        folders
            .add_with_id(C, "Lieder", "has:chord", None, FolderSort::CreatedDesc)
            .unwrap();
        folders
    }

    /// format.md §9: unknown keys, unknown values and unusable entries are
    /// kept; the golden files in `tests/fixtures/format/9-smart-folders/`.
    #[test]
    fn keeps_what_it_does_not_understand() {
        let mut folders = SmartFolders::parse(&read("before.yaml")).unwrap();
        assert_eq!(
            tree(&folders),
            [
                "Open radio tasks [modified-desc]",
                "  QRP [title-asc]",
                "Lost parent [created-desc]",
                "Songs [modified-desc]",
            ]
        );
        folders
            .update(
                "01JA7R0B2C3D4E5F6G7H8J9K0L",
                Some("Radio tasks"),
                None,
                None,
            )
            .unwrap();
        folders
            .add_with_id(A, "Neu", "\"40 m\" -#done", None, FolderSort::ModifiedAsc)
            .unwrap();
        assert_eq!(folders.to_yaml(), read("after.yaml"));
        assert_eq!(
            SmartFolders::parse(&folders.to_yaml()).unwrap(),
            folders,
            "reads back what it wrote"
        );
    }

    #[test]
    fn edits() {
        let mut folders = three();
        assert_eq!(
            tree(&folders),
            [
                "Radio [modified-desc]",
                "  Aufgaben [title-asc]",
                "Lieder [created-desc]"
            ]
        );

        folders.move_to(C, None, 0).unwrap();
        assert_eq!(tree(&folders)[0], "Lieder [created-desc]");
        folders.move_to(C, Some(A), 0).unwrap();
        assert_eq!(
            tree(&folders),
            [
                "Radio [modified-desc]",
                "  Lieder [created-desc]",
                "  Aufgaben [title-asc]"
            ]
        );
        folders.move_to(C, Some(A), 99).unwrap();
        assert_eq!(tree(&folders)[2], "  Lieder [created-desc]");
        folders.move_to(A, Some(B), 0).unwrap_err();
        assert!(matches!(
            folders.move_to(A, Some(A), 0),
            Err(SmartFolderError::InvalidParent(_))
        ));

        folders
            .update(B, Some(" Offen "), None, Some(FolderSort::ModifiedAsc))
            .unwrap();
        assert!(matches!(
            folders.update(B, None, Some("  "), None),
            Err(SmartFolderError::Empty("query"))
        ));
        assert!(matches!(
            folders.update("nope", Some("x"), None, None),
            Err(SmartFolderError::UnknownFolder(_))
        ));

        // Subfolders move up when their parent goes.
        folders.remove(A).unwrap();
        assert_eq!(
            tree(&folders),
            ["Offen [modified-asc]", "Lieder [created-desc]"]
        );
        assert!(folders.list().iter().all(|f| f.parent.is_none()));
    }

    #[test]
    fn cycles_and_missing_parents_are_the_top_level() {
        let folders = SmartFolders::parse(&format!(
            "folders:\n\
             - {{id: {A}, name: a, query: x, parent: {B}}}\n\
             - {{id: {B}, name: b, query: x, parent: {A}}}\n\
             - {{id: {C}, name: c, query: x, parent: {A}}}\n"
        ))
        .unwrap();
        assert_eq!(
            tree(&folders),
            [
                "a [modified-desc]",
                "  c [modified-desc]",
                "b [modified-desc]"
            ]
        );
    }

    #[test]
    fn damaged_files_are_errors() {
        for text in ["folders: [", "- a\n- b\n", "folders: 3\n"] {
            assert!(
                matches!(SmartFolders::parse(text), Err(SmartFolderError::Damaged(_))),
                "{text:?}"
            );
        }
        assert!(SmartFolders::parse("").unwrap().list().is_empty());
        assert!(
            SmartFolders::parse("format: 1\n")
                .unwrap()
                .list()
                .is_empty()
        );
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn saved_in_the_vault() {
        let (_dir, vault) = vault();
        assert!(vault.smart_folders().unwrap().list().is_empty());
        // Nothing to save: no file is created.
        vault.edit_smart_folders(|_| Ok(())).unwrap();
        assert!(!vault.smart_folders_file().exists());

        let id = vault
            .edit_smart_folders(|f| f.add("Radio", "#radio", None, FolderSort::default()))
            .unwrap();
        let list = vault.smart_folders().unwrap().list();
        assert_eq!(
            (list[0].id.as_str(), list[0].name.as_str()),
            (id.as_str(), "Radio")
        );
        let text = fs::read_to_string(vault.smart_folders_file()).unwrap();
        assert!(text.starts_with("format: 1\nfolders:\n  - id: "), "{text}");

        let failed = vault.edit_smart_folders(|f| f.update("nope", None, None, None));
        assert!(failed.is_err());
        assert_eq!(
            fs::read_to_string(vault.smart_folders_file()).unwrap(),
            text
        );
    }

    #[test]
    fn a_change_elsewhere_is_kept() {
        let (_dir, vault) = vault();
        let file = vault.smart_folders_file();
        fs::write(
            &file,
            format!("folders:\n  - {{id: {A}, name: a, query: x}}\n"),
        )
        .unwrap();
        let mut runs = 0;
        vault
            .edit_smart_folders(|f| {
                runs += 1;
                if runs == 1 {
                    // Another device adds a folder while this edit is made.
                    fs::write(
                        &file,
                        format!(
                            "folders:\n  - {{id: {A}, name: a, query: x}}\n  \
                             - {{id: {B}, name: b, query: y}}\n"
                        ),
                    )
                    .unwrap();
                }
                f.update(A, Some("renamed"), None, None)
            })
            .unwrap();
        assert_eq!(runs, 2, "applied again to the new content");
        let names: Vec<String> = vault
            .smart_folders()
            .unwrap()
            .list()
            .into_iter()
            .map(|f| f.name)
            .collect();
        assert_eq!(names, ["renamed", "b"]);
    }

    #[test]
    fn newer_or_damaged_files_are_not_changed() {
        let (_dir, vault) = vault();
        let file = vault.smart_folders_file();
        for (text, newer) in [("format: 2\nfolders: []\n", true), ("folders: [\n", false)] {
            fs::write(&file, text).unwrap();
            let result = vault.edit_smart_folders(|f| f.add("a", "b", None, FolderSort::default()));
            if newer {
                assert!(matches!(result, Err(SmartFolderError::NewerFormat(2))));
            } else {
                assert!(matches!(result, Err(SmartFolderError::Damaged(_))));
            }
            assert_eq!(fs::read_to_string(&file).unwrap(), text);
        }
    }
}
