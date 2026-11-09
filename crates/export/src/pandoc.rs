// SPDX-License-Identifier: Apache-2.0
//! DOCX and EPUB through Pandoc (ADR-0010, ADR-0025): the tree all formats share
//! is written as Markdown, with its pictures as files in a temporary folder, for
//! a Pandoc the user installed. It is called as a program, never linked or
//! bundled (ADR-0005). [`PandocExporter`] does not know how Pandoc is run: a
//! [`Converter`] does, so the Markdown can be tested without it.

use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
use std::fmt::Write as _;
use std::fs::{self, File};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine as _;
use comrak::nodes::{AstNode, NodeValue};
use comrak::{Arena, format_commonmark};
use schnipsle_core::stamp::rfc3339_utc;
use schnipsle_core::tags::parse_options;
use schnipsle_core::vault::{Result, Vault, VaultError};

use crate::document::{self, Collected, Rules, Source};
use crate::music::{Music, Output};
use crate::odt_content::escape;
use crate::output::{self, Names};
use crate::{Exporter, Format, Input, Layout, Options, Paper, Report, docx_package};

/// What Pandoc reads: plain `CommonMark` and what the tree needs of the rest, so
/// that no other Markdown of Pandoc's changes what a note says.
const MARKDOWN: &str = "commonmark+pipe_tables+footnotes+strikeout+attributes+raw_attribute";

/// A new page in DOCX, between the notes of one document. In EPUB each note is a
/// chapter anyway.
const DOCX_PAGE_BREAK: &str =
    "```{=openxml}\n<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\n```\n\n";

/// What an EPUB adds to Pandoc's stylesheet, after it in every chapter, so that
/// a reader such as Books can lay out its pages: a note, a chapter of its own,
/// starts no second page before its title; a picture is never taller than a page;
/// and a long code block goes on over pages instead of being cut off.
const EPUB_STYLE: &str = "<style>
section.level1 > h1 { page-break-before: auto; break-before: auto; margin-top: 0; }
img { width: auto; height: auto; max-width: 100%; max-height: 90vh; object-fit: contain; }
pre { overflow: visible; }
</style>
";

/// How the pages of a document look.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Pages {
    pub paper: Paper,
    /// A page number at the foot of every page of a DOCX.
    pub numbered: bool,
    /// One document of several notes starts with a table of contents.
    pub contents: bool,
}

/// The identifier and name of the bookmark at the start of note `index` of a
/// DOCX with a table of contents. Pandoc's own identifiers are small numbers.
fn docx_bookmark(index: usize) -> (usize, String) {
    (900_000 + index, format!("_Schnipsle{}", index + 1))
}

/// The table of contents of a DOCX of notes with these titles, as raw Word XML:
/// each title links to its note, and a field shows the page it starts on, which
/// Word and `LibreOffice` work out when they open it (ADR-0029).
fn docx_contents_of<'t>(titles: impl Iterator<Item = &'t str>, paper: Paper) -> String {
    let width = docx_package::text_width(paper);
    let mut xml = String::from(
        "<w:p><w:pPr><w:pStyle w:val=\"TOCHeading\"/></w:pPr><w:r><w:t>Contents</w:t></w:r></w:p>\n",
    );
    for (index, title) in titles.enumerate() {
        let (_, name) = docx_bookmark(index);
        let _ = writeln!(
            xml,
            "<w:p><w:pPr><w:tabs><w:tab w:val=\"right\" w:leader=\"dot\" w:pos=\"{width}\"/></w:tabs></w:pPr><w:hyperlink w:anchor=\"{name}\"><w:r><w:t xml:space=\"preserve\">{}</w:t></w:r></w:hyperlink><w:r><w:tab/></w:r><w:r><w:fldChar w:fldCharType=\"begin\" w:dirty=\"true\"/></w:r><w:r><w:instrText xml:space=\"preserve\"> PAGEREF {name} \\h </w:instrText></w:r><w:r><w:fldChar w:fldCharType=\"separate\"/></w:r><w:r><w:t></w:t></w:r><w:r><w:fldChar w:fldCharType=\"end\"/></w:r></w:p>",
            escape(title)
        );
    }
    let fence = "`".repeat(longest_backtick_run(&xml).max(2) + 1);
    format!("{fence}{{=openxml}}\n{xml}{fence}\n\n")
}

/// The longest run of backticks in `text`, so a fence around it can be longer.
fn longest_backtick_run(text: &str) -> usize {
    text.split(|c| c != '`').map(str::len).max().unwrap_or(0)
}

/// How long a conversion may take before the export gives up.
const LIMIT: Duration = Duration::from_secs(300);

/// Where Homebrew puts Pandoc: an app started from Finder has neither on its `PATH`.
const USUAL_FOLDERS: [&str; 3] = [
    "/opt/homebrew/bin",
    "/usr/local/bin",
    "/home/linuxbrew/.linuxbrew/bin",
];

/// A file a Markdown text refers to, in the folder it is converted in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobPicture {
    /// Its file name, as the Markdown refers to it.
    pub name: String,
    /// The content of the file.
    pub bytes: Vec<u8>,
}

/// One document to convert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConversionJob {
    /// What to write.
    pub format: Format,
    /// The document in Markdown.
    pub markdown: String,
    /// The document's title.
    pub title: String,
    /// The note's dates, for a document of one note, as RFC 3339 in UTC.
    pub created: Option<String>,
    /// In milliseconds since the Unix epoch.
    pub modified_ms: Option<u64>,
    /// The files the Markdown refers to.
    pub pictures: Vec<JobPicture>,
    /// One document of several notes starts with a table of contents (ADR-0029),
    /// which Pandoc makes for an e-book; a DOCX has it in its Markdown.
    pub contents: bool,
}

/// A finished document, and what the converter said about it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Converted {
    /// The file.
    pub bytes: Vec<u8>,
    /// What the converter said, one warning each.
    pub warnings: Vec<String>,
}

/// Turns Markdown into a document.
pub trait Converter {
    /// # Errors
    /// The conversion failed; the error says why.
    fn convert(&self, job: &ConversionJob) -> io::Result<Converted>;
}

/// A Pandoc on this computer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pandoc {
    path: PathBuf,
    version: String,
}

impl Pandoc {
    /// The Pandoc on `PATH` or in Homebrew's folders, if any that runs.
    #[must_use]
    pub fn find() -> Option<Self> {
        let usual = USUAL_FOLDERS.map(PathBuf::from);
        Self::find_in(env::var_os("PATH").as_deref(), &usual)
    }

    fn find_in(path: Option<&OsStr>, usual: &[PathBuf]) -> Option<Self> {
        let name = format!("pandoc{}", env::consts::EXE_SUFFIX);
        path.into_iter()
            .flat_map(env::split_paths)
            .chain(usual.iter().cloned())
            .map(|folder| folder.join(&name))
            .filter(|file| file.is_file())
            .find_map(|file| Self::at(file).ok())
    }

    /// The Pandoc in the file `path`.
    ///
    /// # Errors
    /// It cannot be run, or does not say what version it is.
    pub fn at(path: PathBuf) -> io::Result<Self> {
        let output = Command::new(&path).arg("--version").output()?;
        let text = String::from_utf8_lossy(&output.stdout);
        let version = version_in(&text)
            .filter(|_| output.status.success())
            .ok_or_else(|| io::Error::other("not a Pandoc that says its version"))?;
        Ok(Self { path, version })
    }

    /// Its version, such as `3.1.3`.
    #[must_use]
    pub fn version(&self) -> &str {
        &self.version
    }
}

impl Converter for Pandoc {
    fn convert(&self, job: &ConversionJob) -> io::Result<Converted> {
        let dir = tempfile::tempdir()?;
        fs::write(dir.path().join("input.md"), &job.markdown)?;
        for picture in &job.pictures {
            fs::write(dir.path().join(&picture.name), &picture.bytes)?;
        }
        let output = format!("output.{}", job.format.extension());
        let errors = dir.path().join("errors.txt");

        let mut command = Command::new(&self.path);
        command
            .current_dir(dir.path())
            .arg(format!("--from={MARKDOWN}"))
            .arg(format!("--to={}", job.format.extension()))
            .arg(format!("--output={output}"))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(File::create(&errors)?);
        // DOCX shows a title given here on its first page; docx_package sets it instead.
        if job.format == Format::Epub {
            let style = format!("{EPUB_STYLE}<style>\n{}</style>\n", crate::songs::CSS);
            fs::write(dir.path().join("style.html"), style)?;
            command
                .arg(format!("--metadata=title:{}", job.title))
                .arg("--include-in-header=style.html");
            if let Some(day) = job.created.as_deref().and_then(|c| c.get(..10)) {
                command.arg(format!("--metadata=date:{day}"));
            }
            if job.contents {
                command
                    .arg("--toc")
                    .arg("--toc-depth=1")
                    .arg("--metadata=toc-title:Contents");
            }
        }
        command.arg("input.md");
        match job.modified_ms {
            Some(ms) => command.env("SOURCE_DATE_EPOCH", (ms / 1000).to_string()),
            None => command.env_remove("SOURCE_DATE_EPOCH"),
        };
        // It finds what it calls itself, such as rsvg-convert, next to it.
        if let Some(folder) = self.path.parent() {
            let path = env::var_os("PATH").unwrap_or_default();
            let folders = std::iter::once(folder.to_owned()).chain(env::split_paths(&path));
            command.env("PATH", env::join_paths(folders).map_err(io::Error::other)?);
        }

        let status = wait(&mut command)?;
        let said = fs::read_to_string(&errors).unwrap_or_default();
        if !status {
            let why = said.trim();
            return Err(io::Error::other(format!("Pandoc stopped: {why}")));
        }
        Ok(Converted {
            bytes: fs::read(dir.path().join(output))?,
            warnings: warnings_in(&said),
        })
    }
}

/// Runs `command` up to [`LIMIT`]; whether it succeeded.
fn wait(command: &mut Command) -> io::Result<bool> {
    let mut child = command.spawn()?;
    let start = Instant::now();
    loop {
        if let Some(status) = child.try_wait()? {
            return Ok(status.success());
        }
        if start.elapsed() > LIMIT {
            let _ = child.kill();
            let _ = child.wait();
            return Err(io::Error::other("Pandoc did not finish"));
        }
        thread::sleep(Duration::from_millis(20));
    }
}

/// The version in the output of `pandoc --version`.
fn version_in(text: &str) -> Option<String> {
    let mut words = text.lines().next()?.split_whitespace();
    let name = words.next()?;
    let version = words.next()?;
    (name.starts_with("pandoc") && version.starts_with(|c: char| c.is_ascii_digit()))
        .then(|| version.to_owned())
}

/// Pandoc's warnings, which may run over several lines, each as one line.
fn warnings_in(text: &str) -> Vec<String> {
    text.split("[WARNING]")
        .skip(1)
        .map(|warning| warning.split_whitespace().collect::<Vec<_>>().join(" "))
        .filter(|warning| !warning.is_empty())
        .collect()
}

/// A warning of Pandoc in words the user can act on, for the one that needs it:
/// Pandoc names a picture by its file in the temporary folder and asks for
/// `rsvg-convert`, without saying that the SVG is in the document all the same.
fn explained(warning: &str, descriptions: &HashMap<String, String>) -> Option<String> {
    let (name, why) = warning
        .strip_prefix("Could not convert image ")?
        .split_once(": ")?;
    // Named by `Markdown::picture`, in lower case.
    let svg = Path::new(name).extension().is_some_and(|e| e == "svg");
    if !(svg && why.contains("rsvg-convert")) {
        return None;
    }
    let picture = match descriptions.get(name).map(|d| d.trim()) {
        Some("") | None => "An SVG drawing".to_owned(),
        Some(description) => format!("The SVG drawing \u{201c}{description}\u{201d}"),
    };
    Some(format!(
        "{picture} is in the document as SVG only, which Word before 2016 and some other \
         programs do not show. With rsvg-convert installed (on a Mac: brew install \
         librsvg), a PNG copy is added for them."
    ))
}

/// Writes DOCX or EPUB through a [`Converter`].
pub struct PandocExporter<C> {
    format: Format,
    converter: C,
}

impl<C: Converter> PandocExporter<C> {
    /// An exporter of `format`, which Pandoc must be able to write.
    pub const fn new(format: Format, converter: C) -> Self {
        Self { format, converter }
    }

    /// What music is written as for this format.
    fn output(&self) -> Output {
        if self.format == Format::Epub {
            Output::Epub
        } else {
            Output::Docx
        }
    }

    /// One document of all `notes`, and its file name.
    fn one_document(
        &self,
        vault: &Vault,
        notes: &[Source],
        input: &Input,
        options: Options,
        to: &Path,
        collected: &mut Collected,
    ) -> Result<Option<String>> {
        let (anchors, mut rules) = document::rules_of_one_document(
            vault,
            notes,
            options,
            false,
            false,
            Music::of(&input.scores, self.output(), options.abc_source),
        );
        rules.upright_pictures = self.format == Format::Docx;
        let pages = Pages {
            paper: options.paper,
            numbered: document::numbers_pages(options, notes),
            contents: document::has_contents(options, notes),
        };
        let docx_contents = pages.contents && self.format == Format::Docx;
        let mut writer = Markdown::default();
        if docx_contents {
            let titles = notes.iter().map(|note| note.title.as_str());
            writer
                .text
                .push_str(&docx_contents_of(titles, options.paper));
        }
        for (index, (note, anchor)) in notes.iter().zip(&anchors).enumerate() {
            let arena = Arena::new();
            let root = document::prepare(&arena, note, anchor, &rules, collected);
            if self.format == Format::Docx && (index > 0 || docx_contents) {
                writer.text.push_str(DOCX_PAGE_BREAK);
            }
            let anchor = rules.sections.then_some(anchor.as_str());
            writer.note(&arena, root, anchor, docx_contents.then_some(index))?;
        }
        let (title, note) = document::title_of_one_document(notes, input);
        let bytes = self.convert(writer, &title, note, pages, collected)?;
        if options.layout == Layout::SingleFile {
            output::replace_file(to, &bytes)?;
            return Ok(to.file_name().map(|n| n.to_string_lossy().into_owned()));
        }
        fs::create_dir_all(to)?;
        let mut names = Names::new(to);
        let name = names.free(&title, self.format.extension());
        collected.numbered.append(&mut names.numbered);
        output::write_new(&to.join(&name), &bytes)?;
        Ok(Some(name))
    }

    /// A file per note. A DOCX links to the others next to it; an e-book reader
    /// follows no link out of its book, so in EPUB those links become text.
    #[allow(clippy::too_many_arguments)] // the parts of an export, as `export` has them
    fn file_per_note(
        &self,
        vault: &Vault,
        notes: &[Source],
        input: &Input,
        options: Options,
        to: &Path,
        report: &mut Report,
        collected: &mut Collected,
    ) -> Result<()> {
        fs::create_dir_all(to)?;
        let mut names = Names::new(to);
        let extension = self.format.extension();
        let files: Vec<String> = notes
            .iter()
            .map(|note| names.free(&note.title, extension))
            .collect();
        collected.numbered.append(&mut names.numbered);
        let links = self.format == Format::Docx;
        let rules = Rules {
            vault,
            hrefs: if links {
                notes
                    .iter()
                    .zip(&files)
                    .map(|(note, name)| (note.path.clone(), document::href_of(name)))
                    .collect()
            } else {
                HashMap::new()
            },
            titles: document::titles_of(notes),
            out_of_reach: if links {
                std::collections::HashSet::new()
            } else {
                notes.iter().map(|note| note.path.clone()).collect()
            },
            copy_attachments: false,
            web_pictures: false,
            keep_tag_line: options.keep_tag_line,
            sections: false,
            upright_pictures: self.format == Format::Docx,
            music: Music::of(&input.scores, self.output(), options.abc_source),
        };
        for (note, name) in notes.iter().zip(files) {
            let arena = Arena::new();
            let anchor = format!("note-{}", schnipsle_core::names::slug(&note.title));
            let root = document::prepare(&arena, note, &anchor, &rules, collected);
            let mut writer = Markdown::default();
            writer.note(&arena, root, None, None)?;
            let pages = Pages {
                paper: options.paper,
                numbered: options.page_numbers,
                contents: false,
            };
            let bytes = self.convert(writer, &note.title, Some(note), pages, collected)?;
            output::write_new(&to.join(&name), &bytes)?;
            report.files.push(name);
        }
        Ok(())
    }

    /// The finished document of `writer`.
    fn convert(
        &self,
        writer: Markdown,
        title: &str,
        note: Option<&Source>,
        pages: Pages,
        collected: &mut Collected,
    ) -> Result<Vec<u8>> {
        let date = |key| {
            note.and_then(|note| document::date_of(note, key))
                .and_then(|ms| u64::try_from(ms).ok())
        };
        let created = date("created").map(rfc3339_utc);
        let descriptions = writer.descriptions;
        let job = ConversionJob {
            format: self.format,
            markdown: writer.text,
            title: title.to_owned(),
            created: created.clone(),
            modified_ms: date("modified"),
            pictures: writer.pictures,
            contents: pages.contents,
        };
        let converted = self.converter.convert(&job).map_err(VaultError::Io)?;
        collected
            .warnings
            .extend(converted.warnings.into_iter().map(|warning| {
                let warning = explained(&warning, &descriptions).unwrap_or(warning);
                format!("{title}: {warning}")
            }));
        if self.format == Format::Docx {
            let bytes = docx_package::finish(&converted.bytes, title, created.as_deref(), pages)
                .map_err(VaultError::Io)?;
            return Ok(bytes);
        }
        Ok(converted.bytes)
    }
}

impl<C: Converter> Exporter for PandocExporter<C> {
    fn format(&self) -> Format {
        self.format
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
                self.file_per_note(
                    vault,
                    &notes,
                    input,
                    *options,
                    to,
                    &mut report,
                    &mut collected,
                )?;
            }
            Layout::SingleFile | Layout::Folder => {
                let name = self.one_document(vault, &notes, input, *options, to, &mut collected)?;
                report.files.extend(name);
            }
        }
        report.problems.append(&mut collected.problems);
        report.numbered.append(&mut collected.numbered);
        report.warnings.append(&mut collected.warnings);
        Ok(report)
    }
}

/// The Markdown of a document, and the pictures it refers to.
#[derive(Default)]
struct Markdown {
    text: String,
    pictures: Vec<JobPicture>,
    /// The name of a picture by its `data:` URL.
    known: HashMap<String, String>,
    /// The description of a picture by its name, for what Pandoc says about it.
    descriptions: HashMap<String, String>,
}

impl Markdown {
    /// Adds a note. `anchor` is the identifier of its first heading, for links to it.
    fn note<'a>(
        &mut self,
        arena: &'a Arena<'a>,
        root: &'a AstNode<'a>,
        anchor: Option<&str>,
        bookmark: Option<usize>,
    ) -> Result<()> {
        for node in root.descendants().collect::<Vec<_>>() {
            self.adapt(arena, node);
        }
        let mut options = parse_options();
        options.render.r#unsafe = true;
        let mut text = String::new();
        format_commonmark(root, &options, &mut text)
            .map_err(|e| VaultError::Io(io::Error::other(e)))?;
        if let Some(anchor) = anchor {
            let end = text.find('\n').unwrap_or(text.len());
            text.insert_str(end, &format!(" {{#{anchor}}}"));
        }
        // The page reference of a table of contents needs a bookmark of a name it
        // knows, which Pandoc's own do not promise.
        if let Some(index) = bookmark
            && text.starts_with("# ")
        {
            let (id, name) = docx_bookmark(index);
            let mark = format!(
                "`<w:bookmarkStart w:id=\"{id}\" w:name=\"{name}\"/><w:bookmarkEnd w:id=\"{id}\"/>`{{=openxml}}"
            );
            text.insert_str(2, &mark);
        }
        self.text.push_str(&text);
        if !self.text.ends_with("\n\n") {
            self.text.push('\n');
        }
        Ok(())
    }

    /// Makes a node say to Pandoc what it says in the note.
    fn adapt<'a>(&mut self, arena: &'a Arena<'a>, node: &'a AstNode<'a>) {
        let url = match &node.data().value {
            NodeValue::Link(link) if link.url.starts_with('#') => Some(decoded(&link.url)),
            NodeValue::Image(image) if image.url.starts_with("data:") => {
                let name = self.picture(&image.url);
                if let Some(name) = &name {
                    let description = document::plain_text(node);
                    self.descriptions.entry(name.clone()).or_insert(description);
                }
                name
            }
            _ => None,
        };
        if let Some(url) = url {
            document::set_url(node, &url);
        }
        // Its ☐ or ☑ is in its text by now; written as a task it would show `[x]` too.
        if matches!(node.data().value, NodeValue::TaskItem(_))
            && let Some(parent) = node.parent()
            && let NodeValue::List(list) = &mut parent.data_mut().value
        {
            list.is_task_list = false;
            let list = *list;
            node.data_mut().value = NodeValue::Item(list);
        }
        let text = match &mut node.data_mut().value {
            // Would be read as raw output for one format, and left out of the others.
            NodeValue::CodeBlock(block) if block.info.starts_with("{=") => {
                block.info.clear();
                None
            }
            NodeValue::Text(text) if text.contains('{') => Some(text.to_string()),
            _ => None,
        };
        if let Some(text) = text {
            split_braces(arena, node, &text);
        }
    }

    /// The file for the picture in a `data:` URL.
    fn picture(&mut self, url: &str) -> Option<String> {
        if let Some(name) = self.known.get(url) {
            return Some(name.clone());
        }
        let (media_type, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?;
        let extension = match media_type {
            "image/jpeg" => "jpg",
            "image/svg+xml" => "svg",
            "image/apng" => "png",
            other => other.strip_prefix("image/").unwrap_or("bin"),
        };
        let name = format!("picture-{}.{extension}", self.pictures.len() + 1);
        self.pictures.push(JobPicture {
            name: name.clone(),
            bytes,
        });
        self.known.insert(url.to_owned(), name.clone());
        Some(name)
    }
}

/// The name in a link inside the document, as Pandoc compares names undecoded.
fn decoded(url: &str) -> String {
    percent_encoding::percent_decode_str(url)
        .decode_utf8_lossy()
        .into_owned()
}

/// Replaces a text node by the same text with `{` as an entity: after code, a link
/// or at the end of a heading Pandoc would take `{…}` for attributes and drop it.
fn split_braces<'a>(arena: &'a Arena<'a>, node: &'a AstNode<'a>, text: &str) {
    for (index, piece) in text.split('{').enumerate() {
        if index > 0 {
            let brace = NodeValue::HtmlInline("&#123;".to_owned());
            node.insert_before(arena.alloc(brace.into()));
        }
        if !piece.is_empty() {
            let piece = NodeValue::Text(piece.to_owned().into());
            node.insert_before(arena.alloc(piece.into()));
        }
    }
    node.detach();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_version_comes_from_the_first_line() {
        assert_eq!(
            version_in("pandoc 3.12.1\nFeatures: +server\n").as_deref(),
            Some("3.12.1")
        );
        assert_eq!(version_in("pandoc.exe 2.9").as_deref(), Some("2.9"));
        assert_eq!(version_in("pandoc: command failed"), None);
        assert_eq!(version_in("something 1.0"), None);
        assert_eq!(version_in(""), None);
    }

    #[test]
    fn warnings_are_one_line_each() {
        let said = "[WARNING] Could not convert image a.svg: check that\n  rsvg-convert is in path\n[WARNING] Another\nthing\n";
        assert_eq!(
            warnings_in(said),
            [
                "Could not convert image a.svg: check that rsvg-convert is in path",
                "Another thing"
            ]
        );
        assert!(warnings_in("").is_empty());
    }

    #[test]
    fn a_missing_svg_converter_is_explained() {
        let descriptions = HashMap::from([
            ("picture-2.svg".to_owned(), "A plan".to_owned()),
            ("picture-3.svg".to_owned(), String::new()),
        ]);
        let said = |name: &str| {
            format!(
                "Could not convert image {name}: \"check that rsvg-convert is in path.\\n\
                 rsvg-convert: createProcess: posix_spawnp: does not exist\""
            )
        };
        let named = explained(&said("picture-2.svg"), &descriptions).unwrap();
        assert!(
            named.starts_with(
                "The SVG drawing \u{201c}A plan\u{201d} is in the document as SVG only"
            )
        );
        assert!(named.contains("brew install librsvg"));
        let unnamed = explained(&said("picture-3.svg"), &descriptions).unwrap();
        assert!(unnamed.starts_with("An SVG drawing is in"));
        assert!(explained(&said("picture-9.svg"), &descriptions).is_some());
        let other = "Could not convert image picture-1.png: something else";
        assert_eq!(explained(other, &descriptions), None);
        assert_eq!(explained("Something else", &descriptions), None);
    }

    #[cfg(unix)]
    #[test]
    fn found_on_the_path_before_the_usual_folders() {
        use std::os::unix::fs::PermissionsExt as _;

        let dir = tempfile::tempdir().unwrap();
        let script = |folder: &str, version: &str| {
            let folder = dir.path().join(folder);
            fs::create_dir(&folder).unwrap();
            let file = folder.join("pandoc");
            fs::write(&file, format!("#!/bin/sh\necho pandoc {version}\n")).unwrap();
            fs::set_permissions(&file, fs::Permissions::from_mode(0o755)).unwrap();
            folder
        };
        let path = script("path", "3.0");
        let usual = script("usual", "2.0");
        let empty = dir.path().join("empty");
        fs::create_dir(&empty).unwrap();
        let broken = dir.path().join("broken");
        fs::create_dir(&broken).unwrap();
        fs::write(broken.join("pandoc"), "not a program").unwrap();

        let on = |folders: &[&Path]| env::join_paths(folders).unwrap();
        let found =
            Pandoc::find_in(Some(&on(&[&empty, &path])), std::slice::from_ref(&usual)).unwrap();
        assert_eq!(found.version(), "3.0");
        let found = Pandoc::find_in(Some(&on(&[&empty])), std::slice::from_ref(&usual)).unwrap();
        assert_eq!(found.version(), "2.0");
        let found = Pandoc::find_in(None, std::slice::from_ref(&usual)).unwrap();
        assert_eq!(found.version(), "2.0");
        // One that does not run is passed over.
        let found = Pandoc::find_in(Some(&on(&[&broken])), &[usual]).unwrap();
        assert_eq!(found.version(), "2.0");
        assert_eq!(Pandoc::find_in(Some(&on(&[&empty])), &[]), None);
    }
}
