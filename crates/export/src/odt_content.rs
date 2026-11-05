// SPDX-License-Identifier: Apache-2.0
//! The body of an ODT file: `OpenDocument` XML for the tree of [`crate::document`]
//! (ADR-0010). The look is in `odt_styles.xml`; this module only names styles.
//! Pictures come as the `data:` URLs the tree has, and leave as files for the
//! package. Raw HTML keeps only its text (ADR-0021 rule 9).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use base64::Engine as _;
use comrak::nodes::{AstNode, ListType, NodeLink, NodeValue, TableAlignment};

/// How many levels of quotes have a style of their own.
const QUOTE_STYLES: [&str; 3] = ["Quotations", "Quotations_20_2", "Quotations_20_3"];

/// Centimetres per CSS pixel, for the size a picture has in a browser.
const CM_PER_PIXEL: f64 = 2.54 / 96.0;

/// Width and height of a picture whose size its file does not tell.
const UNKNOWN_WIDTH: f64 = 8.0;

/// Room a table cell's padding takes from a picture in it.
const CELL_PADDING: f64 = 0.3;

/// Marks a footnote referred to from within another, as it cannot be in it.
const MARK: &str = "<text:span text:style-name=\"Superscript\">*</text:span>";

/// A picture for the package.
pub(crate) struct Picture {
    /// Where it goes in the package, as `Pictures/1.png`.
    pub path: String,
    pub media_type: String,
    pub bytes: Vec<u8>,
    size: Option<(f64, f64)>,
}

/// Where in the document the writer is.
#[derive(Clone, Copy)]
struct Place {
    /// The style of a paragraph here.
    paragraph: &'static str,
    quotes: usize,
    /// In a list, which can only hold paragraphs, headings and lists.
    in_list: bool,
    /// In a footnote, which cannot hold another.
    in_note: bool,
    /// The widest a picture can be here, in cm.
    width: f64,
}

/// Collects the body of one document.
pub(crate) struct Content {
    pub body: String,
    pub pictures: Vec<Picture>,
    /// Pictures by their URL, each one stored once.
    known: HashMap<String, usize>,
    text_width: f64,
    /// The highest a picture can be, in cm: taller than this, an office program cuts it off.
    text_height: f64,
    notes: usize,
    tables: usize,
    frames: usize,
    /// The next paragraph or heading starts here: a link target.
    bookmark: Option<String>,
    /// The next level-1 heading starts a new page.
    page_break: bool,
    /// Footnotes referred to from within a footnote, to add to it.
    nested: Vec<String>,
    /// Starts the next paragraph: the mark of a footnote added to another.
    mark: Option<&'static str>,
}

impl Content {
    pub fn new((text_width, text_height): (f64, f64)) -> Self {
        Self {
            body: String::new(),
            pictures: Vec::new(),
            known: HashMap::new(),
            text_width,
            text_height,
            notes: 0,
            tables: 0,
            frames: 0,
            bookmark: None,
            page_break: false,
            nested: Vec::new(),
            mark: None,
        }
    }

    /// Writes a note. Links to `anchor` go to its start; with `page_break` its
    /// level-1 heading starts a new page.
    pub fn note<'a>(&mut self, root: &'a AstNode<'a>, anchor: &str, page_break: bool) {
        self.bookmark = Some(anchor.to_owned());
        self.page_break = page_break;
        let definitions: HashMap<String, &'a AstNode<'a>> = root
            .children()
            .filter_map(|child| match &child.data().value {
                NodeValue::FootnoteDefinition(definition) => Some((definition.name.clone(), child)),
                _ => None,
            })
            .collect();
        let place = Place {
            paragraph: "Text_20_body",
            quotes: 0,
            in_list: false,
            in_note: false,
            width: self.text_width,
        };
        self.blocks(root, place, &definitions);
    }

    fn blocks<'a>(
        &mut self,
        parent: &'a AstNode<'a>,
        place: Place,
        notes: &HashMap<String, &'a AstNode<'a>>,
    ) {
        for child in parent.children() {
            match &child.data().value {
                NodeValue::Paragraph => {
                    self.open_paragraph(place.paragraph);
                    self.inlines(child, place, notes);
                    self.body.push_str("</text:p>\n");
                }
                NodeValue::Heading(heading) => self.heading(child, heading.level, place, notes),
                NodeValue::List(list) => {
                    let ordered = list.list_type == ListType::Ordered;
                    self.list(child, ordered, list.start, place, notes);
                }
                NodeValue::BlockQuote | NodeValue::MultilineBlockQuote(_) => {
                    let quotes = place.quotes + 1;
                    let inner = Place {
                        paragraph: QUOTE_STYLES[quotes.min(QUOTE_STYLES.len()) - 1],
                        quotes,
                        ..place
                    };
                    self.blocks(child, inner, notes);
                }
                NodeValue::CodeBlock(block) => {
                    self.open_paragraph("Preformatted_20_Text");
                    write_source(&mut self.body, block.literal.trim_end_matches('\n'));
                    self.body.push_str("</text:p>\n");
                }
                NodeValue::ThematicBreak => {
                    self.open_paragraph("Horizontal_20_Line");
                    self.body.push_str("</text:p>\n");
                }
                NodeValue::Table(table) => {
                    if place.in_list {
                        self.table_as_paragraphs(child, place, notes);
                    } else {
                        self.table(child, &table.alignments, place, notes);
                    }
                }
                NodeValue::FootnoteDefinition(_) => {}
                _ => self.blocks(child, place, notes),
            }
        }
    }

    fn open_paragraph(&mut self, style: &str) {
        let _ = write!(self.body, "<text:p text:style-name=\"{style}\">");
        self.write_bookmark();
        if let Some(mark) = self.mark.take() {
            self.body.push_str(mark);
        }
    }

    fn write_bookmark(&mut self) {
        if let Some(name) = self.bookmark.take() {
            let _ = write!(
                self.body,
                "<text:bookmark text:name=\"{}\"/>",
                escape(&name)
            );
        }
    }

    fn heading<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        level: u8,
        place: Place,
        notes: &HashMap<String, &'a AstNode<'a>>,
    ) {
        let style = if level == 1 && std::mem::take(&mut self.page_break) {
            "Note_20_Start".to_owned()
        } else {
            format!("Heading_20_{level}")
        };
        let _ = write!(
            self.body,
            "<text:h text:style-name=\"{style}\" text:outline-level=\"{level}\">"
        );
        self.write_bookmark();
        self.inlines(node, place, notes);
        self.body.push_str("</text:h>\n");
    }

    fn list<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        ordered: bool,
        start: usize,
        place: Place,
        notes: &HashMap<String, &'a AstNode<'a>>,
    ) {
        let style = if ordered { "Numbering" } else { "Bullets" };
        let _ = writeln!(self.body, "<text:list text:style-name=\"{style}\">");
        let inner = Place {
            paragraph: if place.quotes == 0 && !place.in_note {
                "List_20_Contents"
            } else {
                place.paragraph
            },
            in_list: true,
            ..place
        };
        for (index, item) in node.children().enumerate() {
            let task = matches!(item.data().value, NodeValue::TaskItem(_));
            self.body.push_str("<text:list-item");
            if task {
                self.body.push_str(" text:style-override=\"Tasks\"");
            }
            if ordered && index == 0 && start != 1 {
                let _ = write!(self.body, " text:start-value=\"{start}\"");
            }
            self.body.push_str(">\n");
            if item.first_child().is_none() {
                self.open_paragraph(inner.paragraph);
                self.body.push_str("</text:p>\n");
            }
            self.blocks(item, inner, notes);
            self.body.push_str("</text:list-item>\n");
        }
        self.body.push_str("</text:list>\n");
    }

    fn table<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        alignments: &[TableAlignment],
        place: Place,
        notes: &HashMap<String, &'a AstNode<'a>>,
    ) {
        self.tables += 1;
        let _ = writeln!(
            self.body,
            "<table:table table:name=\"Table{}\" table:style-name=\"Table\">\n<table:table-column table:number-columns-repeated=\"{}\"/>",
            self.tables,
            alignments.len().max(1)
        );
        #[allow(clippy::cast_precision_loss)]
        let cell_width = place.width / alignments.len().max(1) as f64 - CELL_PADDING;
        let cell_place = Place {
            width: cell_width.max(1.0),
            ..place
        };
        for row in node.children() {
            let header = matches!(row.data().value, NodeValue::TableRow(true));
            if header {
                self.body.push_str("<table:table-header-rows>\n");
            }
            self.body
                .push_str("<table:table-row table:style-name=\"Row\">\n");
            for (cell, alignment) in row.children().zip(
                alignments
                    .iter()
                    .chain(std::iter::repeat(&TableAlignment::None)),
            ) {
                let cell_style = if header { "Heading_20_Cell" } else { "Cell" };
                let _ = write!(
                    self.body,
                    "<table:table-cell table:style-name=\"{cell_style}\">"
                );
                self.open_paragraph(cell_paragraph(header, *alignment));
                self.inlines(cell, cell_place, notes);
                self.body.push_str("</text:p></table:table-cell>\n");
            }
            self.body.push_str("</table:table-row>\n");
            if header {
                self.body.push_str("</table:table-header-rows>\n");
            }
        }
        self.body.push_str("</table:table>\n");
    }

    /// A table in a list: `OpenDocument` has no place for it there, so each row
    /// becomes a paragraph with tabs between its cells.
    fn table_as_paragraphs<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        place: Place,
        notes: &HashMap<String, &'a AstNode<'a>>,
    ) {
        for row in node.children() {
            self.open_paragraph(place.paragraph);
            for (index, cell) in row.children().enumerate() {
                if index > 0 {
                    self.body.push_str("<text:tab/>");
                }
                self.inlines(cell, place, notes);
            }
            self.body.push_str("</text:p>\n");
        }
    }

    fn inlines<'a>(
        &mut self,
        parent: &'a AstNode<'a>,
        place: Place,
        notes: &HashMap<String, &'a AstNode<'a>>,
    ) {
        for child in parent.children() {
            match &child.data().value {
                NodeValue::Text(text) => self.body.push_str(&escape(text)),
                NodeValue::SoftBreak => self.body.push(' '),
                NodeValue::LineBreak => self.body.push_str("<text:line-break/>"),
                NodeValue::Code(code) => {
                    self.body
                        .push_str("<text:span text:style-name=\"Source_20_Text\">");
                    write_source(&mut self.body, &code.literal);
                    self.body.push_str("</text:span>");
                }
                NodeValue::Emph => self.span(child, "Emphasis", place, notes),
                NodeValue::Strong => self.span(child, "Strong_20_Emphasis", place, notes),
                NodeValue::Strikethrough => self.span(child, "Strikethrough", place, notes),
                NodeValue::Underline => self.span(child, "Underline", place, notes),
                NodeValue::Superscript => self.span(child, "Superscript", place, notes),
                NodeValue::Subscript => self.span(child, "Subscript", place, notes),
                NodeValue::Highlight => self.span(child, "Highlight", place, notes),
                NodeValue::Link(link) => self.link(child, link, place, notes),
                NodeValue::Image(link) => self.picture(child, link, place),
                NodeValue::FootnoteReference(reference) => {
                    self.footnote(&reference.name, place, notes);
                }
                // What is left of raw HTML is its tags.
                NodeValue::HtmlInline(_) => {}
                _ => self.inlines(child, place, notes),
            }
        }
    }

    fn span<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        style: &str,
        place: Place,
        notes: &HashMap<String, &'a AstNode<'a>>,
    ) {
        let _ = write!(self.body, "<text:span text:style-name=\"{style}\">");
        self.inlines(node, place, notes);
        self.body.push_str("</text:span>");
    }

    fn link<'a>(
        &mut self,
        node: &'a AstNode<'a>,
        link: &NodeLink,
        place: Place,
        notes: &HashMap<String, &'a AstNode<'a>>,
    ) {
        let _ = write!(
            self.body,
            "<text:a xlink:type=\"simple\" xlink:href=\"{}\" text:style-name=\"Internet_20_link\" text:visited-style-name=\"Internet_20_link\">",
            escape(&href(&link.url))
        );
        self.inlines(node, place, notes);
        self.body.push_str("</text:a>");
    }

    fn picture<'a>(&mut self, node: &'a AstNode<'a>, link: &NodeLink, place: Place) {
        let description = plain_text(node);
        let Some(index) = self.stored_picture(&link.url) else {
            // Not a picture the export can hold; its description stays.
            let _ = write!(self.body, "[{}]", escape(&description));
            return;
        };
        self.frames += 1;
        let picture = &self.pictures[index];
        let size = if let Some((width, height)) = picture.size {
            let scale = (place.width / width)
                .min(self.text_height / height)
                .min(1.0);
            (width * scale, height * scale)
        } else {
            let width = UNKNOWN_WIDTH.min(place.width);
            (width, width)
        };
        let _ = write!(
            self.body,
            "<draw:frame draw:style-name=\"Picture\" draw:name=\"Picture{}\" text:anchor-type=\"as-char\" svg:width=\"{:.3}cm\" svg:height=\"{:.3}cm\"><draw:image xlink:href=\"{}\" xlink:type=\"simple\" xlink:show=\"embed\" xlink:actuate=\"onLoad\"/>",
            self.frames,
            size.0,
            size.1,
            escape(&picture.path)
        );
        if !description.is_empty() {
            let _ = write!(self.body, "<svg:desc>{}</svg:desc>", escape(&description));
        }
        self.body.push_str("</draw:frame>");
    }

    /// The picture of a `data:` URL, stored for the package.
    fn stored_picture(&mut self, url: &str) -> Option<usize> {
        if let Some(index) = self.known.get(url) {
            return Some(*index);
        }
        let (media_type, data) = url.strip_prefix("data:")?.split_once(";base64,")?;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(data)
            .ok()?;
        let size = size_of_picture(media_type, &bytes);
        let index = self.pictures.len();
        self.pictures.push(Picture {
            path: format!("Pictures/{}.{}", index + 1, extension(media_type)),
            media_type: media_type.to_owned(),
            bytes,
            size,
        });
        self.known.insert(url.to_owned(), index);
        Some(index)
    }

    /// A real footnote for each reference to it. A footnote cannot hold another,
    /// so one referred to from within a footnote gets a `*` there and comes after it.
    fn footnote<'a>(&mut self, name: &str, place: Place, notes: &HashMap<String, &'a AstNode<'a>>) {
        if place.in_note {
            self.body.push_str(MARK);
            self.nested.push(name.to_owned());
            return;
        }
        let Some(definition) = notes.get(name) else {
            return;
        };
        self.notes += 1;
        let _ = write!(
            self.body,
            "<text:note text:id=\"ftn{0}\" text:note-class=\"footnote\"><text:note-citation>{0}</text:note-citation><text:note-body>",
            self.notes
        );
        let inner = Place {
            paragraph: "Footnote",
            quotes: 0,
            in_list: false,
            in_note: true,
            ..place
        };
        self.blocks(definition, inner, notes);
        let mut written = HashSet::from([name.to_owned()]);
        while let Some(next) = self.nested.pop() {
            if written.insert(next.clone())
                && let Some(definition) = notes.get(&next)
            {
                self.mark = Some(MARK);
                self.blocks(definition, inner, notes);
            }
        }
        self.body.push_str("</text:note-body></text:note>");
    }
}

/// The size of a picture in cm, as it has in a browser.
fn size_of_picture(media_type: &str, bytes: &[u8]) -> Option<(f64, f64)> {
    if media_type == "image/svg+xml" {
        return size_of_svg(&String::from_utf8_lossy(bytes));
    }
    let size = imagesize::blob_size(bytes).ok()?;
    #[allow(clippy::cast_precision_loss)]
    Some((
        size.width as f64 * CM_PER_PIXEL,
        size.height as f64 * CM_PER_PIXEL,
    ))
}

/// The size of an SVG in cm: from `width` and `height` of its root element, or
/// from its `viewBox`, whose units count as pixels.
fn size_of_svg(svg: &str) -> Option<(f64, f64)> {
    let start = svg.find("<svg")?;
    let root = &svg[start..start + svg[start..].find('>')?];
    let length = |name| attribute(root, name).and_then(cm_of_length);
    if let (Some(width), Some(height)) = (length("width"), length("height")) {
        return Some((width, height));
    }
    let view_box = attribute(root, "viewBox")?;
    let numbers: Vec<f64> = view_box
        .split([' ', ','])
        .filter(|part| !part.is_empty())
        .map(str::parse)
        .collect::<Result<_, _>>()
        .ok()?;
    match numbers.as_slice() {
        [_, _, width, height] if *width > 0.0 && *height > 0.0 => {
            Some((width * CM_PER_PIXEL, height * CM_PER_PIXEL))
        }
        _ => None,
    }
}

/// The value of an attribute in the text of an element.
fn attribute<'t>(element: &'t str, name: &str) -> Option<&'t str> {
    let mut rest = element;
    while let Some(at) = rest.find(name) {
        let before = rest[..at].chars().next_back();
        let after = rest[at + name.len()..].trim_start();
        rest = &rest[at + name.len()..];
        if !before.is_some_and(char::is_whitespace) {
            continue;
        }
        let Some(value) = after.strip_prefix('=').map(str::trim_start) else {
            continue;
        };
        let quote = value.chars().next().filter(|c| matches!(c, '"' | '\''))?;
        let value = &value[1..];
        return value.find(quote).map(|end| &value[..end]);
    }
    None
}

/// A length of CSS in cm; `None` for a percentage or anything else relative.
fn cm_of_length(length: &str) -> Option<f64> {
    let length = length.trim();
    let digits = length
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(length.len());
    let number: f64 = length[..digits].parse().ok()?;
    let per_unit = match length[digits..].trim() {
        "" | "px" => CM_PER_PIXEL,
        "pt" => 2.54 / 72.0,
        "pc" => 2.54 / 6.0,
        "in" => 2.54,
        "cm" => 1.0,
        "mm" => 0.1,
        _ => return None,
    };
    (number > 0.0).then_some(number * per_unit)
}

fn cell_paragraph(header: bool, alignment: TableAlignment) -> &'static str {
    match (header, alignment) {
        (false, TableAlignment::Center) => "Table_20_Center",
        (false, TableAlignment::Right) => "Table_20_Right",
        (false, _) => "Table_20_Contents",
        (true, TableAlignment::Center) => "Table_20_Heading_20_Center",
        (true, TableAlignment::Right) => "Table_20_Heading_20_Right",
        (true, _) => "Table_20_Heading",
    }
}

/// A link for `xlink:href`. A target in the document is the name of a bookmark,
/// without the escapes of an HTML link.
fn href(url: &str) -> String {
    match url.strip_prefix('#') {
        Some(name) => format!(
            "#{}",
            percent_encoding::percent_decode_str(name).decode_utf8_lossy()
        ),
        None => url.to_owned(),
    }
}

fn extension(media_type: &str) -> &str {
    match media_type {
        "image/jpeg" => "jpg",
        "image/svg+xml" => "svg",
        "image/apng" => "png",
        other => other.strip_prefix("image/").unwrap_or("bin"),
    }
}

fn plain_text<'a>(node: &'a AstNode<'a>) -> String {
    let mut out = String::new();
    for descendant in node.descendants() {
        match &descendant.data().value {
            NodeValue::Text(text) => out.push_str(text),
            NodeValue::Code(code) => out.push_str(&code.literal),
            NodeValue::SoftBreak | NodeValue::LineBreak => out.push(' '),
            _ => {}
        }
    }
    out
}

/// `text` where every space counts, as in code.
fn write_source(out: &mut String, text: &str) {
    let mut spaces = 0;
    let mut first = true;
    for c in text.chars() {
        if c == ' ' {
            spaces += 1;
            continue;
        }
        write_spaces(out, &mut spaces, first);
        first = false;
        match c {
            '\t' => out.push_str("<text:tab/>"),
            '\n' => {
                out.push_str("<text:line-break/>");
                first = true;
            }
            _ => out.push_str(&escape(c.encode_utf8(&mut [0; 4]))),
        }
    }
    write_spaces(out, &mut spaces, first);
}

/// Writes `count` spaces: only one that follows a character stays as a space
/// in `OpenDocument`, the others need an element.
fn write_spaces(out: &mut String, count: &mut usize, first: bool) {
    let elements = if first {
        *count
    } else {
        if *count > 0 {
            out.push(' ');
        }
        count.saturating_sub(1)
    };
    match elements {
        0 => {}
        1 => out.push_str("<text:s/>"),
        n => {
            let _ = write!(out, "<text:s text:c=\"{n}\"/>");
        }
    }
    *count = 0;
}

/// `text` for XML, without the characters XML 1.0 cannot hold.
pub(crate) fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\t' | '\n' | '\r' => out.push(c),
            c if u32::from(c) < 0x20 || matches!(c, '\u{FFFE}' | '\u{FFFF}') => {}
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn source(text: &str) -> String {
        let mut out = String::new();
        write_source(&mut out, text);
        out
    }

    #[test]
    fn every_space_of_code_stays() {
        assert_eq!(source("a b"), "a b");
        assert_eq!(source("a  b"), "a <text:s/>b");
        assert_eq!(source("a    b"), "a <text:s text:c=\"3\"/>b");
        assert_eq!(source(" a"), "<text:s/>a");
        assert_eq!(
            source("a\n  b"),
            "a<text:line-break/><text:s text:c=\"2\"/>b"
        );
        assert_eq!(source("a\tb "), "a<text:tab/>b ");
        assert_eq!(source("<&>"), "&lt;&amp;&gt;");
    }

    #[test]
    fn what_xml_cannot_hold_is_left_out() {
        assert_eq!(escape("a\u{1}b\u{FFFE}\"c\""), "ab&quot;c&quot;");
        assert_eq!(escape("Grüße ☐ 🙂"), "Grüße ☐ 🙂");
    }

    #[test]
    fn a_target_in_the_document_is_a_bookmark_name() {
        assert_eq!(href("#note-gr%C3%BC%C3%9Fe"), "#note-grüße");
        assert_eq!(href("../Gr%C3%BC.odt"), "../Gr%C3%BC.odt");
        assert_eq!(
            href("https://example.org/a%20b"),
            "https://example.org/a%20b"
        );
    }

    #[test]
    fn an_svg_tells_its_size_by_its_length_or_its_view_box() {
        let size = |svg: &str| {
            size_of_svg(svg)
                .map(|(w, h)| ((w * 100.0).round() / 100.0, (h * 100.0).round() / 100.0))
        };
        assert_eq!(size("<svg width=\"96\" height=\"48\">"), Some((2.54, 1.27)));
        assert_eq!(
            size("<?xml?><svg height='2in' width='4cm'>"),
            Some((4.0, 5.08))
        );
        assert_eq!(
            size("<svg width=\"100%\" viewBox=\"0 0 96 192\">"),
            Some((2.54, 5.08))
        );
        assert_eq!(
            size("<svg viewBox=\"0,0,96,48\" stroke-width=\"9\">"),
            Some((2.54, 1.27))
        );
        assert_eq!(size("<svg stroke-width=\"9\" height=\"9\">"), None);
        assert_eq!(size("<svg viewBox=\"0 0 0 0\">"), None);
        assert_eq!(size("<svg width=\"1em\" height=\"1em\">"), None);
        assert_eq!(size("no picture"), None);
    }

    #[test]
    fn pictures_keep_a_usual_extension() {
        assert_eq!(extension("image/jpeg"), "jpg");
        assert_eq!(extension("image/svg+xml"), "svg");
        assert_eq!(extension("image/png"), "png");
    }
}
