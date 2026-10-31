// SPDX-License-Identifier: Apache-2.0
//! The Markdown adapter (ADR-0012, ADR-0019, format.md §12.1): one Markdown
//! or ChordPro file, or a folder with such files at any depth. Each file
//! becomes one note; its folders below the chosen folder are its folders.
//!
//! - A Markdown file (`.md`, `.markdown`, `.mdown`, `.mkd`, `.mkdn`) keeps
//!   its text, front matter of other tools included (format.md §4). Only an
//!   `id` it brings along is replaced, as for every imported note. It is
//!   named after the level-1 heading it starts with; otherwise after the
//!   `title` in its front matter, which then becomes that heading, so the
//!   note shows it (§5.2, §12.1 rule 7); otherwise after its file.
//! - A ChordPro file (`.cho`, `.crd`, `.chopro`, `.chordpro`) becomes a
//!   note with its `{title}` as a level-1 heading, so the note shows it
//!   (format.md §5.2), followed by the whole file in one `chordpro` block
//!   (§5.7 rule 9).
//! - Local files that links and pictures point to inside the chosen folder
//!   go into `attachments/` (format.md §6) and the links are rewritten.
//!   Links to other notes, to the web and to files outside the chosen folder
//!   stay as they are; links to missing files are reported.
//! - Text that is not UTF-8 is read as Windows-1252, and the report says so.
//!
//! The dates are the file's creation and modification times, so a re-import
//! updates a note whose file changed since (§12.3). A `created` in the front
//! matter is kept.
//!
//! Hidden files and folders (`.obsidian`, `.git`, …) are left out.

use std::fs;
use std::path::Path;
use std::time::UNIX_EPOCH;

use schnipsle_core::front_matter;
use schnipsle_core::stamp::rfc3339_utc;

use crate::files::{self, Found};
use crate::note::{Batch, ImportedNote, Notice, Source};
use crate::pipeline::ImportError;

/// The extensions of Markdown files.
const MARKDOWN: &[&str] = &["md", "markdown", "mdown", "mkd", "mkdn"];

/// The extensions of ChordPro files (format.md §5.7 rule 9).
const CHORDPRO: &[&str] = &["cho", "crd", "chopro", "chordpro"];

/// Reads the Markdown or ChordPro file at `path`, or such files in the
/// folder at `path` and its subfolders, into a batch for the pipeline.
///
/// # Errors
///
/// [`ImportError::Source`] if `path` cannot be read or is a file of another
/// kind. Files that cannot be read are notices.
pub fn read(path: &Path) -> Result<Batch, ImportError> {
    let (root, found, mut notices) = files::find(
        path,
        &is_note,
        &|_| false,
        "a Markdown file (.md) or a ChordPro file (.cho, .crd, .chopro, .chordpro)",
    )?;
    let mut notes = Vec::new();
    for file in &found {
        let bytes = match fs::read(&file.file) {
            Ok(bytes) => bytes,
            Err(e) => {
                notices.push(Notice::general(format!(
                    "{} cannot be read and is not imported: {e}",
                    file.path
                )));
                continue;
            }
        };
        let text = String::from_utf8(bytes).unwrap_or_else(|e| {
            notices.push(Notice::general(format!(
                "{} is not UTF-8 text; it is read as Windows-1252 (Western European).",
                file.path
            )));
            files::windows_1252(e.as_bytes())
        });
        let mut problems = Vec::new();
        let note = if files::has_extension(&file.file, CHORDPRO) {
            song(file, &text)
        } else {
            markdown(&root, file, text, &mut problems)
        };
        notices.extend(problems.into_iter().map(|message| Notice {
            source_id: Some(file.path.clone()),
            title: Some(note.title.clone()),
            message,
        }));
        notes.push(note);
    }
    Ok(Batch {
        source: Source::Markdown,
        input: files::display(path),
        notes,
        notices,
    })
}

/// `true` for the files this adapter makes notes of.
fn is_note(path: &Path) -> bool {
    files::has_extension(path, MARKDOWN) || files::has_extension(path, CHORDPRO)
}

/// What every note from a file has: its source id, folders and dates.
fn imported(file: &Found, title: String, body: String) -> ImportedNote {
    let created_ms = fs::metadata(&file.file)
        .and_then(|m| m.created())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| u64::try_from(d.as_millis()).ok());
    // A copy can be younger than its last change.
    let created_ms = match (created_ms, file.modified_ms) {
        (Some(c), Some(m)) => Some(c.min(m)),
        (c, m) => c.or(m),
    };
    ImportedNote {
        source_id: file.path.clone(),
        title,
        body,
        folders: file.folders.clone(),
        created: created_ms.map(rfc3339_utc),
        modified: file.modified_ms.map(rfc3339_utc),
        ..ImportedNote::default()
    }
}

/// The note made from the Markdown file `file`, whose text is `text`.
fn markdown(root: &Path, file: &Found, text: String, problems: &mut Vec<String>) -> ImportedNote {
    let attachments = files::linked_files(root, &file.dir, &text, &is_note, problems);
    let named = front_matter::get(&text, "title")
        .map(|t| t.trim().to_owned())
        .filter(|t| !t.is_empty());
    let body = front_matter::body(&text);
    let (title, text) = match (files::leading_heading(body), named) {
        (Some(heading), _) => (heading.to_owned(), text),
        (None, Some(named)) => {
            let head = &text[..text.len() - body.len()];
            let rest = body.trim_start_matches(['\n', '\r']);
            let text = format!("{head}# {named}\n\n{rest}");
            (named, text)
        }
        (None, None) => (file.stem.clone(), text),
    };
    let pinned = front_matter::is_pinned(&text);
    ImportedNote {
        pinned,
        attachments,
        ..imported(file, title, text)
    }
}

/// The note made from the ChordPro file `file`, whose text is `text`: its
/// title as a heading, then the file in one `chordpro` block.
fn song(file: &Found, text: &str) -> ImportedNote {
    let title = chordpro_title(text).unwrap_or_else(|| file.stem.clone());
    let fence = "`".repeat(longest_backtick_run(text).max(2) + 1);
    let song = text.trim_end_matches(['\n', '\r']);
    let body = format!("# {title}\n\n{fence}chordpro\n{song}\n{fence}\n");
    imported(file, title, body)
}

/// The value of the first `{title: …}` or `{t: …}` directive.
fn chordpro_title(text: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let inner = line.trim().strip_prefix('{')?.strip_suffix('}')?;
        let (name, value) = inner.split_once(':')?;
        let name = name.trim();
        let is_title = name.eq_ignore_ascii_case("title") || name.eq_ignore_ascii_case("t");
        let value = value.trim();
        (is_title && !value.is_empty()).then(|| value.to_owned())
    })
}

/// The longest run of backticks in `text`, so a fence around it is longer.
fn longest_backtick_run(text: &str) -> usize {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        if c == '`' {
            run += 1;
            longest = longest.max(run);
        } else {
            run = 0;
        }
    }
    longest
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chordpro_titles() {
        assert_eq!(
            chordpro_title("# comment\n{t:Abendlied}\n{title: Other}").as_deref(),
            Some("Abendlied")
        );
        assert_eq!(
            chordpro_title("  { Title :  Morning song }  ").as_deref(),
            Some("Morning song")
        );
        assert_eq!(chordpro_title("{subtitle: x}\n{title:}"), None);
    }

    #[test]
    fn titles_and_headings() {
        let file = Found {
            path: "a/Old name.md".into(),
            file: "/nowhere/a/Old name.md".into(),
            dir: "/nowhere/a".into(),
            folders: vec!["a".into()],
            stem: "Old name".into(),
            modified_ms: None,
        };
        let note =
            |text: &str| markdown(Path::new("/nowhere"), &file, text.into(), &mut Vec::new());

        let named = note("---\ntitle: \"Antenna tuner\"\n---\n\nParts list.\n");
        assert_eq!(named.title, "Antenna tuner");
        assert_eq!(
            named.body,
            "---\ntitle: \"Antenna tuner\"\n---\n# Antenna tuner\n\nParts list.\n"
        );
        let headed = note("---\ntitle: Other\n---\n# Tuner\n\nParts list.\n");
        assert_eq!(headed.title, "Tuner");
        assert_eq!(
            headed.body,
            "---\ntitle: Other\n---\n# Tuner\n\nParts list.\n"
        );
        let plain = note("Parts list.\n");
        assert_eq!(plain.title, "Old name");
        assert_eq!(plain.body, "Parts list.\n");
    }

    #[test]
    fn fences_outlast_the_text() {
        assert_eq!(longest_backtick_run("a ``` b ```` c"), 4);
        assert_eq!(longest_backtick_run("none"), 0);
    }
}
