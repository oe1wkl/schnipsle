// SPDX-License-Identifier: Apache-2.0
//! ODT export (ADR-0010): the XML of [`crate::odt_content`] with its styles,
//! metadata and pictures, zipped as an `OpenDocument` package. Nothing else is
//! needed, so it works on every platform.

use std::fmt::Write as _;
use std::fs;
use std::io::{self, Cursor, Write as _};
use std::path::Path;

use comrak::Arena;
use schnipsle_core::names::slug;
use schnipsle_core::stamp::rfc3339_utc;
use schnipsle_core::vault::{Result, Vault};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::document::{self, Collected, Rules, Source};
use crate::odt_content::{Content, Picture, escape};
use crate::output::{self, Names};
use crate::{Exporter, Format, Input, Layout, Options, Paper, Report};

const STYLES: &str = include_str!("odt_styles.xml");

const MEDIA_TYPE: &str = "application/vnd.oasis.opendocument.text";

/// The margin of a page, as in `odt_styles.xml`, in cm.
const MARGIN: f64 = 2.0;

/// Levels of a list that have their own bullet and indent.
const LIST_LEVELS: usize = 6;

const BULLETS: [&str; 3] = ["•", "◦", "▪"];

/// Exports notes as ODT.
#[derive(Debug, Clone, Copy, Default)]
pub struct OdtExporter;

impl Exporter for OdtExporter {
    fn format(&self) -> Format {
        Format::Odt
    }

    fn export(&self, vault: &Vault, input: &Input, options: &Options, to: &Path) -> Result<Report> {
        let mut report = Report::default();
        let notes = document::read_notes(vault, &input.notes, &mut report);
        if notes.is_empty() {
            return Ok(report);
        }
        let mut collected = Collected::default();
        match options.layout {
            Layout::FilePerNote => {
                file_per_note(vault, &notes, *options, to, &mut report, &mut collected)?;
            }
            Layout::SingleFile | Layout::Folder => {
                let name = one_document(vault, &notes, input, *options, to, &mut collected)?;
                report.files.extend(name);
            }
        }
        report.problems.append(&mut collected.problems);
        report.numbered.append(&mut collected.numbered);
        Ok(report)
    }
}

/// One file of all `notes`, and its file name. Attachments never come along
/// (ADR-0021 rule 6).
fn one_document(
    vault: &Vault,
    notes: &[Source],
    input: &Input,
    options: Options,
    to: &Path,
    collected: &mut Collected,
) -> Result<Option<String>> {
    let (anchors, mut rules) = document::rules_of_one_document(vault, notes, options, false, false);
    rules.upright_pictures = true;
    let contents = document::has_contents(options, notes);
    let pages = Pages {
        paper: options.paper,
        numbered: document::numbers_pages(options, notes),
        contents,
    };
    let mut content = Content::new(text_size(options.paper));
    if contents {
        let entries: Vec<(&str, &str)> = notes
            .iter()
            .zip(&anchors)
            .map(|(note, anchor)| (note.title.as_str(), anchor.as_str()))
            .collect();
        content.contents(&entries);
    }
    for (index, (note, anchor)) in notes.iter().zip(&anchors).enumerate() {
        let arena = Arena::new();
        let root = document::prepare(&arena, note, anchor, &rules, collected);
        content.note(root, anchor, index > 0 || contents);
    }
    let (title, note) = document::title_of_one_document(notes, input);
    let odt = package(&content, &title, note, pages)?;
    if options.layout == Layout::SingleFile {
        output::replace_file(to, &odt)?;
        return Ok(to.file_name().map(|n| n.to_string_lossy().into_owned()));
    }
    fs::create_dir_all(to)?;
    let mut names = Names::new(to);
    let name = names.free(&title, "odt");
    collected.numbered.append(&mut names.numbered);
    output::write_new(&to.join(&name), &odt)?;
    Ok(Some(name))
}

/// A file per note, linking to each other. A link in an `OpenDocument` package
/// starts inside the package, so the other file is one level up (ADR-0021 rule 7).
fn file_per_note(
    vault: &Vault,
    notes: &[Source],
    options: Options,
    to: &Path,
    report: &mut Report,
    collected: &mut Collected,
) -> Result<()> {
    fs::create_dir_all(to)?;
    let mut names = Names::new(to);
    let files: Vec<String> = notes
        .iter()
        .map(|note| names.free(&note.title, "odt"))
        .collect();
    collected.numbered.append(&mut names.numbered);
    let rules = Rules {
        vault,
        hrefs: notes
            .iter()
            .zip(&files)
            .map(|(note, name)| (note.path.clone(), format!("../{}", document::href_of(name))))
            .collect(),
        titles: document::titles_of(notes),
        out_of_reach: std::collections::HashSet::new(),
        copy_attachments: false,
        web_pictures: false,
        keep_tag_line: options.keep_tag_line,
        sections: false,
        upright_pictures: true,
    };
    let pages = Pages {
        paper: options.paper,
        numbered: options.page_numbers,
        contents: false,
    };
    for (note, name) in notes.iter().zip(files) {
        let anchor = format!("note-{}", slug(&note.title));
        let arena = Arena::new();
        let root = document::prepare(&arena, note, &anchor, &rules, collected);
        let mut content = Content::new(text_size(options.paper));
        content.note(root, &anchor, false);
        let odt = package(&content, &note.title, Some(note), pages)?;
        output::write_new(&to.join(&name), &odt)?;
        report.files.push(name);
    }
    Ok(())
}

/// How the pages of a document look.
#[derive(Debug, Clone, Copy)]
struct Pages {
    paper: Paper,
    /// A page number at the foot of every page.
    numbered: bool,
    /// The document starts with a table of contents.
    contents: bool,
}

/// Width and height of the paper in cm.
fn paper_size(paper: Paper) -> (f64, f64) {
    match paper {
        Paper::A4 => (21.0, 29.7),
        Paper::Letter => (21.59, 27.94),
    }
}

fn text_size(paper: Paper) -> (f64, f64) {
    let (width, height) = paper_size(paper);
    (width - 2.0 * MARGIN, height - 2.0 * MARGIN)
}

/// The zipped file. Its entries carry no time, so an export does not differ by when it ran.
fn package(
    content: &Content,
    title: &str,
    note: Option<&Source>,
    pages: Pages,
) -> io::Result<Vec<u8>> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    let deflated = SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
    // The file's type comes first and unpacked, so others can read it at a fixed place.
    zip.start_file("mimetype", stored)
        .map_err(io::Error::other)?;
    zip.write_all(MEDIA_TYPE.as_bytes())?;
    let parts = [
        ("content.xml", content_xml(&content.body, pages.paper)),
        ("styles.xml", styles_xml(pages)),
        ("meta.xml", meta_xml(title, note)),
        ("META-INF/manifest.xml", manifest_xml(&content.pictures)),
    ];
    for (name, xml) in parts {
        zip.start_file(name, deflated).map_err(io::Error::other)?;
        zip.write_all(xml.as_bytes())?;
    }
    for picture in &content.pictures {
        zip.start_file(picture.path.as_str(), stored)
            .map_err(io::Error::other)?;
        zip.write_all(&picture.bytes)?;
    }
    Ok(zip.finish().map_err(io::Error::other)?.into_inner())
}

fn content_xml(body: &str, paper: Paper) -> String {
    let (width, _) = text_size(paper);
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-content xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:style=\"urn:oasis:names:tc:opendocument:xmlns:style:1.0\" xmlns:fo=\"urn:oasis:names:tc:opendocument:xmlns:xsl-fo-compatible:1.0\" xmlns:text=\"urn:oasis:names:tc:opendocument:xmlns:text:1.0\" xmlns:table=\"urn:oasis:names:tc:opendocument:xmlns:table:1.0\" xmlns:draw=\"urn:oasis:names:tc:opendocument:xmlns:drawing:1.0\" xmlns:svg=\"urn:oasis:names:tc:opendocument:xmlns:svg-compatible:1.0\" xmlns:xlink=\"http://www.w3.org/1999/xlink\" office:version=\"1.2\">\n<office:automatic-styles>\n<style:style style:name=\"Table\" style:family=\"table\"><style:table-properties style:width=\"{width}cm\" table:align=\"margins\" fo:margin-top=\"0.1cm\" fo:margin-bottom=\"0.3cm\"/></style:style>\n<style:style style:name=\"Row\" style:family=\"table-row\"><style:table-row-properties fo:keep-together=\"always\"/></style:style>\n<style:style style:name=\"Cell\" style:family=\"table-cell\"><style:table-cell-properties fo:padding=\"0.1cm\" fo:border=\"0.5pt solid #d2d2d7\"/></style:style>\n<style:style style:name=\"Heading_20_Cell\" style:family=\"table-cell\"><style:table-cell-properties fo:padding=\"0.1cm\" fo:border=\"0.5pt solid #d2d2d7\" fo:background-color=\"#f5f5f7\"/></style:style>\n</office:automatic-styles>\n<office:body>\n<office:text>\n{body}</office:text>\n</office:body>\n</office:document-content>\n"
    )
}

fn styles_xml(pages: Pages) -> String {
    let mut styles = list_styles();
    if pages.numbered {
        styles.push('\n');
        styles.push_str(FOOTER_STYLE);
    }
    if pages.contents {
        let (width, _) = text_size(pages.paper);
        styles.push('\n');
        styles.push_str(&CONTENTS_STYLES.replace("{TEXT_WIDTH}", &format!("{width:.2}")));
    }
    STYLES
        .replace("{LIST_STYLES}", &styles)
        .replace("{PAGE_LAYOUT}", &page_layout(pages))
        .replace("{MASTER_PAGE}", &master_page(pages.numbered))
}

/// A page number, centred and small, at the foot of a page.
const FOOTER_STYLE: &str = "<style:style style:name=\"Footer\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:class=\"extra\"><style:paragraph-properties fo:text-align=\"center\"/><style:text-properties fo:font-size=\"9pt\" fo:color=\"#6e6e73\"/></style:style>";

/// The heading of a table of contents and its entries, whose page numbers stand
/// at the right margin after a row of dots, and their links, which look like text.
const CONTENTS_STYLES: &str = "<style:style style:name=\"Contents_20_Heading\" style:display-name=\"Contents Heading\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:next-style-name=\"Contents_20_1\" style:class=\"index\"><style:paragraph-properties fo:margin-top=\"0cm\" fo:margin-bottom=\"0.4cm\"/><style:text-properties fo:font-size=\"16pt\" fo:font-weight=\"bold\" style:font-weight-asian=\"bold\" style:font-weight-complex=\"bold\"/></style:style>\n<style:style style:name=\"Contents_20_1\" style:display-name=\"Contents 1\" style:family=\"paragraph\" style:parent-style-name=\"Standard\" style:class=\"index\"><style:paragraph-properties fo:margin-top=\"0cm\" fo:margin-bottom=\"0.15cm\"><style:tab-stops><style:tab-stop style:position=\"{TEXT_WIDTH}cm\" style:type=\"right\" style:leader-style=\"dotted\" style:leader-text=\".\"/></style:tab-stops></style:paragraph-properties></style:style>\n<style:style style:name=\"Index_20_Link\" style:display-name=\"Index Link\" style:family=\"text\"/>";

/// The page's size and margins. With page numbers the footer takes the lower half
/// of the bottom margin, so the text has as much room as without.
fn page_layout(pages: Pages) -> String {
    let (width, height) = paper_size(pages.paper);
    let (bottom, footer) = if pages.numbered {
        (
            MARGIN / 2.0,
            "<style:footer-style><style:header-footer-properties fo:min-height=\"0.6cm\" fo:margin-top=\"0.4cm\"/></style:footer-style>",
        )
    } else {
        (MARGIN, "")
    };
    format!(
        "<style:page-layout style:name=\"Page\"><style:page-layout-properties fo:page-width=\"{width}cm\" fo:page-height=\"{height}cm\" style:print-orientation=\"portrait\" fo:margin-top=\"{MARGIN}cm\" fo:margin-bottom=\"{bottom}cm\" fo:margin-left=\"{MARGIN}cm\" fo:margin-right=\"{MARGIN}cm\"/>{footer}</style:page-layout>"
    )
}

fn master_page(numbered: bool) -> String {
    if numbered {
        "<style:master-page style:name=\"Standard\" style:page-layout-name=\"Page\"><style:footer><text:p text:style-name=\"Footer\"><text:page-number text:select-page=\"current\">1</text:page-number></text:p></style:footer></style:master-page>".to_owned()
    } else {
        "<style:master-page style:name=\"Standard\" style:page-layout-name=\"Page\"/>".to_owned()
    }
}

/// The bullets, the numbers, and tasks, whose ☐ or ☑ is in their text.
fn list_styles() -> String {
    let mut out = String::new();
    for (name, level_of) in [
        ("Bullets", bullet as fn(usize) -> String),
        ("Numbering", number),
        ("Tasks", |_| bullet_of("\u{A0}")),
    ] {
        let _ = write!(out, "<text:list-style style:name=\"{name}\">");
        for level in 1..=LIST_LEVELS {
            let _ = write!(
                out,
                "{}",
                level_of(level)
                    .replace("{LEVEL}", &level.to_string())
                    .replace("{PROPERTIES}", &level_properties(level))
            );
        }
        let _ = writeln!(out, "</text:list-style>");
    }
    out.pop();
    out
}

fn bullet(level: usize) -> String {
    bullet_of(BULLETS[(level - 1) % BULLETS.len()])
}

fn bullet_of(character: &str) -> String {
    format!(
        "<text:list-level-style-bullet text:level=\"{{LEVEL}}\" text:bullet-char=\"{character}\">{{PROPERTIES}}</text:list-level-style-bullet>"
    )
}

fn number(_level: usize) -> String {
    "<text:list-level-style-number text:level=\"{LEVEL}\" style:num-format=\"1\" style:num-suffix=\".\">{PROPERTIES}</text:list-level-style-number>".to_owned()
}

fn level_properties(level: usize) -> String {
    let indent = 0.6 * f64::from(u8::try_from(level).unwrap_or(1));
    format!(
        "<style:list-level-properties text:list-level-position-and-space-mode=\"label-alignment\"><style:list-level-label-alignment text:label-followed-by=\"listtab\" text:list-tab-stop-position=\"{indent:.1}cm\" fo:text-indent=\"-0.5cm\" fo:margin-left=\"{indent:.1}cm\"/></style:list-level-properties>"
    )
}

fn meta_xml(title: &str, note: Option<&Source>) -> String {
    let mut dates = String::new();
    for (key, element) in [("created", "meta:creation-date"), ("modified", "dc:date")] {
        let date = note
            .and_then(|note| document::date_of(note, key))
            .and_then(|ms| u64::try_from(ms).ok());
        if let Some(ms) = date {
            let _ = writeln!(dates, "<{element}>{}</{element}>", rfc3339_utc(ms));
        }
    }
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<office:document-meta xmlns:office=\"urn:oasis:names:tc:opendocument:xmlns:office:1.0\" xmlns:meta=\"urn:oasis:names:tc:opendocument:xmlns:meta:1.0\" xmlns:dc=\"http://purl.org/dc/elements/1.1/\" office:version=\"1.2\">\n<office:meta>\n<meta:generator>Schnipsle</meta:generator>\n<dc:title>{}</dc:title>\n{dates}</office:meta>\n</office:document-meta>\n",
        escape(title)
    )
}

fn manifest_xml(pictures: &[Picture]) -> String {
    let mut out = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<manifest:manifest xmlns:manifest=\"urn:oasis:names:tc:opendocument:xmlns:manifest:1.0\" manifest:version=\"1.2\">\n<manifest:file-entry manifest:full-path=\"/\" manifest:version=\"1.2\" manifest:media-type=\"{MEDIA_TYPE}\"/>\n"
    );
    for part in ["content.xml", "styles.xml", "meta.xml"] {
        let _ = writeln!(
            out,
            "<manifest:file-entry manifest:full-path=\"{part}\" manifest:media-type=\"text/xml\"/>"
        );
    }
    for picture in pictures {
        let _ = writeln!(
            out,
            "<manifest:file-entry manifest:full-path=\"{}\" manifest:media-type=\"{}\"/>",
            escape(&picture.path),
            escape(&picture.media_type)
        );
    }
    out.push_str("</manifest:manifest>\n");
    out
}
