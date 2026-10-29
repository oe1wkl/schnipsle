// SPDX-License-Identifier: Apache-2.0
//! The Apple Notes adapter (ADR-0019, format.md §12.1): the Markdown export
//! of Notes, one Finder folder per export, plus `apple-notes-metadata.json`
//! from `scripts/apple-notes-metadata.js` in the folder chosen for the
//! import.
//!
//! The export has no IDs and no dates, so each exported file is matched to a
//! note in the metadata by its title: the note's `name` equals the file's
//! first line or its file name, compared without Markdown marks and the
//! characters a file name drops; a name shortened with `…` matches the start.
//! Of several matches the one whose snippet continues like the file's text
//! wins. A matched note gets the note's `id`, dates and real folder in
//! Notes; a file without a clear match keeps the time of the export and the
//! folder it has in the export.
//!
//! The text is the export's Markdown with its quirks tidied up:
//!
//! - Notes ends every line with two spaces (a hard line break). They are
//!   kept only where they break a line inside a paragraph, so the text
//!   renders as before.
//! - A stray U+2028 at the end of a line is dropped; one inside a line
//!   becomes a hard line break.
//! - Attachments that are not pictures (PDFs) are linked as images; they
//!   become ordinary links (format.md §5.5).
//!
//! Everything else stays as Notes wrote it, `++underline++` too: it has no
//! Markdown equivalent, and the marks keep the information.

use std::collections::HashSet;
use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use schnipsle_core::dates::parse_rfc3339;
use schnipsle_core::stamp::rfc3339_utc;
use serde::Deserialize;
use unicode_normalization::UnicodeNormalization;

use crate::note::{AttachmentData, Batch, ImportedAttachment, ImportedNote, Notice, Source};
use crate::pipeline::ImportError;

/// The metadata file in the folder chosen for the import.
pub const METADATA_FILE: &str = "apple-notes-metadata.json";

/// The newest `format` of [`METADATA_FILE`] this adapter reads.
const METADATA_FORMAT: u64 = 1;

/// The folder Notes puts attachments in, next to the exported notes.
const ATTACHMENTS: &str = "Attachments";

/// Extensions shown as pictures; other attachments become links.
const PICTURES: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "webp", "heic", "heif", "tif", "tiff", "bmp", "svg", "avif",
];

#[derive(Deserialize)]
struct Metadata {
    #[serde(default)]
    notes: Vec<MetaNote>,
}

/// One note in [`METADATA_FILE`]; fields not needed here are ignored.
#[derive(Deserialize)]
struct MetaNote {
    id: String,
    #[serde(default)]
    folder: Vec<String>,
    #[serde(default)]
    name: String,
    #[serde(default)]
    created: Option<String>,
    #[serde(default)]
    modified: Option<String>,
    #[serde(default)]
    locked: bool,
    #[serde(default)]
    snippet: String,
}

/// An exported Markdown file.
struct Exported {
    /// Its path relative to the chosen folder, with `/`, NFC.
    path: String,
    /// The folder it is in on disk, for its attachment links.
    dir: PathBuf,
    /// Its folders in the export, from the chosen folder down.
    folders: Vec<String>,
    /// Its file name without `.md`, NFC.
    stem: String,
    /// Its text as Notes wrote it.
    text: String,
    /// Its modification time: the time of the export.
    modified_ms: Option<u64>,
}

/// Reads the Apple Notes export in `dir` into a batch for the pipeline.
///
/// # Errors
///
/// [`ImportError::Source`] if `dir` cannot be read, or if the metadata file
/// is damaged or of a newer format (importing without it would give every
/// note other ids than a later import with it). A missing metadata file, an
/// unreadable note file and unmatched files are notices.
pub fn read(dir: &Path) -> Result<Batch, ImportError> {
    let mut notices = Vec::new();
    let mut exported = Vec::new();
    let top = fs::read_dir(dir)
        .map_err(|e| ImportError::Source(format!("{} cannot be read: {e}", dir.display())))?;
    walk(dir, top, &mut exported, &mut notices);
    exported.sort_by(|a, b| a.path.cmp(&b.path));
    let meta = metadata(dir)?;
    if meta.is_none() {
        notices.push(Notice::general(format!(
            "{METADATA_FILE} is missing, so every note keeps the time of the export and the \
             folder it has in the export. A later import with it would add the notes again. \
             Run scripts/apple-notes-metadata.js on the export folder first."
        )));
    }
    let meta = meta.unwrap_or_default();

    let mut matched = HashSet::new();
    let mut notes = Vec::new();
    for file in &exported {
        let found = if meta.is_empty() {
            Ok(None)
        } else {
            find(file, &meta)
        };
        let note = if let Ok(Some(i)) = found {
            matched.insert(i);
            from_metadata(file, &meta[i])
        } else {
            let note = unmatched(file);
            if !meta.is_empty() {
                let why = if found.is_err() {
                    "Several notes in Notes have this title, and its text does not tell which one \
                     it is"
                } else {
                    "No note in the metadata has this title"
                };
                notices.push(Notice {
                    source_id: Some(note.source_id.clone()),
                    title: Some(note.title.clone()),
                    message: format!(
                        "{why}; it keeps the time of the export and its folder in the export."
                    ),
                });
            }
            note
        };
        notes.push(note);
    }
    notices.extend(not_exported(&meta, &matched));

    let input = dir.file_name().map_or_else(
        || dir.display().to_string(),
        |n| n.to_string_lossy().nfc().collect(),
    );
    Ok(Batch {
        source: Source::AppleNotes,
        input,
        notes,
        notices,
    })
}

/// Collects the `.md` files below `root`, skipping hidden entries and
/// attachment folders.
fn walk(root: &Path, entries: fs::ReadDir, out: &mut Vec<Exported>, notices: &mut Vec<Notice>) {
    let mut entries: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
            continue;
        };
        if name.starts_with('.') {
            continue;
        }
        let rel: String = path
            .strip_prefix(root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/")
            .nfc()
            .collect();
        if path.is_dir() {
            if name == ATTACHMENTS {
                continue;
            }
            match fs::read_dir(&path) {
                Ok(inner) => walk(root, inner, out, notices),
                Err(e) => notices.push(Notice::general(format!("{rel} cannot be read: {e}"))),
            }
            continue;
        }
        let Some(stem) = name
            .rsplit_once('.')
            .filter(|(_, ext)| ext.eq_ignore_ascii_case("md"))
            .map(|(stem, _)| stem)
        else {
            continue;
        };
        let text = match fs::read(&path).map(String::from_utf8) {
            Ok(Ok(text)) => text,
            Ok(Err(_)) => {
                notices.push(Notice::general(format!(
                    "{rel} is not UTF-8 text and is not imported."
                )));
                continue;
            }
            Err(e) => {
                notices.push(Notice::general(format!(
                    "{rel} cannot be read and is not imported: {e}"
                )));
                continue;
            }
        };
        let modified_ms = fs::metadata(&path)
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .and_then(|d| u64::try_from(d.as_millis()).ok());
        let mut folders: Vec<String> = rel.split('/').map(str::to_owned).collect();
        folders.pop();
        out.push(Exported {
            path: rel,
            dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
            folders,
            stem: stem.nfc().collect(),
            text,
            modified_ms,
        });
    }
}

/// The notes in the metadata file, or `None` if there is none.
fn metadata(dir: &Path) -> Result<Option<Vec<MetaNote>>, ImportError> {
    let file = dir.join(METADATA_FILE);
    let text = match fs::read_to_string(&file) {
        Ok(text) => text,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => {
            return Err(ImportError::Source(format!(
                "{METADATA_FILE} cannot be read: {e}"
            )));
        }
    };
    let damaged = |e: serde_json::Error| {
        ImportError::Source(format!(
            "{METADATA_FILE} is damaged ({e}); run the script again."
        ))
    };
    let value: serde_json::Value = serde_json::from_str(&text).map_err(damaged)?;
    match value.get("format").and_then(serde_json::Value::as_u64) {
        Some(f) if f <= METADATA_FORMAT => {}
        Some(f) => {
            return Err(ImportError::Source(format!(
                "{METADATA_FILE} has format {f}, which this version of Schnipsle cannot read."
            )));
        }
        None => {
            return Err(ImportError::Source(format!(
                "{METADATA_FILE} has no format number; run the script again."
            )));
        }
    }
    let meta: Metadata = serde_json::from_value(value).map_err(damaged)?;
    Ok(Some(meta.notes))
}

/// The note in `meta` that `file` was exported from: `Ok(None)` without a
/// match, `Err(())` if several match equally well.
fn find(file: &Exported, meta: &[MetaNote]) -> Result<Option<usize>, ()> {
    let line = Key::new(first_line(&file.text));
    let stem = Key::new(&file.stem);
    let candidates: Vec<usize> = meta
        .iter()
        .enumerate()
        .filter(|(_, n)| !n.locked)
        .filter(|(_, n)| {
            let name = Key::new(&n.name);
            name.matches(&line) || name.matches(&stem)
        })
        .map(|(i, _)| i)
        .collect();
    if candidates.len() <= 1 {
        return Ok(candidates.first().copied());
    }
    let text = plain(&strip_links(&file.text));
    let score = |i: usize| common_prefix(&text, &plain(&meta[i].snippet));
    let best = candidates.iter().map(|&i| score(i)).max().unwrap_or(0);
    let mut winners = candidates.into_iter().filter(|&i| score(i) == best);
    match (winners.next(), winners.next()) {
        (Some(i), None) => Ok(Some(i)),
        _ => Err(()),
    }
}

/// A file matched to `meta`: its id, dates and folder in Notes.
fn from_metadata(file: &Exported, meta: &MetaNote) -> ImportedNote {
    let mut note = imported(file);
    note.source_id.clone_from(&meta.id);
    if note.title.is_empty() {
        // An empty note: Notes names it like its file.
        note.title = meta.name.nfc().collect();
    }
    note.folders = meta.folder.iter().map(|f| f.nfc().collect()).collect();
    note.created = meta.created.as_deref().map(tidy_time);
    note.modified = meta.modified.as_deref().map(tidy_time);
    note
}

/// A file without a match: `export:<path>`, the export time and folder.
fn unmatched(file: &Exported) -> ImportedNote {
    let mut note = imported(file);
    if note.title.is_empty() {
        note.title.clone_from(&file.stem);
    }
    let time = file.modified_ms.map(rfc3339_utc);
    note.created.clone_from(&time);
    note.modified = time;
    note
}

/// What both kinds of file have in common: text, title (empty for an empty
/// note), attachments.
fn imported(file: &Exported) -> ImportedNote {
    let title = display_title(first_line(&file.text));
    let attachments = attachments(file);
    let body = tidy(&file.text, &attachments);
    ImportedNote {
        source_id: format!("export:{}", file.path),
        title,
        body,
        folders: file.folders.clone(),
        attachments,
        ..ImportedNote::default()
    }
}

/// A time from the metadata as the app writes them (`…T09:14:00Z`); left as
/// it is if it is not RFC 3339, so the pipeline reports it.
fn tidy_time(value: &str) -> String {
    parse_rfc3339(value)
        .and_then(|ms| u64::try_from(ms).ok())
        .map_or_else(|| value.to_owned(), rfc3339_utc)
}

/// Report lines for notes the export does not hold although it should:
/// locked notes, and a note whose folder holds another note with the same
/// title that was exported (Notes writes only one of them).
fn not_exported(meta: &[MetaNote], matched: &HashSet<usize>) -> Vec<Notice> {
    let mut out = Vec::new();
    for (i, note) in meta.iter().enumerate() {
        let message = if note.locked {
            "This note is locked, so Notes does not export it. Unlock it in Notes and export it \
             again to import it."
        } else if !matched.contains(&i)
            && matched.iter().any(|&m| {
                meta[m].folder == note.folder && Key::new(&meta[m].name) == Key::new(&note.name)
            })
        {
            "Not in the export: Notes exports only one of several notes with the same title in a \
             folder. Give it another title in Notes and export it again."
        } else {
            continue;
        };
        out.push(Notice {
            source_id: Some(note.id.clone()),
            title: Some(note.name.nfc().collect()),
            message: message.to_owned(),
        });
    }
    out
}

/// A title as compared for matching (ADR-0019).
#[derive(Debug, PartialEq, Eq)]
struct Key {
    text: String,
    /// It ended in `…`: Notes shortened it, so it matches the start.
    shortened: bool,
}

impl Key {
    fn new(title: &str) -> Self {
        let title: String = title.nfc().collect::<String>().replace('\u{a0}', " ");
        let title = title.trim();
        let (title, shortened) = match title.strip_suffix('…') {
            Some(rest) => (rest, true),
            None => (title, false),
        };
        let title = strip_heading(title.trim());
        let text = strip_links(title)
            .chars()
            .filter(|c| !matches!(c, ':' | '"' | '/' | '\\' | '*' | '_' | '~' | '`' | '+'))
            .flat_map(char::to_lowercase)
            .collect::<String>()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        Self { text, shortened }
    }

    fn matches(&self, other: &Self) -> bool {
        if self.text.is_empty() || other.text.is_empty() {
            return false;
        }
        (self.shortened && other.text.starts_with(&self.text))
            || (other.shortened && self.text.starts_with(&other.text))
            || self.text == other.text
    }
}

/// The first line that is not blank.
fn first_line(text: &str) -> &str {
    text.lines()
        .map(str::trim)
        .find(|l| !l.is_empty())
        .unwrap_or("")
}

/// `line` without a leading `#`…`######` heading mark.
fn strip_heading(line: &str) -> &str {
    let rest = line.trim_start_matches('#');
    if rest.len() < line.len()
        && line.len() - rest.len() <= 6
        && (rest.is_empty() || rest.starts_with(' '))
    {
        rest.trim()
    } else {
        line
    }
}

/// The title a first line gives: without heading, emphasis and link marks.
fn display_title(line: &str) -> String {
    let line = strip_links(strip_heading(line.trim()));
    let mut line = line.as_str();
    for mark in ["**", "__", "++", "~~", "`", "*", "_"] {
        while let Some(inner) = line.strip_prefix(mark).and_then(|l| l.strip_suffix(mark)) {
            line = inner;
        }
    }
    line.replace("**", "").replace("__", "").trim().to_owned()
}

/// `text` with `![alt](…)` removed and `[text](…)` reduced to its text.
fn strip_links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find(']').map(|c| open + c) else {
            break;
        };
        let end = rest[close..]
            .strip_prefix("](")
            .and_then(|t| t.find(')'))
            .map(|e| close + 2 + e);
        let Some(end) = end else {
            // `[` that starts no link, e.g. a checkbox.
            out.push_str(&rest[..=open]);
            rest = &rest[open + 1..];
            continue;
        };
        let image = rest[..open].ends_with('!');
        out.push_str(&rest[..if image { open - 1 } else { open }]);
        if !image {
            out.push_str(&rest[open + 1..close]);
        }
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

/// Letters and digits only, lowercase, list numbers left out: what the
/// text and a snippet are compared by.
fn plain(text: &str) -> String {
    text.lines()
        .map(|l| {
            let l = l.trim_start();
            let digits = l.len() - l.trim_start_matches(|c: char| c.is_ascii_digit()).len();
            match l[digits..].strip_prefix(". ") {
                Some(rest) if digits > 0 => rest,
                _ => l,
            }
        })
        .flat_map(str::chars)
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn common_prefix(a: &str, b: &str) -> usize {
    a.chars().zip(b.chars()).take_while(|(x, y)| x == y).count()
}

/// The link destinations in `text`: `](dest)`, `](dest "title")` and
/// `](<dest>)`.
fn destinations(text: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        let dest = if let Some(inner) = rest.strip_prefix('<') {
            inner.find('>').map(|e| &inner[..e])
        } else {
            rest.find([')', ' ']).map(|e| &rest[..e])
        };
        if let Some(dest) = dest.filter(|d| !d.is_empty() && !out.contains(d)) {
            out.push(dest);
        }
    }
    out
}

/// The attachments `file` links to: relative links into the export to
/// files that exist, and every link into `Attachments/` (one that is
/// missing is reported by the pipeline).
fn attachments(file: &Exported) -> Vec<ImportedAttachment> {
    destinations(&file.text)
        .into_iter()
        .filter(|d| !d.contains(':') && !d.starts_with(['/', '#']))
        .filter(|d| {
            Path::new(d)
                .components()
                .all(|c| matches!(c, Component::Normal(_) | Component::CurDir))
        })
        .filter(|d| d.starts_with("Attachments/") || file.dir.join(d).is_file())
        .map(|d| ImportedAttachment {
            link: d.to_owned(),
            file_name: d.rsplit('/').next().unwrap_or(d).to_owned(),
            data: AttachmentData::File(file.dir.join(d)),
        })
        .collect()
}

/// The export's text with its quirks tidied up (see the module docs).
fn tidy(text: &str, attachments: &[ImportedAttachment]) -> String {
    let text = text.replace("\r\n", "\n");
    if text.trim().is_empty() {
        return String::new();
    }
    let text = links_for_documents(&text, attachments);
    let ends_in_newline = text.ends_with('\n');
    let mut lines: Vec<(String, bool)> = Vec::new(); // (line, verbatim)
    let mut fence = false;
    for line in text.strip_suffix('\n').unwrap_or(&text).split('\n') {
        let is_fence = starts_fence(line);
        if fence || is_fence {
            fence ^= is_fence;
            lines.push((line.to_owned(), true));
            continue;
        }
        let content = line.trim_end_matches(' ');
        let trailing = &line[content.len()..];
        let content = content.trim_end_matches(['\u{2028}', ' ']);
        let mut parts = content.split('\u{2028}').peekable();
        while let Some(part) = parts.next() {
            if parts.peek().is_some() {
                lines.push((format!("{}  ", part.trim_end()), false));
            } else {
                lines.push((format!("{part}{trailing}"), false));
            }
        }
    }
    for i in 0..lines.len() {
        if lines[i].1 {
            continue;
        }
        let next = lines.get(i + 1).map(|(l, _)| l.as_str());
        if !breaks_paragraph(&lines[i].0, next) {
            let trimmed = lines[i].0.trim_end().len();
            lines[i].0.truncate(trimmed);
        }
    }
    // Blank lines at the end mean nothing (outside a code block).
    while lines
        .last()
        .is_some_and(|(l, verbatim)| !verbatim && l.is_empty())
    {
        lines.pop();
    }
    let mut out = lines
        .into_iter()
        .map(|(l, _)| l)
        .collect::<Vec<_>>()
        .join("\n");
    if ends_in_newline {
        out.push('\n');
    }
    out
}

/// `true` if `line` ends in a hard line break that joins it to `next` in
/// one paragraph, so it makes a difference.
fn breaks_paragraph(line: &str, next: Option<&str>) -> bool {
    let Some(mut next) = next else {
        return false;
    };
    let mut line = line;
    // Inside a quote, compare what follows the `>`.
    while let (Some(a), Some(b)) = (
        line.trim_start().strip_prefix('>'),
        next.trim_start().strip_prefix('>'),
    ) {
        line = a.strip_prefix(' ').unwrap_or(a);
        next = b.strip_prefix(' ').unwrap_or(b);
    }
    line.ends_with("  ")
        && !line.trim().is_empty()
        && strip_heading(line.trim()) == line.trim()
        && !next.trim().is_empty()
        && !starts_block(next)
}

/// `true` if `line` starts a Markdown block of its own.
fn starts_block(line: &str) -> bool {
    let t = line.trim_start();
    let digits = t.len() - t.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    starts_fence(t)
        || strip_heading(t) != t
        || ["- ", "* ", "+ ", ">", "|"]
            .iter()
            .any(|m| t.starts_with(m))
        || ["-", "*", "+"].contains(&t)
        || t.trim_end() == "---"
        || (digits > 0 && (t[digits..].starts_with(". ") || t[digits..].starts_with(") ")))
}

fn starts_fence(line: &str) -> bool {
    let t = line.trim_start();
    t.starts_with("```") || t.starts_with("~~~")
}

/// `![alt](dest)` → `[alt](dest)` for attachments that are not pictures.
fn links_for_documents(text: &str, attachments: &[ImportedAttachment]) -> String {
    let mut out = text.to_owned();
    for a in attachments {
        let ext = Path::new(&a.file_name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        if PICTURES.contains(&ext.as_str()) {
            continue;
        }
        let mut result = String::new();
        let mut rest = out.as_str();
        while let Some(open) = rest.find("![") {
            let tail = &rest[open + 1..];
            let is_this = tail.find("](").is_some_and(|c| {
                let dest = &tail[c + 2..];
                dest.strip_prefix('<')
                    .unwrap_or(dest)
                    .starts_with(a.link.as_str())
            });
            result.push_str(&rest[..open]);
            if !is_this {
                result.push('!');
            }
            rest = tail;
        }
        result.push_str(rest);
        out = result;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn titles_are_compared_without_marks() {
        let key = Key::new;
        assert_eq!(
            key("**Club history: the first ten years**"),
            key("Club history the first ten years")
        );
        assert_eq!(key("# Antenna  Tuner"), key("antenna tuner"));
        assert_eq!(
            key("[Trip planning 2026-03-14 09:30](https://example.com/x)"),
            key("Trip planning 2026-03-14 0930")
        );
        let shortened = key("Keyer firmware: changes for the \"bench\"\u{a0}…");
        assert!(shortened.shortened);
        assert!(shortened.matches(&key("Keyer firmware changes for the bench machine")));
        assert!(shortened.matches(&key("Keyer firmware changes for the bench …")));
        assert!(!shortened.matches(&key("Keyer firmware")));
        assert!(!key("").matches(&key("")));
    }

    #[test]
    fn display_titles() {
        assert_eq!(display_title("## Bread rolls"), "Bread rolls");
        assert_eq!(
            display_title("**Club history: ten years**"),
            "Club history: ten years"
        );
        assert_eq!(display_title("[Trip](https://example.com)"), "Trip");
        assert_eq!(display_title("#recipe"), "#recipe");
        assert_eq!(display_title(""), "");
    }

    #[test]
    fn hard_breaks_are_kept_only_inside_paragraphs() {
        let export = "# Title  \nfirst line  \nsecond line  \n  \n- [ ] a  \n- [ ] b  \n\n| a |\n\
                      | - |\n> quote  \n> on  \n> two lines  \n  \nend  \n";
        assert_eq!(
            tidy(export, &[]),
            "# Title\nfirst line  \nsecond line\n\n- [ ] a\n- [ ] b\n\n| a |\n| - |\n\
             > quote  \n> on  \n> two lines\n\nend\n"
        );
        assert_eq!(tidy("  \n", &[]), "");
        assert_eq!(tidy("a  \nb", &[]), "a  \nb");
        assert_eq!(tidy("# Lake trip  \n  \n\n", &[]), "# Lake trip\n");
        // Code blocks stay exactly as written.
        assert_eq!(tidy("```  \nx  \n```  \n", &[]), "```  \nx  \n```  \n");
    }

    #[test]
    fn line_separators() {
        assert_eq!(
            tidy("## Parts\u{2028}  \n\n| a |\n", &[]),
            "## Parts\n\n| a |\n"
        );
        assert_eq!(
            tidy("one\u{2028}two  \nthree  \n", &[]),
            "one  \ntwo  \nthree\n"
        );
    }

    #[test]
    fn documents_become_links() {
        let attachment = |link: &str| ImportedAttachment {
            link: link.to_owned(),
            file_name: link.rsplit('/').next().unwrap().to_owned(),
            data: AttachmentData::Bytes(Vec::new()),
        };
        let pdf = attachment("Attachments/A.pdf");
        let png = attachment("Attachments/B.png");
        assert_eq!(
            links_for_documents(
                "![PDF](Attachments/A.pdf) ![x](Attachments/B.png)",
                &[pdf, png]
            ),
            "[PDF](Attachments/A.pdf) ![x](Attachments/B.png)"
        );
    }

    #[test]
    fn snippets_tell_notes_apart() {
        let text = plain(&strip_links(
            "# Pizza dough  \n#recipe  \n\n1. Mix  \n![x](Attachments/a.png)",
        ));
        assert_eq!(text, "pizzadoughrecipemix");
        assert_eq!(
            strip_links("- [ ] Pack [the keyer](x) [y]"),
            "- [ ] Pack the keyer [y]"
        );
        assert!(
            common_prefix(&text, &plain("Pizza dough #recipe ￼ Mix"))
                > common_prefix(&text, &plain("Pizza dough #recipe Ingredients"))
        );
    }
}
