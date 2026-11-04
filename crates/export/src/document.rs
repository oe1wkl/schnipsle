// SPDX-License-Identifier: Apache-2.0
//! From a note to the syntax tree of what an export shows (ADR-0021).
//!
//! All formats write the tree this module makes, so a note looks alike in all
//! of them. Front matter and the final tag line are gone; links and pictures
//! point where they should in this export, or are text; raw HTML is clean
//! ([`crate::raw_html`]); math is code; a task item starts with ☐ or ☑.

use std::collections::{BTreeMap, HashMap, HashSet};

use base64::Engine as _;
use comrak::nodes::{
    AstNode, NodeCode, NodeCodeBlock, NodeFootnoteDefinition, NodeHeading, NodeLink, NodeValue,
};
use comrak::{Arena, parse_document};
use schnipsle_core::attachments::LinkTarget;
use schnipsle_core::dates::parse_rfc3339;
use schnipsle_core::front_matter;
use schnipsle_core::names::slug;
use schnipsle_core::tags::{extract_tags, normalize, parse_options};
use schnipsle_core::vault::{Vault, VaultError};
use unicode_normalization::UnicodeNormalization;

use crate::output;
use crate::raw_html;
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
}

/// What preparing notes collected.
#[derive(Default)]
pub(crate) struct Collected {
    pub problems: Vec<Problem>,
    /// Files that got a number, as [`crate::Report::numbered`] says.
    pub numbered: Vec<String>,
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
/// 3). A first paragraph of one line is the title, so it becomes the heading
/// rather than repeating it.
fn start_with_title<'a>(arena: &'a Arena<'a>, root: &'a AstNode<'a>, title: &str) {
    let heading = NodeValue::Heading(NodeHeading {
        level: 1,
        setext: false,
        closed: false,
    });
    if let Some(first) = root.first_child() {
        let value = &first.data().value;
        if matches!(value, NodeValue::Heading(h) if h.level == 1) {
            return;
        }
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
fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
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
        if self.rules.copy_attachments {
            self.copy(path, url);
            set_url(node, &href_of(path));
            return;
        }
        match self.rules.vault.read_linked_file(&self.note.path, url) {
            Ok(bytes) => {
                let data = base64::engine::general_purpose::STANDARD.encode(bytes);
                set_url(node, &format!("data:{mime};base64,{data}"));
            }
            Err(_) => self.picture_missing(node, url, description),
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

fn set_url<'a>(node: &'a AstNode<'a>, url: &str) {
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
        };
        let mut collected = Collected::default();
        let arena = Arena::new();
        let root = prepare(&arena, &note, "n", &rules, &mut collected);
        let mut out = String::new();
        comrak::format_html(root, &parse_options(), &mut out).unwrap();
        (out, collected)
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
