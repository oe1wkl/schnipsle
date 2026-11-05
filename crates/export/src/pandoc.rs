// SPDX-License-Identifier: Apache-2.0
//! DOCX and EPUB through Pandoc (ADR-0010, ADR-0025): the tree all formats share
//! is written as Markdown, with its pictures as files in a temporary folder, for
//! a Pandoc the user installed. It is called as a program, never linked or
//! bundled (ADR-0005). [`PandocExporter`] does not know how Pandoc is run: a
//! [`Converter`] does, so the Markdown can be tested without it.

use std::collections::HashMap;
use std::env;
use std::ffi::OsStr;
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
use crate::output::{self, Names};
use crate::{Exporter, Format, Input, Layout, Options, Paper, Report, docx_package};

/// What Pandoc reads: plain `CommonMark` and what the tree needs of the rest, so
/// that no other Markdown of Pandoc's changes what a note says.
const MARKDOWN: &str = "commonmark+pipe_tables+footnotes+strikeout+attributes+raw_attribute";

/// A new page in DOCX, between the notes of one document. In EPUB each note is a
/// chapter anyway.
const DOCX_PAGE_BREAK: &str =
    "```{=openxml}\n<w:p><w:r><w:br w:type=\"page\"/></w:r></w:p>\n```\n\n";

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
            command.arg(format!("--metadata=title:{}", job.title));
            if let Some(day) = job.created.as_deref().and_then(|c| c.get(..10)) {
                command.arg(format!("--metadata=date:{day}"));
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
        let (anchors, rules) = document::rules_of_one_document(vault, notes, options, false, false);
        let mut writer = Markdown::default();
        for (index, (note, anchor)) in notes.iter().zip(&anchors).enumerate() {
            let arena = Arena::new();
            let root = document::prepare(&arena, note, anchor, &rules, collected);
            if self.format == Format::Docx && index > 0 {
                writer.text.push_str(DOCX_PAGE_BREAK);
            }
            writer.note(&arena, root, rules.sections.then_some(anchor.as_str()))?;
        }
        let (title, note) = document::title_of_one_document(notes, input);
        let bytes = self.convert(writer, &title, note, options.paper, collected)?;
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
    fn file_per_note(
        &self,
        vault: &Vault,
        notes: &[Source],
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
        };
        for (note, name) in notes.iter().zip(files) {
            let arena = Arena::new();
            let anchor = format!("note-{}", schnipsle_core::names::slug(&note.title));
            let root = document::prepare(&arena, note, &anchor, &rules, collected);
            let mut writer = Markdown::default();
            writer.note(&arena, root, None)?;
            let bytes = self.convert(writer, &note.title, Some(note), options.paper, collected)?;
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
        paper: Paper,
        collected: &mut Collected,
    ) -> Result<Vec<u8>> {
        let date = |key| {
            note.and_then(|note| document::date_of(note, key))
                .and_then(|ms| u64::try_from(ms).ok())
        };
        let created = date("created").map(rfc3339_utc);
        let job = ConversionJob {
            format: self.format,
            markdown: writer.text,
            title: title.to_owned(),
            created: created.clone(),
            modified_ms: date("modified"),
            pictures: writer.pictures,
        };
        let converted = self.converter.convert(&job).map_err(VaultError::Io)?;
        collected.warnings.extend(
            converted
                .warnings
                .into_iter()
                .map(|warning| format!("{title}: {warning}")),
        );
        if self.format == Format::Docx {
            let bytes = docx_package::finish(&converted.bytes, title, created.as_deref(), paper)
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
                self.file_per_note(vault, &notes, *options, to, &mut report, &mut collected)?;
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
}

impl Markdown {
    /// Adds a note. `anchor` is the identifier of its first heading, for links to it.
    fn note<'a>(
        &mut self,
        arena: &'a Arena<'a>,
        root: &'a AstNode<'a>,
        anchor: Option<&str>,
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
            NodeValue::Image(image) if image.url.starts_with("data:") => self.picture(&image.url),
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
