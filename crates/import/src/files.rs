// SPDX-License-Identifier: Apache-2.0
//! What the adapters for files on disk share (HTML, Markdown): finding the
//! files in the chosen folder, legacy text, and the local files a note's
//! links point to.

use std::fs;
use std::path::{Component, Path, PathBuf};
use std::time::UNIX_EPOCH;

use percent_encoding::percent_decode_str;
use unicode_normalization::UnicodeNormalization;

use crate::note::{AttachmentData, ImportedAttachment, Notice};
use crate::pipeline::ImportError;

/// Windows-1252 at 0x80–0x9F; the five unassigned bytes stay C1 controls.
const CP1252_HIGH: [char; 32] = [
    '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž', '\u{8f}',
    '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}', 'ž', 'Ÿ',
];

/// A file found for import.
pub(crate) struct Found {
    /// Its path relative to the chosen folder, with `/`, NFC: the
    /// `source_id` (format.md §12.1).
    pub path: String,
    /// The file on disk.
    pub file: PathBuf,
    /// The folder it is in on disk, for its links.
    pub dir: PathBuf,
    /// Its folders below the chosen folder.
    pub folders: Vec<String>,
    /// Its file name without the extension, NFC.
    pub stem: String,
    /// Its modification time.
    pub modified_ms: Option<u64>,
}

/// The files to import at `path`: the file itself, which must be `wanted`,
/// or the `wanted` files in the folder and its subfolders, sorted by path,
/// skipping hidden entries and folders for which `skip_dir` is `true`.
/// Returns the folder the paths are relative to, the files, and notices
/// about folders that cannot be read.
///
/// # Errors
///
/// [`ImportError::Source`] if the folder cannot be read, or if `path` is a
/// file that is not `wanted`; `kind` names the files wanted, e.g. "an HTML
/// file (.html or .htm)".
pub(crate) fn find(
    path: &Path,
    wanted: &dyn Fn(&Path) -> bool,
    skip_dir: &dyn Fn(&Path) -> bool,
    kind: &str,
) -> Result<(PathBuf, Vec<Found>, Vec<Notice>), ImportError> {
    let mut notices = Vec::new();
    if !path.is_dir() {
        let root = path.parent().map(Path::to_path_buf).unwrap_or_default();
        if !wanted(path) || !path.is_file() {
            return Err(ImportError::Source(format!(
                "{} is not {kind}.",
                display(path)
            )));
        }
        let found = found(&root, path);
        return Ok((root, vec![found], notices));
    }
    let top = fs::read_dir(path)
        .map_err(|e| ImportError::Source(format!("{} cannot be read: {e}", display(path))))?;
    let mut out = Vec::new();
    walk(path, top, wanted, skip_dir, &mut out, &mut notices);
    out.sort_by(|a, b| a.path.cmp(&b.path));
    Ok((path.to_path_buf(), out, notices))
}

fn walk(
    root: &Path,
    entries: fs::ReadDir,
    wanted: &dyn Fn(&Path) -> bool,
    skip_dir: &dyn Fn(&Path) -> bool,
    out: &mut Vec<Found>,
    notices: &mut Vec<Notice>,
) {
    let mut entries: Vec<_> = entries.filter_map(Result::ok).map(|e| e.path()).collect();
    entries.sort();
    for path in entries {
        let hidden = path
            .file_name()
            .is_none_or(|n| n.to_string_lossy().starts_with('.'));
        if hidden {
            continue;
        }
        if path.is_dir() {
            if skip_dir(&path) {
                continue;
            }
            match fs::read_dir(&path) {
                Ok(inner) => walk(root, inner, wanted, skip_dir, out, notices),
                Err(e) => notices.push(Notice::general(format!(
                    "{} cannot be read: {e}",
                    relative(root, &path)
                ))),
            }
        } else if wanted(&path) && path.is_file() {
            out.push(found(root, &path));
        }
    }
}

fn found(root: &Path, path: &Path) -> Found {
    let rel = relative(root, path);
    let mut folders: Vec<String> = rel.split('/').map(str::to_owned).collect();
    folders.pop();
    let modified_ms = fs::metadata(path)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .and_then(|d| u64::try_from(d.as_millis()).ok());
    Found {
        path: rel,
        file: path.to_path_buf(),
        dir: path.parent().map(Path::to_path_buf).unwrap_or_default(),
        folders,
        stem: path
            .file_stem()
            .map(|s| s.to_string_lossy().nfc().collect())
            .unwrap_or_default(),
        modified_ms,
    }
}

/// `true` if `path` has one of `extensions`, ignoring case.
pub(crate) fn has_extension(path: &Path, extensions: &[&str]) -> bool {
    path.extension()
        .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// `path` relative to `root`, with `/`, NFC.
pub(crate) fn relative(root: &Path, path: &Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
        .nfc()
        .collect()
}

/// The file or folder name of `path`, NFC, for the report and the log.
pub(crate) fn display(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().nfc().collect(),
    )
}

/// `bytes` read as Windows-1252, which browsers also use for ISO-8859-1.
pub(crate) fn windows_1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|&b| match b {
            0x80..=0x9f => CP1252_HIGH[usize::from(b - 0x80)],
            _ => char::from(b),
        })
        .collect()
}

/// The text of the level-1 heading the Markdown starts with (format.md §5.2).
pub(crate) fn leading_heading(markdown: &str) -> Option<&str> {
    let first = markdown.lines().find(|l| !l.trim().is_empty())?;
    let text = first.strip_prefix("# ")?.trim();
    (!text.is_empty()).then_some(text)
}

/// The link destinations in `markdown` as written: `](dest)`,
/// `](dest "title")` and `](<dest>)`, with `\(` and `\)` kept.
pub(crate) fn destinations(markdown: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut rest = markdown;
    while let Some(at) = rest.find("](") {
        rest = &rest[at + 2..];
        let dest = if let Some(inner) = rest.strip_prefix('<') {
            inner.find('>').map(|e| &inner[..e])
        } else {
            let mut end = None;
            let mut escaped = false;
            for (i, c) in rest.char_indices() {
                match c {
                    _ if escaped => escaped = false,
                    '\\' => escaped = true,
                    ')' | ' ' | '\n' => {
                        end = Some(i);
                        break;
                    }
                    _ => {}
                }
            }
            end.map(|e| &rest[..e])
        };
        if let Some(dest) = dest.filter(|d| !d.is_empty() && !out.contains(d)) {
            out.push(dest);
        }
    }
    out
}

/// The local files a note in `dir` links to in `markdown` that exist below
/// `root` and are not notes themselves (`is_note`), as attachments. Links to
/// files that are missing go into `problems`.
pub(crate) fn linked_files(
    root: &Path,
    dir: &Path,
    markdown: &str,
    is_note: &dyn Fn(&Path) -> bool,
    problems: &mut Vec<String>,
) -> Vec<ImportedAttachment> {
    let mut out = Vec::new();
    for dest in destinations(markdown) {
        let Some(file) = local_file(root, dir, dest) else {
            continue;
        };
        if is_note(&file) {
            continue;
        }
        if !file.is_file() {
            problems.push(format!(
                "It links to {}, which is not there; the link is left as it is.",
                relative(root, &file)
            ));
            continue;
        }
        let name: String = file
            .file_name()
            .map(|n| n.to_string_lossy().nfc().collect())
            .unwrap_or_default();
        out.push(ImportedAttachment {
            link: dest.to_owned(),
            file_name: name,
            data: AttachmentData::File(file),
        });
    }
    out
}

/// The file a relative link `dest` from a note in `dir` points to, if it
/// stays below `root`; `None` for links to the web, anchors and absolute
/// paths.
pub(crate) fn local_file(root: &Path, dir: &Path, dest: &str) -> Option<PathBuf> {
    if dest.contains(':') || dest.starts_with(['/', '#', '?']) {
        return None;
    }
    let dest = dest.replace("\\(", "(").replace("\\)", ")");
    let dest = dest.split(['#', '?']).next().unwrap_or_default();
    let dest = percent_decode_str(dest).decode_utf8().ok()?;
    let mut path = dir.to_path_buf();
    for component in Path::new(&*dest).components() {
        match component {
            Component::Normal(part) => path.push(part),
            Component::CurDir => {}
            Component::ParentDir => {
                if !path.pop() {
                    return None;
                }
            }
            Component::RootDir | Component::Prefix(_) => return None,
        }
    }
    (path.starts_with(root) && path != root).then_some(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_text() {
        assert_eq!(windows_1252(b"Gr\xfc\xdfe \x80 \x93x\x94"), "Grüße € “x”");
    }

    #[test]
    fn headings() {
        assert_eq!(leading_heading("\n# A \\*b\\*\n\ntext"), Some("A \\*b\\*"));
        assert_eq!(leading_heading("## A"), None);
        assert_eq!(leading_heading("#tag\n"), None);
    }

    #[test]
    fn link_destinations() {
        assert_eq!(
            destinations(
                "![a](x\\(1\\).png) [b](<my file.pdf>) [c](d.pdf \"t\") [e](x\\(1\\).png)"
            ),
            ["x\\(1\\).png", "my file.pdf", "d.pdf"]
        );
    }

    #[test]
    fn local_files() {
        let root = Path::new("/r");
        let dir = Path::new("/r/a");
        let file = |d| local_file(root, dir, d);
        assert_eq!(file("p%20q.png#x"), Some(PathBuf::from("/r/a/p q.png")));
        assert_eq!(
            file("../b/x\\(1\\).png"),
            Some(PathBuf::from("/r/b/x(1).png"))
        );
        assert_eq!(file("../../etc/passwd"), None);
        for dest in [
            "https://x.org/a.png",
            "/abs.png",
            "#top",
            "mailto:a@b",
            "..",
        ] {
            assert_eq!(file(dest), None, "{dest}");
        }
    }
}
