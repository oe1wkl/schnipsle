// SPDX-License-Identifier: Apache-2.0
//! From a note to the syntax tree of what an export shows (ADR-0021).
//!
//! All formats write the tree this module makes, so a note looks alike in all
//! of them. Front matter and the final tag line are gone; links and pictures
//! point where they should in this export, or are text; raw HTML is clean
//! ([`crate::raw_html`]); math is code; a task item starts with ☐ or ☑.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt::Write as _;

use base64::Engine as _;
use comrak::nodes::{
    AstNode, NodeCode, NodeCodeBlock, NodeFootnoteDefinition, NodeHeading, NodeHtmlBlock, NodeLink,
    NodeValue,
};
use comrak::{Arena, parse_document};
use schnipsle_core::attachments::LinkTarget;
use schnipsle_core::dates::parse_rfc3339;
use schnipsle_core::front_matter;
use schnipsle_core::names::slug;
use schnipsle_core::tags::{extract_tags, normalize, parse_options};
use schnipsle_core::vault::{Vault, VaultError};
use unicode_normalization::UnicodeNormalization;

use crate::music::{self, Music, Output};
use crate::output;
use crate::raw_html;
use crate::songs;
use crate::{Input, Options, Problem, ProblemKind, Report};

/// A note to export.
pub(crate) struct Source {
    /// Relative to `notes/`.
    pub path: String,
    pub title: String,
    pub text: String,
}

/// Reads the notes; one that cannot be read goes into the report.
pub(crate) fn read_notes(vault: &Vault, paths: &[String], report: &mut Report) -> Vec<Source> {
    let mut notes = Vec::with_capacity(paths.len());
    for path in paths {
        match vault.read_note_text(path) {
            Ok(note) => notes.push(Source {
                path: note.path,
                title: note.title,
                text: note.text,
            }),
            Err(_) => report.problems.push(Problem {
                path: path.clone(),
                title: path.clone(),
                kind: ProblemKind::NoteNotRead,
                target: String::new(),
            }),
        }
    }
    notes
}

/// What goes into the export of notes.
#[allow(clippy::struct_excessive_bools)] // independent choices
pub(crate) struct Rules<'r> {
    pub vault: &'r Vault,
    /// Where a link to an exported note, by its path, goes.
    pub hrefs: HashMap<String, String>,
    /// Exported notes by their normalized title; the first of several wins.
    pub titles: HashMap<String, String>,
    /// Notes in other files of this export that a link cannot reach, by path.
    pub out_of_reach: HashSet<String>,
    /// Attachments are copied next to the document, else pictures are embedded.
    pub copy_attachments: bool,
    /// Pictures on the web stay pictures, else they become links.
    pub web_pictures: bool,
    pub keep_tag_line: bool,
    /// Each note is a section of one document, which starts with its title.
    pub sections: bool,
    /// Pictures that only their orientation turns upright are turned in the
    /// file, for formats whose programs ignore it (ADR-0028).
    pub upright_pictures: bool,
    /// The engraved `abc` blocks (ADR-0033).
    pub music: Music<'r>,
}

/// Whether one document of `notes` starts with a table of contents: asked for,
/// and more than one note (ADR-0029).
pub(crate) fn has_contents(options: Options, notes: &[Source]) -> bool {
    options.contents && notes.len() > 1 && options.layout != crate::Layout::FilePerNote
}

/// Whether the pages of a format with pages are numbered: asked for, and always
/// with a table of contents, whose page numbers need them (ADR-0029).
pub(crate) fn numbers_pages(options: Options, notes: &[Source]) -> bool {
    options.page_numbers || has_contents(options, notes)
}

/// The title of one document of several notes without a name of its own.
pub(crate) const DEFAULT_TITLE: &str = "Notes";

/// The anchors of `notes` in one document, and the rules that link to them.
pub(crate) fn rules_of_one_document<'r>(
    vault: &'r Vault,
    notes: &[Source],
    options: Options,
    copy_attachments: bool,
    web_pictures: bool,
    music: Music<'r>,
) -> (Vec<String>, Rules<'r>) {
    let mut used = HashSet::new();
    let anchors: Vec<String> = notes
        .iter()
        .map(|note| output::free_anchor(&mut used, &slug(&note.title)))
        .collect();
    let rules = Rules {
        vault,
        hrefs: notes
            .iter()
            .zip(&anchors)
            .map(|(note, anchor)| (note.path.clone(), format!("#{}", href_of(anchor))))
            .collect(),
        titles: titles_of(notes),
        out_of_reach: HashSet::new(),
        copy_attachments,
        web_pictures,
        keep_tag_line: options.keep_tag_line,
        sections: notes.len() > 1,
        upright_pictures: false,
        music,
    };
    (anchors, rules)
}

/// The title of one document of `notes`, and the note if it is the only one: a
/// document of several has no dates of its own.
pub(crate) fn title_of_one_document<'n>(
    notes: &'n [Source],
    input: &Input,
) -> (String, Option<&'n Source>) {
    match notes {
        [note] => (note.title.clone(), Some(note)),
        _ => (
            input
                .name
                .clone()
                .unwrap_or_else(|| DEFAULT_TITLE.to_owned()),
            None,
        ),
    }
}

/// A date of the note's front matter, in milliseconds since the Unix epoch.
pub(crate) fn date_of(note: &Source, key: &str) -> Option<i64> {
    front_matter::get(&note.text, key).and_then(|value| parse_rfc3339(&value))
}

/// The notes by their normalized title, for [`Rules::titles`].
pub(crate) fn titles_of(notes: &[Source]) -> HashMap<String, String> {
    let mut titles = HashMap::new();
    for note in notes {
        titles
            .entry(normalize(note.title.trim()))
            .or_insert_with(|| note.path.clone());
    }
    titles
}

/// A file to copy, reached by `link` in the note at `path`.
pub(crate) struct Copy {
    pub path: String,
    pub title: String,
    pub link: String,
    /// What to write instead of the file: the picture converted (ADR-0027).
    pub converted: Option<Vec<u8>>,
}

/// What preparing notes collected.
#[derive(Default)]
pub(crate) struct Collected {
    pub problems: Vec<Problem>,
    /// Files that got a number, as [`crate::Report::numbered`] says.
    pub numbered: Vec<String>,
    /// What a converter said, as [`crate::Report::warnings`] says.
    pub warnings: Vec<String>,
    /// Files to copy, by where they go: relative to the document's folder, the
    /// same as relative to the vault.
    pub copies: BTreeMap<String, Copy>,
}

/// The syntax tree of `note` for an export. `anchor` makes the names of its
/// footnotes unique in a document.
pub(crate) fn prepare<'a>(
    arena: &'a Arena<'a>,
    note: &Source,
    anchor: &str,
    rules: &Rules<'_>,
    collected: &mut Collected,
) -> &'a AstNode<'a> {
    let root = parse_document(arena, &note.text, &parse_options());
    for child in root.children().collect::<Vec<_>>() {
        if matches!(child.data().value, NodeValue::FrontMatter(_)) {
            child.detach();
        }
    }
    raw_html::clean(arena, root);
    if !rules.keep_tag_line
        && let Some(line) = final_tag_line(root)
    {
        line.detach();
    }
    let mut preparer = Preparer {
        arena,
        note,
        rules,
        collected,
    };
    preparer.walk(root);
    name_footnotes(root, anchor);
    if rules.sections {
        start_with_title(arena, root, &note.title);
    }
    root
}

/// The last block of the note if it is a paragraph of nothing but tags and
/// spaces (format.md §5.3). Footnotes are at the end of the tree, wherever
/// they are written.
fn final_tag_line<'a>(root: &'a AstNode<'a>) -> Option<&'a AstNode<'a>> {
    let last = root
        .reverse_children()
        .find(|c| !matches!(c.data().value, NodeValue::FootnoteDefinition(_)))?;
    if !matches!(last.data().value, NodeValue::Paragraph) {
        return None;
    }
    let mut text = String::new();
    for child in last.children() {
        match &child.data().value {
            NodeValue::Text(t) => text.push_str(t),
            NodeValue::SoftBreak | NodeValue::LineBreak => text.push(' '),
            _ => return None,
        }
    }
    let all_tags = text.split_whitespace().all(|word| {
        let word: String = word.nfc().collect();
        matches!(extract_tags(&word).as_slice(), [tag] if word.strip_prefix('#') == Some(tag.name.as_str()))
    });
    (all_tags && !text.trim().is_empty()).then_some(last)
}

/// Footnotes are written per note, so their names must differ between notes.
fn name_footnotes<'a>(root: &'a AstNode<'a>, anchor: &str) {
    for node in root.descendants() {
        match &mut node.data_mut().value {
            NodeValue::FootnoteDefinition(NodeFootnoteDefinition { name, .. }) => {
                *name = format!("{anchor}-{name}");
            }
            NodeValue::FootnoteReference(reference) => {
                reference.name = format!("{anchor}-{}", reference.name);
            }
            _ => {}
        }
    }
}

/// Makes the note start with a level-1 heading with its title (ADR-0021 rule
/// 3). A first heading of any level or a first paragraph of one line is the
/// title (format.md §5.2), so it becomes that heading rather than repeating it.
fn start_with_title<'a>(arena: &'a Arena<'a>, root: &'a AstNode<'a>, title: &str) {
    let heading = NodeValue::Heading(NodeHeading {
        level: 1,
        setext: false,
        closed: false,
    });
    if let Some(first) = root.first_child()
        && let NodeValue::Heading(existing) = &mut first.data_mut().value
    {
        existing.level = 1;
        return;
    }
    if let Some(first) = root.first_child()
        && is_one_line_title(first)
    {
        first.data_mut().value = heading;
        return;
    }
    let new: &'a AstNode<'a> = arena.alloc(heading.into());
    new.append(arena.alloc(NodeValue::Text(title.to_owned().into()).into()));
    root.prepend(new);
}

/// A paragraph that can stand as the title: one line of at most 120 characters
/// (format.md §5.2), without pictures.
fn is_one_line_title<'a>(node: &'a AstNode<'a>) -> bool {
    matches!(node.data().value, NodeValue::Paragraph)
        && !node.descendants().any(|d| {
            matches!(
                d.data().value,
                NodeValue::SoftBreak | NodeValue::LineBreak | NodeValue::Image(_)
            )
        })
        && plain_text(node).chars().count() <= 120
}

/// The text of `node` and what is below it.
pub(crate) fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    for d in node.descendants() {
        match &d.data().value {
            NodeValue::Text(t) => out.push_str(t),
            NodeValue::Code(c) => out.push_str(&c.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
            _ => {}
        }
    }
    out
}

/// Replaces `node` by its children.
fn unwrap<'a>(node: &'a AstNode<'a>) {
    for child in node.children().collect::<Vec<_>>() {
        node.insert_before(child);
    }
    node.detach();
}

/// What a node needs done, taken out so the node can be changed.
enum Work {
    Picture(String),
    Link(String),
    WikiLink(String),
    Math { display: bool, literal: String },
    Task { done: bool },
    Score { source: String, transpose: i32 },
    Song { source: String, info: String },
    Nothing,
}

/// What a link points to, for this module's purposes.
enum Target {
    Web,
    Note(String),
    File(String),
    /// Nothing there, or not downloaded.
    Missing,
    /// A link that is no place in the vault or on the web: another scheme, an
    /// absolute path, a fragment of the note.
    Other,
}

struct Preparer<'a, 'r> {
    arena: &'a Arena<'a>,
    note: &'r Source,
    rules: &'r Rules<'r>,
    collected: &'r mut Collected,
}

impl<'a> Preparer<'a, '_> {
    fn walk(&mut self, parent: &'a AstNode<'a>) {
        for node in parent.children().collect::<Vec<_>>() {
            let work = match &node.data().value {
                NodeValue::Image(link) => Work::Picture(link.url.clone()),
                NodeValue::Link(link) => Work::Link(link.url.clone()),
                NodeValue::WikiLink(link) => Work::WikiLink(link.url.trim().to_owned()),
                NodeValue::Math(math) => Work::Math {
                    display: math.display_math,
                    literal: math.literal.clone(),
                },
                NodeValue::TaskItem(item) => Work::Task {
                    done: item.symbol.is_some(),
                },
                NodeValue::CodeBlock(block) if block.fenced && songs::is_song(&block.info) => {
                    Work::Song {
                        source: block.literal.clone(),
                        info: block.info.clone(),
                    }
                }
                NodeValue::CodeBlock(block) if block.fenced => music::abc_transpose(&block.info)
                    .map_or(Work::Nothing, |transpose| Work::Score {
                        source: block.literal.clone(),
                        transpose,
                    }),
                _ => Work::Nothing,
            };
            // The description of a picture is text only. Links are changed
            // last: they may give their children to `parent`.
            if !matches!(work, Work::Picture(_)) {
                self.walk(node);
            }
            match work {
                Work::Picture(url) => self.picture(node, &url),
                Work::Link(url) => self.link(node, &url),
                Work::WikiLink(title) => self.wiki_link(node, &title),
                Work::Math { display, literal } => math(node, display, &literal),
                Work::Task { done } => self.task(node, done),
                Work::Score { source, transpose } => self.score(node, &source, transpose),
                Work::Song { source, info } => self.song(node, &source, &info),
                Work::Nothing => {}
            }
        }
    }

    fn problem(&mut self, kind: ProblemKind, target: &str) {
        self.collected.problems.push(Problem {
            path: self.note.path.clone(),
            title: self.note.title.clone(),
            kind,
            target: target.to_owned(),
        });
    }

    fn target(&self, url: &str) -> Target {
        match self.rules.vault.link_target(&self.note.path, url) {
            Ok(LinkTarget::Web(_)) => Target::Web,
            Ok(LinkTarget::Note(path)) => Target::Note(path),
            Ok(LinkTarget::File(path)) => {
                if self.rules.vault.full_path(&path).is_file() {
                    Target::File(path)
                } else {
                    Target::Missing
                }
            }
            Err(VaultError::InvalidPath(_)) => Target::Other,
            Err(_) => Target::Missing,
        }
    }

    fn link(&mut self, node: &'a AstNode<'a>, url: &str) {
        match self.target(url) {
            Target::Web => {}
            Target::Note(path) => self.link_to_note(node, &path, url),
            Target::File(path) => self.link_to_file(node, &path, url),
            Target::Missing => {
                let kind = if is_note_link(url) {
                    ProblemKind::NoteNotExported
                } else {
                    ProblemKind::AttachmentNotIncluded
                };
                self.problem(kind, url);
                unwrap(node);
            }
            Target::Other => unwrap(node),
        }
    }

    fn link_to_note(&mut self, node: &'a AstNode<'a>, path: &str, url: &str) {
        if let Some(href) = self.rules.hrefs.get(path) {
            set_url(node, href);
        } else {
            self.problem(self.no_link_to(Some(path)), url);
            unwrap(node);
        }
    }

    /// Why a link to the note at `path` has no target.
    fn no_link_to(&self, path: Option<&str>) -> ProblemKind {
        if path.is_some_and(|path| self.rules.out_of_reach.contains(path)) {
            ProblemKind::NoteInOtherFile
        } else {
            ProblemKind::NoteNotExported
        }
    }

    fn link_to_file(&mut self, node: &'a AstNode<'a>, path: &str, url: &str) {
        if self.rules.copy_attachments {
            self.copy(path, url);
            set_url(node, &href_of(path));
        } else {
            self.problem(ProblemKind::AttachmentNotIncluded, url);
            unwrap(node);
        }
    }

    fn copy(&mut self, path: &str, url: &str) {
        self.collected
            .copies
            .entry(path.to_owned())
            .or_insert_with(|| Copy {
                path: self.note.path.clone(),
                title: self.note.title.clone(),
                link: url.to_owned(),
                converted: None,
            });
    }

    fn wiki_link(&mut self, node: &'a AstNode<'a>, title: &str) {
        let path = self.rules.titles.get(&normalize(title));
        let href = path.and_then(|path| self.rules.hrefs.get(path)).cloned();
        if let Some(url) = href {
            let link = NodeLink {
                url,
                title: String::new(),
            };
            node.data_mut().value = NodeValue::Link(Box::new(link));
        } else {
            let kind = self.no_link_to(path.map(String::as_str));
            self.problem(kind, title);
            unwrap(node);
        }
    }

    fn picture(&mut self, node: &'a AstNode<'a>, url: &str) {
        let description = plain_text(node);
        match self.target(url) {
            Target::Web if self.rules.web_pictures => {}
            Target::Web => {
                self.problem(ProblemKind::WebPictureAsLink, url);
                let text = if description.is_empty() {
                    url
                } else {
                    &description
                };
                self.replace_children(node, text);
                let link = NodeLink {
                    url: url.to_owned(),
                    title: String::new(),
                };
                node.data_mut().value = NodeValue::Link(Box::new(link));
            }
            Target::File(path) => self.picture_in_vault(node, &path, url, &description),
            Target::Note(_) | Target::Missing | Target::Other => {
                self.picture_missing(node, url, &description);
            }
        }
    }

    fn picture_in_vault(
        &mut self,
        node: &'a AstNode<'a>,
        path: &str,
        url: &str,
        description: &str,
    ) {
        let Some(mime) = picture_mime(path) else {
            // Written as a picture, but another kind of file.
            let link = NodeLink {
                url: url.to_owned(),
                title: String::new(),
            };
            node.data_mut().value = NodeValue::Link(Box::new(link));
            self.link_to_file(node, path, url);
            return;
        };
        let copying = self.rules.copy_attachments;
        if copying && !needs_converting(mime) && !self.rules.upright_pictures {
            self.copy(path, url);
            set_url(node, &href_of(path));
            return;
        }
        let Ok(bytes) = self.rules.vault.read_linked_file(&self.note.path, url) else {
            if copying {
                // The copy finds out and reports it.
                self.copy(path, url);
                set_url(node, &href_of(path));
            } else {
                self.picture_missing(node, url, description);
            }
            return;
        };
        let turning = self.rules.upright_pictures && is_turned(&bytes);
        let converted = (needs_converting(mime) || turning)
            .then(|| convert_picture(&bytes))
            .flatten();
        if needs_converting(mime) && converted.is_none() {
            self.problem(ProblemKind::PictureNotConverted, url);
        }
        match (converted, copying) {
            (Some(picture), true) => {
                let target = format!("{path}.{}", picture.extension);
                set_url(node, &href_of(&target));
                self.collected.copies.entry(target).or_insert_with(|| Copy {
                    path: self.note.path.clone(),
                    title: self.note.title.clone(),
                    link: url.to_owned(),
                    converted: Some(picture.bytes),
                });
            }
            (Some(picture), false) => set_data_url(node, picture.media_type, &picture.bytes),
            (None, true) => {
                self.copy(path, url);
                set_url(node, &href_of(path));
            }
            (None, false) => set_data_url(node, mime, &bytes),
        }
    }

    fn picture_missing(&mut self, node: &'a AstNode<'a>, url: &str, description: &str) {
        self.problem(ProblemKind::PictureMissing, url);
        let description = if description.is_empty() {
            "picture"
        } else {
            description
        };
        let text = NodeValue::Text(format!("[{description}]").into());
        node.insert_before(self.arena.alloc(text.into()));
        node.detach();
    }

    fn replace_children(&self, node: &'a AstNode<'a>, text: &str) {
        for child in node.children().collect::<Vec<_>>() {
            child.detach();
        }
        node.append(
            self.arena
                .alloc(NodeValue::Text(text.to_owned().into()).into()),
        );
    }

    /// An `abc` block becomes its engraved tunes (ADR-0033): figures in the
    /// page for HTML and PDF, pictures for the others, followed by the block
    /// if its source is asked for. Without them its source stays, and the
    /// report says so.
    fn score(&mut self, node: &'a AstNode<'a>, source: &str, transpose: i32) {
        let Some(tunes) = self.rules.music.tunes(source, transpose) else {
            self.problem(ProblemKind::ScoreNotEngraved, &music::title_of(source));
            return;
        };
        if self.rules.music.output == Output::Html {
            let mut figures = String::new();
            for svg in &tunes {
                let _ = writeln!(figures, "<figure class=\"score\">{svg}</figure>");
            }
            let figures = NodeValue::HtmlBlock(NodeHtmlBlock {
                block_type: 6,
                literal: figures,
            });
            node.insert_before(self.arena.alloc(figures.into()));
        } else {
            self.pictures(node, source, &tunes);
        }
        if !self.rules.music.abc_source {
            node.detach();
        }
    }

    /// Each tune as a picture in a paragraph of its own, before `node`.
    fn pictures(&self, node: &'a AstNode<'a>, source: &str, tunes: &[String]) {
        let title = music::title_of(source);
        for svg in tunes {
            let paragraph: &'a AstNode<'a> = self.arena.alloc(NodeValue::Paragraph.into());
            let link = NodeLink {
                url: String::new(),
                title: String::new(),
            };
            let picture: &'a AstNode<'a> =
                self.arena.alloc(NodeValue::Image(Box::new(link)).into());
            picture.append(
                self.arena
                    .alloc(NodeValue::Text(title.clone().into()).into()),
            );
            set_data_url(picture, "image/svg+xml", svg.as_bytes());
            paragraph.append(picture);
            node.insert_before(paragraph);
        }
    }

    /// A `chordpro` block becomes its song sheet (ADR-0034), written for the
    /// format, as the tree cannot hold chords over syllables.
    fn song(&self, node: &'a AstNode<'a>, source: &str, info: &str) {
        let sheet = songs::render(source, info, self.rules.vault.chord_notation());
        let written = match self.rules.music.output {
            Output::Html => songs::html(&sheet),
            Output::Odt => songs::odt(&sheet),
            Output::Docx => songs::raw_block("openxml", &songs::docx(&sheet)),
            Output::Epub => songs::raw_block("html", &songs::html(&sheet)),
        };
        node.data_mut().value = NodeValue::Raw(written);
    }

    fn task(&self, node: &'a AstNode<'a>, done: bool) {
        let glyph = if done { "☑ " } else { "☐ " };
        let text: &'a AstNode<'a> = self.arena.alloc(NodeValue::Text(glyph.into()).into());
        match node.first_child() {
            Some(first) if matches!(first.data().value, NodeValue::Paragraph) => {
                first.prepend(text);
            }
            _ => {
                let paragraph: &'a AstNode<'a> = self.arena.alloc(NodeValue::Paragraph.into());
                paragraph.append(text);
                node.prepend(paragraph);
            }
        }
    }
}

fn set_data_url<'a>(node: &'a AstNode<'a>, mime: &str, bytes: &[u8]) {
    let data = base64::engine::general_purpose::STANDARD.encode(bytes);
    set_url(node, &format!("data:{mime};base64,{data}"));
}

pub(crate) fn set_url<'a>(node: &'a AstNode<'a>, url: &str) {
    if let NodeValue::Link(link) | NodeValue::Image(link) = &mut node.data_mut().value {
        url.clone_into(&mut link.url);
    }
}

/// `path` as a relative link: comrak leaves `%` as it is, and `#` or `?` in a
/// file name would end the path.
pub(crate) fn href_of(path: &str) -> String {
    const KEEP: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
        .remove(b'-')
        .remove(b'_')
        .remove(b'.')
        .remove(b'~')
        .remove(b'/');
    percent_encoding::utf8_percent_encode(path, KEEP).to_string()
}

fn is_note_link(url: &str) -> bool {
    let path = url.split(['#', '?']).next().unwrap_or_default();
    path.to_ascii_lowercase().ends_with(".md")
}

/// Math is exported as its source (ADR-0021 rule 8): a formula that is a
/// whole paragraph as a code block, else as code in the text.
fn math<'a>(node: &'a AstNode<'a>, display: bool, literal: &str) {
    let parent = node.parent();
    let alone = node.next_sibling().is_none() && node.previous_sibling().is_none();
    if let Some(paragraph) =
        parent.filter(|p| display && alone && matches!(p.data().value, NodeValue::Paragraph))
    {
        let block = NodeCodeBlock {
            fenced: true,
            fence_char: b'`',
            fence_length: 3,
            fence_offset: 0,
            info: String::new(),
            literal: format!("$${literal}$$\n"),
            closed: true,
        };
        node.detach();
        paragraph.data_mut().value = NodeValue::CodeBlock(Box::new(block));
        return;
    }
    let delimiter = if display { "$$" } else { "$" };
    node.data_mut().value = NodeValue::Code(NodeCode {
        num_backticks: 1,
        literal: format!("{delimiter}{literal}{delimiter}"),
    });
}

/// Pictures of this type show in hardly any program but Apple's (ADR-0027).
fn needs_converting(mime: &str) -> bool {
    matches!(mime, "image/heic" | "image/heif" | "image/tiff")
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
use schnipsle_apple::{convert_picture, is_turned};

/// No system converter here, so the picture stays as it is and is reported.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn convert_picture(_bytes: &[u8]) -> Option<Converted> {
    None
}

/// Without a system converter there is nothing to turn a picture with.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
fn is_turned(_bytes: &[u8]) -> bool {
    false
}

/// What [`convert_picture`] gives on the platforms that have one.
#[cfg(not(any(target_os = "macos", target_os = "ios")))]
struct Converted {
    bytes: Vec<u8>,
    media_type: &'static str,
    extension: &'static str,
}

/// The media type of a picture file by its extension, for a `data:` URL.
fn picture_mime(path: &str) -> Option<&'static str> {
    let extension = path.rsplit_once('.')?.1.to_ascii_lowercase();
    Some(match extension.as_str() {
        "apng" => "image/apng",
        "avif" => "image/avif",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "heic" => "image/heic",
        "heif" => "image/heif",
        "jpeg" | "jpg" => "image/jpeg",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "webp" => "image/webp",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tag_line(markdown: &str) -> Option<String> {
        let arena = Arena::new();
        let root = parse_document(&arena, markdown, &parse_options());
        final_tag_line(root).map(plain_text)
    }

    #[test]
    fn a_final_tag_line_is_a_last_paragraph_of_nothing_but_tags() {
        assert_eq!(tag_line("Text\n\n#a #b/c\n").as_deref(), Some("#a #b/c"));
        assert_eq!(tag_line("Text\n\n#a #a\n").as_deref(), Some("#a #a"));
        assert_eq!(tag_line("Text\n\n#a\n#b\n").as_deref(), Some("#a #b"));
        // Footnotes end up behind it in the tree.
        assert_eq!(tag_line("A[^1]\n\n[^1]: n\n\n#a\n").as_deref(), Some("#a"));
        for other in [
            "",
            "Text #a\n",
            "#a and\n",
            "#a.\n",
            "#123\n",
            "#00ff00\n",
            "\\#a\n",
            "`#a`\n",
            "*#a*\n",
            "#a\n\nText\n",
            "- #a\n",
        ] {
            assert_eq!(tag_line(other), None, "{other}");
        }
    }

    /// `markdown` as HTML through [`prepare`], with the problems found.
    fn prepared(markdown: &str, copy_attachments: bool, web_pictures: bool) -> (String, Collected) {
        prepared_with(
            markdown,
            copy_attachments,
            web_pictures,
            Music::default(),
            false,
        )
    }

    /// As [`prepared`], with engraved scores, and with `raw`, raw HTML and
    /// `data:` URLs of SVG written out.
    fn prepared_with(
        markdown: &str,
        copy_attachments: bool,
        web_pictures: bool,
        music: Music<'_>,
        raw: bool,
    ) -> (String, Collected) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        vault.create_note("note.md", markdown).unwrap();
        std::fs::write(dir.path().join("notes/manual.pdf"), b"%PDF").unwrap();
        let note = vault.read_note_text("note.md").unwrap();
        let note = Source {
            path: note.path,
            title: note.title,
            text: note.text,
        };
        let rules = Rules {
            vault: &vault,
            hrefs: HashMap::new(),
            titles: HashMap::new(),
            out_of_reach: HashSet::new(),
            copy_attachments,
            web_pictures,
            keep_tag_line: false,
            sections: false,
            upright_pictures: false,
            music,
        };
        let mut collected = Collected::default();
        let arena = Arena::new();
        let root = prepare(&arena, &note, "n", &rules, &mut collected);
        let mut out = String::new();
        let mut options = parse_options();
        options.render.r#unsafe = raw;
        comrak::format_html(root, &options, &mut out).unwrap();
        (out, collected)
    }

    const SCORE_NOTE: &str =
        "Vorher\n\n```abc transpose=2\nT:Kanon\nK:D\nfe|\n```\n\n```abc\nT:Ohne\nK:C\nC|\n```\n";

    fn engraved() -> Vec<crate::Score> {
        vec![crate::Score {
            source: "T:Kanon\nK:D\nfe|\n".to_owned(),
            transpose: 2,
            tunes: vec![
                "<svg width=\"10\" height=\"5\"><style>x</style><text>Eins</text></svg>".to_owned(),
                "<svg width=\"10\" height=\"5\"><text>Zwei</text></svg>".to_owned(),
            ],
        }]
    }

    #[test]
    fn a_score_goes_into_the_page_as_figures() {
        let scores = engraved();
        let (html, collected) = prepared_with(
            SCORE_NOTE,
            false,
            false,
            Music::of(&scores, Output::Html, false),
            true,
        );
        assert!(
            html.contains(concat!(
                "<figure class=\"score\"><svg width=\"10\" height=\"5\" viewBox=\"0 0 10 5\"><text>Eins</text></svg></figure>\n",
                "<figure class=\"score\"><svg width=\"10\" height=\"5\" viewBox=\"0 0 10 5\"><text>Zwei</text></svg></figure>\n",
            )),
            "{html}"
        );
        // The block without a score stays source, and is reported.
        assert!(
            html.contains("<code class=\"language-abc\">T:Ohne"),
            "{html}"
        );
        let problems: Vec<_> = collected
            .problems
            .iter()
            .map(|p| (p.kind, p.target.as_str()))
            .collect();
        assert_eq!(problems, [(ProblemKind::ScoreNotEngraved, "Ohne")]);
    }

    #[test]
    fn a_score_is_a_picture_per_tune_elsewhere() {
        let scores = engraved();
        let (html, collected) = prepared_with(
            SCORE_NOTE,
            false,
            false,
            Music::of(&scores, Output::Odt, false),
            true,
        );
        let pictures: Vec<&str> = html
            .match_indices("<p><img src=\"data:image/svg+xml;base64,")
            .map(|(i, _)| &html[i..])
            .collect();
        assert_eq!(pictures.len(), 2, "{html}");
        assert!(pictures[0].contains("alt=\"Kanon\""));
        assert_eq!(collected.problems.len(), 1);
    }

    #[test]
    fn the_source_of_a_score_stays_under_it_if_asked_for() {
        let scores = engraved();
        for output in [Output::Html, Output::Odt] {
            let music = Music::of(&scores, output, true);
            let (html, _) = prepared_with(SCORE_NOTE, false, false, music, true);
            let score = html.find("<figure").or_else(|| html.find("<img")).unwrap();
            let source = html.find("<code class=\"language-abc\">T:Kanon").unwrap();
            assert!(score < source, "{html}");
        }
    }

    #[test]
    fn a_score_with_another_transposition_is_not_taken() {
        let scores = engraved();
        let note = SCORE_NOTE.replace("transpose=2", "transpose=3");
        let (html, collected) = prepared_with(
            &note,
            false,
            false,
            Music::of(&scores, Output::Html, false),
            true,
        );
        assert!(!html.contains("<figure"), "{html}");
        assert_eq!(collected.problems.len(), 2);
    }

    #[test]
    fn pictures_on_the_web_become_links_where_they_cannot_be_shown() {
        let markdown = "![A view](https://example.org/a.jpg) ![](https://example.org/b.jpg)\n";
        let (html, collected) = prepared(markdown, false, true);
        assert!(
            html.contains("<img src=\"https://example.org/a.jpg\" alt=\"A view\""),
            "{html}"
        );
        assert!(collected.problems.is_empty());
        let (html, collected) = prepared(markdown, false, false);
        assert!(
            html.contains("<a href=\"https://example.org/a.jpg\">A view</a>"),
            "{html}"
        );
        assert!(
            html.contains("<a href=\"https://example.org/b.jpg\">https://example.org/b.jpg</a>"),
            "{html}"
        );
        assert_eq!(collected.problems.len(), 2);
        assert_eq!(collected.problems[0].kind, ProblemKind::WebPictureAsLink);
    }

    #[test]
    fn a_file_written_as_a_picture_is_treated_as_the_attachment_it_is() {
        let markdown = "![The manual](manual.pdf)\n";
        let (html, collected) = prepared(markdown, false, true);
        assert_eq!(html, "<p>The manual</p>\n");
        assert_eq!(
            collected.problems[0].kind,
            ProblemKind::AttachmentNotIncluded
        );
        let (html, collected) = prepared(markdown, true, true);
        assert_eq!(html, "<p><a href=\"notes/manual.pdf\">The manual</a></p>\n");
        assert!(collected.problems.is_empty());
        assert_eq!(collected.copies["notes/manual.pdf"].link, "manual.pdf");
    }

    #[test]
    fn a_missing_picture_without_a_description_says_so() {
        let (html, collected) = prepared("![](gone.png)\n", false, true);
        assert_eq!(html, "<p>[picture]</p>\n");
        assert_eq!(collected.problems[0].kind, ProblemKind::PictureMissing);
    }

    #[test]
    fn links_to_files_are_safe_paths() {
        assert_eq!(href_of("a b/C#.html"), "a%20b/C%23.html");
        assert_eq!(href_of("Grüße?.html"), "Gr%C3%BC%C3%9Fe%3F.html");
    }

    #[test]
    fn pictures_are_known_by_their_extension() {
        assert_eq!(picture_mime("a/b.PNG"), Some("image/png"));
        assert_eq!(picture_mime("a/b.svg"), Some("image/svg+xml"));
        assert_eq!(picture_mime("a/b.pdf"), None);
        assert_eq!(picture_mime("noextension"), None);
    }
}
