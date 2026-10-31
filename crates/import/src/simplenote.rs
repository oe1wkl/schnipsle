// SPDX-License-Identifier: Apache-2.0
//! The Simplenote adapter (ADR-0012, format.md §12.1): the export Simplenote
//! writes as a zip, or its `notes.json` on its own.
//!
//! Only `source/notes.json` is read. The zip's `.txt` files hold the same
//! text without ids, dates or tags, and some of their names are not UTF-8.
//! Each note keeps its `id`, `creationDate`, `lastModified` and `pinned`;
//! its `tags` go on the final tag line unless the text has them already
//! (the pipeline does that). The title is the first line, as in Simplenote.
//! Notes in `trashedNotes` go to the trash (§12.1 rule 6).
//!
//! The text stays as Simplenote has it, whether or not the note was marked
//! as Markdown: plain text is valid Markdown, and the editor shows its line
//! breaks as they are.

use std::fs;
use std::io::Read as _;
use std::path::Path;

use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

use crate::note::{Batch, ImportedNote, Notice, Source};
use crate::pipeline::ImportError;

/// Where the data is inside the zip.
const NOTES_IN_ZIP: &str = "source/notes.json";

/// The file name of the data.
const NOTES_FILE: &str = "notes.json";

/// More than any real export; protects against a damaged or hostile zip.
const MAX_NOTES_JSON: u64 = 512 * 1024 * 1024;

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Export {
    #[serde(default)]
    active_notes: Vec<SimpleNote>,
    #[serde(default)]
    trashed_notes: Vec<SimpleNote>,
}

/// One note in `notes.json`; fields not needed here (`markdown`,
/// `publicURL`, …) are ignored.
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SimpleNote {
    #[serde(default)]
    id: String,
    #[serde(default)]
    content: String,
    #[serde(default)]
    creation_date: Option<String>,
    #[serde(default)]
    last_modified: Option<String>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    pinned: bool,
}

/// Reads the Simplenote export at `path` into a batch for the pipeline:
/// the zip Simplenote writes, its `notes.json`, or a folder with the
/// unpacked export (`source/notes.json` or `notes.json` in it).
///
/// # Errors
///
/// [`ImportError::Source`] if `path` cannot be read, has no `notes.json`,
/// or `notes.json` is not a Simplenote export. A note without an id is left
/// out with a notice.
pub fn read(path: &Path) -> Result<Batch, ImportError> {
    let json = notes_json(path)?;
    let export: Export = serde_json::from_str(&json).map_err(|e| {
        ImportError::Source(format!(
            "{} is not a Simplenote export that can be read: {e}",
            display_name(path)
        ))
    })?;
    let mut notices = Vec::new();
    let mut notes = Vec::new();
    let all = export
        .active_notes
        .into_iter()
        .map(|n| (n, false))
        .chain(export.trashed_notes.into_iter().map(|n| (n, true)));
    for (note, trashed) in all {
        let id = note.id.trim();
        if id.is_empty() {
            let title = first_line(&note.content);
            notices.push(Notice {
                source_id: None,
                title: (!title.is_empty()).then(|| title.to_owned()),
                message: "This note has no id in notes.json, so it is not imported.".to_owned(),
            });
            continue;
        }
        notes.push(ImportedNote {
            source_id: id.to_owned(),
            body: note.content,
            created: note.creation_date,
            modified: note.last_modified,
            tags: note.tags,
            pinned: note.pinned,
            trashed,
            ..ImportedNote::default()
        });
    }
    Ok(Batch {
        source: Source::Simplenote,
        input: display_name(path),
        notes,
        notices,
    })
}

/// The text of `notes.json` from the zip, the file or the folder at `path`.
fn notes_json(path: &Path) -> Result<String, ImportError> {
    let cannot = |e: &dyn std::fmt::Display| {
        ImportError::Source(format!("{} cannot be read: {e}", display_name(path)))
    };
    if path.is_dir() {
        for candidate in [NOTES_IN_ZIP, NOTES_FILE] {
            let file = path.join(candidate);
            if file.is_file() {
                return fs::read_to_string(&file).map_err(|e| cannot(&e));
            }
        }
        return Err(ImportError::Source(format!(
            "{} has no {NOTES_IN_ZIP} or {NOTES_FILE}; choose the zip Simplenote exported, or \
             the folder it was unpacked to.",
            display_name(path)
        )));
    }
    let is_zip = path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"));
    if !is_zip {
        return fs::read_to_string(path).map_err(|e| cannot(&e));
    }
    let file = fs::File::open(path).map_err(|e| cannot(&e))?;
    let mut zip = zip::ZipArchive::new(file).map_err(|e| cannot(&e))?;
    let mut entry = zip.by_name(NOTES_IN_ZIP).map_err(|_| {
        ImportError::Source(format!(
            "{} has no {NOTES_IN_ZIP}; is it the zip Simplenote exported?",
            display_name(path)
        ))
    })?;
    let mut bytes = Vec::new();
    entry
        .by_ref()
        .take(MAX_NOTES_JSON + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| cannot(&e))?;
    if bytes.len() as u64 > MAX_NOTES_JSON {
        return Err(ImportError::Source(format!(
            "{NOTES_IN_ZIP} in {} is too large",
            display_name(path)
        )));
    }
    String::from_utf8(bytes).map_err(|e| cannot(&e))
}

/// The file or folder name of `path`, NFC, for the report and the log.
fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().nfc().collect(),
    )
}

/// The first line of `text` with text on it, trimmed.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn read_json(json: &str) -> Result<Batch, ImportError> {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join(NOTES_FILE);
        fs::write(&file, json).unwrap();
        read(&file)
    }

    #[test]
    fn active_and_trashed_notes() {
        let batch = read_json(
            r#"{"activeNotes": [{"id": "a1", "content": "One\r\n", "tags": ["x"],
                "pinned": true, "creationDate": "2020-01-01T00:00:00.000Z",
                "lastModified": "2020-01-02T00:00:00.000Z", "markdown": true}],
                "trashedNotes": [{"id": "t1", "content": "Gone"}]}"#,
        )
        .unwrap();
        assert_eq!(batch.source, Source::Simplenote);
        assert_eq!(batch.input, "notes.json");
        let [one, gone] = &batch.notes[..] else {
            panic!("{:?}", batch.notes);
        };
        assert_eq!(one.source_id, "a1");
        assert_eq!(one.body, "One\r\n"); // line endings are the pipeline's job
        assert_eq!(one.tags, ["x"]);
        assert!(one.pinned && !one.trashed);
        assert_eq!(one.created.as_deref(), Some("2020-01-01T00:00:00.000Z"));
        assert_eq!(one.modified.as_deref(), Some("2020-01-02T00:00:00.000Z"));
        assert!(one.folders.is_empty() && one.title.is_empty());
        assert!(gone.trashed && !gone.pinned);
        assert!(batch.notices.is_empty());
    }

    #[test]
    fn a_note_without_id_is_left_out() {
        let batch = read_json(r#"{"activeNotes": [{"content": "\n  Lost  \nmore"}, {"id": " "}]}"#)
            .unwrap();
        assert!(batch.notes.is_empty());
        assert_eq!(batch.notices.len(), 2);
        assert_eq!(batch.notices[0].title.as_deref(), Some("Lost"));
        assert_eq!(batch.notices[1].title, None);
    }

    #[test]
    fn not_an_export() {
        for json in ["[1, 2]", "{\"activeNotes\": [", "{\"activeNotes\": 3}"] {
            match read_json(json) {
                Err(ImportError::Source(why)) => assert!(why.contains("notes.json"), "{why}"),
                other => panic!("{json}: {other:?}"),
            }
        }
        let dir = tempfile::tempdir().unwrap();
        match read(dir.path()) {
            Err(ImportError::Source(why)) => assert!(why.contains("has no"), "{why}"),
            other => panic!("{other:?}"),
        }
    }

    #[test]
    fn first_lines() {
        assert_eq!(first_line("\n \n  Title \nnext"), "Title");
        assert_eq!(first_line(""), "");
    }
}
