// SPDX-License-Identifier: Apache-2.0
//! The HTML adapter (ADR-0012, ADR-0019, format.md §12.1): one `.html` or
//! `.htm` file, or a folder with such files at any depth. Each file becomes
//! one note; its folders below the chosen folder are its folders.
//!
//! The converter is [`htmd`] (Apache-2.0), with a few rules of its own:
//!
//! - The title is the page's first heading if the page starts with a level-1
//!   heading. Otherwise a `<title>` becomes that heading, so the note shows
//!   the title it is named by (format.md §5.2). A page with neither gets its
//!   title from its first line, an empty one from its file name.
//! - Pictures embedded as `data:` URLs and local files linked by pictures or
//!   links (`Page_files/photo.jpg`, `manual.pdf`) become attachments
//!   (format.md §6). Links to other pages, to the web and to files outside
//!   the chosen folder stay as they are.
//! - `<s>`, `<del>` and `<strike>` become `~~…~~`; checkboxes `[x]` and
//!   `[ ]`; `<hr>` `---`. Scripts, styles and other form fields are left
//!   out.
//! - Text that is not UTF-8 is read as Windows-1252 (which covers
//!   ISO-8859-1) unless the page declares another encoding; such a page is
//!   not imported, and the report says so.
//!
//! The dates are the file's modification time, so a re-import updates a note
//! whose file changed since (§12.3).

use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use htmd::element_handler::{HandlerResult, Handlers};
use htmd::options::{BulletListMarker, Options};
use htmd::{Element, HtmlToMarkdown};
use percent_encoding::percent_decode_str;
use schnipsle_core::stamp::rfc3339_utc;

use crate::files::{self, Found};
use crate::note::{AttachmentData, Batch, ImportedAttachment, ImportedNote, Notice, Source};
use crate::pipeline::ImportError;

/// The extensions of the files read.
const EXTENSIONS: &[&str] = &["html", "htm"];

/// What browsers append to a saved page's name for the folder of its
/// pictures and scripts (English and German); not searched for pages.
const ASSET_FOLDERS: &[&str] = &["_files", "-Dateien"];

/// Picture types a `data:` URL can hold, with the extension they are
/// stored with.
const DATA_TYPES: &[(&str, &str)] = &[
    ("image/png", "png"),
    ("image/jpeg", "jpg"),
    ("image/jpg", "jpg"),
    ("image/gif", "gif"),
    ("image/webp", "webp"),
    ("image/svg+xml", "svg"),
    ("image/avif", "avif"),
    ("image/bmp", "bmp"),
    ("image/tiff", "tif"),
    ("image/heic", "heic"),
];

/// Encodings read as Windows-1252, as browsers do (WHATWG Encoding).
const WINDOWS_1252: &[&str] = &[
    "windows-1252",
    "cp1252",
    "iso-8859-1",
    "iso8859-1",
    "latin1",
    "l1",
    "us-ascii",
    "ascii",
];

/// Reads the HTML file at `path`, or the HTML files in the folder at `path`
/// and its subfolders, into a batch for the pipeline.
///
/// # Errors
///
/// [`ImportError::Source`] if `path` cannot be read or is a file that is not
/// HTML. Pages that cannot be read or decoded are notices.
pub fn read(path: &Path) -> Result<Batch, ImportError> {
    let (root, pages, mut notices) = files::find(
        path,
        &is_html,
        &is_asset_folder,
        "an HTML file (.html or .htm)",
    )?;
    let mut notes = Vec::new();
    for found in &pages {
        let bytes = match fs::read(&found.file) {
            Ok(bytes) => bytes,
            Err(e) => {
                notices.push(Notice::general(format!(
                    "{} cannot be read and is not imported: {e}",
                    found.path
                )));
                continue;
            }
        };
        let html = match decode(&bytes) {
            Decoded::Text(html) => html,
            Decoded::Lossy(html) => {
                notices.push(Notice::general(format!(
                    "{} says it is UTF-8 but is not quite; the bytes that are not are shown as �.",
                    found.path
                )));
                html
            }
            Decoded::Unsupported(charset) => {
                notices.push(Notice::general(format!(
                    "{} is in the encoding {charset}, which the import cannot read; it is not \
                     imported.",
                    found.path
                )));
                continue;
            }
        };
        notes.push(note(&root, found, &html, &mut notices));
    }
    Ok(Batch {
        source: Source::Html,
        input: files::display(path),
        notes,
        notices,
    })
}

fn is_html(path: &Path) -> bool {
    files::has_extension(path, EXTENSIONS)
}

/// `true` for the folder a browser saved a page's pictures in: `Page_files`
/// next to `Page.html`.
fn is_asset_folder(dir: &Path) -> bool {
    let (Some(parent), Some(name)) = (dir.parent(), dir.file_name()) else {
        return false;
    };
    let name = name.to_string_lossy();
    ASSET_FOLDERS.iter().any(|suffix| {
        name.strip_suffix(suffix).is_some_and(|stem| {
            EXTENSIONS
                .iter()
                .any(|ext| parent.join(format!("{stem}.{ext}")).is_file())
        })
    })
}

/// The note made from `page`, whose text is `html`.
fn note(root: &Path, page: &Found, html: &str, notices: &mut Vec<Notice>) -> ImportedNote {
    let converted = convert(html);
    let mut problems = converted.problems;
    let mut attachments = converted.attachments;
    let mut body = converted.markdown;
    attachments.extend(files::linked_files(
        root,
        &page.dir,
        &body,
        &is_html,
        &mut problems,
    ));

    let mut title = String::new();
    if let Some(heading) = leading_heading(&body) {
        title = unescape(heading);
    } else if let Some(page_title) = converted.title {
        title = unescape(&page_title);
        body = if body.is_empty() {
            format!("# {page_title}")
        } else {
            format!("# {page_title}\n\n{body}")
        };
    } else if body.is_empty() {
        title.clone_from(&page.stem);
    }
    if !body.is_empty() {
        body.push('\n');
    }
    let shown = if title.is_empty() {
        schnipsle_core::vault::title_of(&body, &page.stem)
    } else {
        title.clone()
    };
    notices.extend(problems.into_iter().map(|message| Notice {
        source_id: Some(page.path.clone()),
        title: Some(shown.clone()),
        message,
    }));
    let time = page.modified_ms.map(rfc3339_utc);
    ImportedNote {
        source_id: page.path.clone(),
        title,
        body,
        folders: page.folders.clone(),
        created: time.clone(),
        modified: time,
        attachments,
        ..ImportedNote::default()
    }
}

/// The text of a page, how it was decoded.
enum Decoded {
    Text(String),
    /// Declared UTF-8, with bytes that are not.
    Lossy(String),
    /// An encoding this adapter cannot read.
    Unsupported(String),
}

/// The page's bytes as text: a byte order mark decides, then whether it is
/// UTF-8, then the encoding the page declares (see the module docs).
fn decode(bytes: &[u8]) -> Decoded {
    if let Some(rest) = bytes.strip_prefix(b"\xef\xbb\xbf") {
        return utf8(rest);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xff\xfe") {
        return utf16(rest, u16::from_le_bytes);
    }
    if let Some(rest) = bytes.strip_prefix(b"\xfe\xff") {
        return utf16(rest, u16::from_be_bytes);
    }
    if let Ok(text) = std::str::from_utf8(bytes) {
        return Decoded::Text(text.to_owned());
    }
    match declared_charset(bytes) {
        Some(charset) if charset == "utf-8" || charset == "utf8" => utf8(bytes),
        Some(charset) if !WINDOWS_1252.contains(&charset.as_str()) => Decoded::Unsupported(charset),
        _ => Decoded::Text(files::windows_1252(bytes)),
    }
}

fn utf8(bytes: &[u8]) -> Decoded {
    match String::from_utf8(bytes.to_vec()) {
        Ok(text) => Decoded::Text(text),
        Err(e) => Decoded::Lossy(String::from_utf8_lossy(e.as_bytes()).into_owned()),
    }
}

fn utf16(bytes: &[u8], unit: fn([u8; 2]) -> u16) -> Decoded {
    let units: Vec<u16> = bytes.chunks_exact(2).map(|c| unit([c[0], c[1]])).collect();
    match String::from_utf16(&units) {
        Ok(text) => Decoded::Text(text),
        Err(_) => Decoded::Lossy(String::from_utf16_lossy(&units)),
    }
}

/// The `charset` a page declares in its first 1024 bytes, lowercase:
/// `<meta charset="…">` or `content="text/html; charset=…"`.
fn declared_charset(bytes: &[u8]) -> Option<String> {
    let head = &bytes[..bytes.len().min(1024)];
    let head: String = head
        .iter()
        .map(|&b| char::from(b).to_ascii_lowercase())
        .collect();
    let at = head.find("charset=")? + "charset=".len();
    let value: String = head[at..]
        .trim_start_matches(['"', '\'', ' '])
        .chars()
        .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | ':'))
        .collect();
    (!value.is_empty()).then_some(value)
}

/// What [`convert`] makes of a page.
struct Converted {
    /// The Markdown, without a final line break.
    markdown: String,
    /// The text of `<title>`, Markdown-escaped, whitespace collapsed.
    title: Option<String>,
    /// Pictures embedded as `data:` URLs.
    attachments: Vec<ImportedAttachment>,
    /// What the report should say about the page.
    problems: Vec<String>,
}

/// What the handlers collect while converting a page.
#[derive(Default)]
struct Collected {
    title: Option<String>,
    attachments: Vec<ImportedAttachment>,
    problems: Vec<String>,
}

/// Converts `html` to Markdown (see the module docs).
fn convert(html: &str) -> Converted {
    let collected = Arc::new(Mutex::new(Collected::default()));
    let converter = converter(&collected);
    let markdown = converter.convert(html).unwrap_or_default();
    drop(converter);
    let collected = Arc::try_unwrap(collected).map_or_else(
        |shared| std::mem::take(&mut *lock(&shared)),
        |m| m.into_inner().unwrap_or_else(PoisonError::into_inner),
    );
    Converted {
        markdown: tidy(&markdown),
        title: collected.title,
        attachments: collected.attachments,
        problems: collected.problems,
    }
}

fn lock(shared: &Mutex<Collected>) -> std::sync::MutexGuard<'_, Collected> {
    shared.lock().unwrap_or_else(PoisonError::into_inner)
}

/// The converter, with handlers that write into `collected`.
fn converter(collected: &Arc<Mutex<Collected>>) -> HtmlToMarkdown {
    let for_title = Arc::clone(collected);
    let for_images = Arc::clone(collected);
    HtmlToMarkdown::builder()
        .options(Options {
            bullet_list_marker: BulletListMarker::Dash,
            ul_bullet_spacing: 1,
            ol_number_spacing: 1,
            ..Options::default()
        })
        .skip_tags(vec!["script", "style", "noscript", "template"])
        .add_handler(
            vec!["title"],
            move |handlers: &dyn Handlers, element: Element| {
                let text = handlers.walk_children(element.node).content;
                let text = text.split_whitespace().collect::<Vec<_>>().join(" ");
                let mut collected = lock(&for_title);
                if collected.title.is_none() && !text.is_empty() {
                    collected.title = Some(text);
                }
                Some(HandlerResult::from(""))
            },
        )
        .add_handler(
            vec!["img"],
            move |handlers: &dyn Handlers, element: Element| {
                let src = attr(&element, "src").unwrap_or_default();
                if !src
                    .trim_start()
                    .get(..5)
                    .is_some_and(|s| s.eq_ignore_ascii_case("data:"))
                {
                    return handlers.fallback(element);
                }
                let alt = attr(&element, "alt")
                    .map(|a| a.split_whitespace().collect::<Vec<_>>().join(" "))
                    .unwrap_or_default()
                    .replace(['[', ']'], "");
                let mut collected = lock(&for_images);
                let Some((bytes, ext)) = data_url(src.trim()) else {
                    collected.problems.push(
                        "A picture embedded in the page cannot be read; it is left out.".to_owned(),
                    );
                    return Some(HandlerResult::from(alt));
                };
                let n = collected.attachments.len() + 1;
                // A name no real link has, replaced by the stored file's path.
                let link = format!("data:{n}");
                collected.attachments.push(ImportedAttachment {
                    link: link.clone(),
                    file_name: format!("image-{n}.{ext}"),
                    data: AttachmentData::Bytes(bytes),
                });
                Some(HandlerResult::from(format!("![{alt}]({link})")))
            },
        )
        .add_handler(
            vec!["s", "del", "strike"],
            |handlers: &dyn Handlers, element: Element| {
                let content = handlers.walk_children(element.node).content;
                let trimmed = content.trim();
                if trimmed.is_empty() {
                    return Some(HandlerResult::from(content));
                }
                let start = &content[..content.len() - content.trim_start().len()];
                let end = &content[content.trim_end().len()..];
                Some(HandlerResult::from(format!("{start}~~{trimmed}~~{end}")))
            },
        )
        .add_handler(vec!["input"], |_: &dyn Handlers, element: Element| {
            let checkbox =
                attr(&element, "type").is_some_and(|t| t.eq_ignore_ascii_case("checkbox"));
            if !checkbox {
                return Some(HandlerResult::from(""));
            }
            let mark = if attr(&element, "checked").is_some() {
                "[x] "
            } else {
                "[ ] "
            };
            Some(HandlerResult::from(mark))
        })
        .add_handler(vec!["hr"], |_: &dyn Handlers, _: Element| {
            Some(HandlerResult::from("\n\n---\n\n"))
        })
        .build()
}

/// The value of the attribute `name` of `element`.
fn attr(element: &Element<'_>, name: &str) -> Option<String> {
    element
        .attrs
        .iter()
        .find(|a| &*a.name.local == name)
        .map(|a| a.value.to_string())
}

/// The content and file extension of a `data:` URL holding a picture.
fn data_url(url: &str) -> Option<(Vec<u8>, &'static str)> {
    let rest = url.get(5..)?;
    let (meta, data) = rest.split_once(',')?;
    let mut parts = meta.split(';').map(str::trim);
    let mime = parts.next().unwrap_or_default().to_ascii_lowercase();
    let base64 = parts.any(|p| p.eq_ignore_ascii_case("base64"));
    let ext = DATA_TYPES
        .iter()
        .find(|(m, _)| *m == mime)
        .map(|(_, ext)| *ext)?;
    let bytes: Vec<u8> = percent_decode_str(data).collect();
    let bytes = if base64 {
        let clean: Vec<u8> = bytes
            .into_iter()
            .filter(|b| !b.is_ascii_whitespace())
            .collect();
        base64::engine::general_purpose::GeneralPurpose::new(
            &base64::alphabet::STANDARD,
            base64::engine::GeneralPurposeConfig::new()
                .with_decode_padding_mode(base64::engine::DecodePaddingMode::Indifferent),
        )
        .decode(clean)
        .ok()?
    } else {
        bytes
    };
    (!bytes.is_empty()).then_some((bytes, ext))
}

/// The converter's output with trailing spaces off lines that do not end in
/// a hard line break, and at most one empty line in a row.
fn tidy(markdown: &str) -> String {
    let mut out = String::new();
    let mut fence = false;
    let mut empty = 0;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            fence = !fence;
        }
        let line = if fence || line.ends_with("  ") && !line.trim().is_empty() {
            line
        } else {
            line.trim_end()
        };
        if line.is_empty() && !fence {
            empty += 1;
            if empty > 1 {
                continue;
            }
        } else {
            empty = 0;
        }
        out.push_str(line);
        out.push('\n');
    }
    out.trim_matches('\n').to_owned()
}

/// The text of the level-1 heading the Markdown starts with.
fn leading_heading(markdown: &str) -> Option<&str> {
    let first = markdown.lines().find(|l| !l.trim().is_empty())?;
    let text = first.strip_prefix("# ")?.trim();
    (!text.is_empty()).then_some(text)
}

/// Markdown text without its backslash escapes.
fn unescape(text: &str) -> String {
    let mut out = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' && chars.peek().is_some_and(char::is_ascii_punctuation) {
            continue;
        }
        out.push(c);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn markdown(html: &str) -> String {
        convert(html).markdown
    }

    #[test]
    fn the_basics() {
        assert_eq!(
            markdown(
                "<h1>Pizza</h1><p>Some <b>bold</b>, <i>italic</i> and <s>old</s> text, #tag.</p>\
                 <ul><li>one</li><li>two<ul><li>nested</li></ul></li></ul><ol><li>first</li></ol>\
                 <hr><p>a<br>b</p><script>x()</script><style>p{}</style>"
            ),
            "# Pizza\n\nSome **bold**, *italic* and ~~old~~ text, #tag.\n\n- one\n- two\n  - nested\n\n1. first\n\n---\n\na  \nb"
        );
    }

    #[test]
    fn checkboxes() {
        assert_eq!(
            markdown(
                "<ul><li><input type=checkbox checked> done</li><li><input type=checkbox> open</li></ul>"
            ),
            "- [x] done\n- [ ] open"
        );
    }

    #[test]
    fn title_and_data_pictures() {
        let page = convert(
            "<html><head><title> Lake\n  *trip* </title></head><body><p><img alt=\"a [dot]\" \
             src=\"data:image/png;base64,iVBORw0K Gg\"><img src=\"data:text/plain,x\">\
             <img src=\"data:image/svg+xml,%3Csvg%2F%3E\"></p></body></html>",
        );
        assert_eq!(page.title.as_deref(), Some("Lake \\*trip\\*"));
        assert_eq!(page.markdown, "![a dot](data:1)![](data:2)");
        let [png, svg] = &page.attachments[..] else {
            panic!("{:?}", page.attachments);
        };
        assert_eq!(png.file_name, "image-1.png");
        assert!(matches!(&png.data, AttachmentData::Bytes(b) if b.starts_with(b"\x89PNG")));
        assert!(matches!(&svg.data, AttachmentData::Bytes(b) if b == b"<svg/>"));
        assert_eq!(page.problems.len(), 1);
    }

    #[test]
    fn encodings() {
        assert!(matches!(decode("Grüße".as_bytes()), Decoded::Text(t) if t == "Grüße"));
        assert!(matches!(decode(b"\xef\xbb\xbfok"), Decoded::Text(t) if t == "ok"));
        assert!(matches!(decode(b"\xff\xfeo\0k\0"), Decoded::Text(t) if t == "ok"));
        assert!(
            matches!(decode(b"Gr\xfc\xdfe \x80 \x93x\x94"), Decoded::Text(t) if t == "Grüße € “x”")
        );
        assert!(matches!(
            decode(b"<meta charset=\"ISO-8859-1\">\xe9"),
            Decoded::Text(t) if t.ends_with('é')
        ));
        assert!(matches!(
            decode(b"<meta http-equiv=content-type content='text/html; charset=Shift_JIS'>\x82\xa0"),
            Decoded::Unsupported(c) if c == "shift_jis"
        ));
        assert!(
            matches!(decode(b"<meta charset=utf-8>\xe9"), Decoded::Lossy(t) if t.ends_with('�'))
        );
    }

    #[test]
    fn headings_and_escapes() {
        assert_eq!(leading_heading("\n# A \\*b\\*\n\ntext"), Some("A \\*b\\*"));
        assert_eq!(leading_heading("## A"), None);
        assert_eq!(unescape("A \\*b\\* \\ c"), "A *b* \\ c");
    }

    #[test]
    fn tidy_keeps_hard_breaks_and_code() {
        assert_eq!(
            tidy("a  \nb \n\n\n\nc\n```\nx  \n\n\n```"),
            "a  \nb\n\nc\n```\nx  \n\n\n```"
        );
    }
}
