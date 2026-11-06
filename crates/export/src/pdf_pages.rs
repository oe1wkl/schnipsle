// SPDX-License-Identifier: Apache-2.0
//! Page numbers in a finished PDF (ADR-0029). The print path adds no header or
//! footer, and `WebKit` has no running footers in CSS, so the numbers are drawn
//! on the printed pages: at the foot of each page, and at the right margin of
//! each entry of a table of contents, which by then links to the page its note
//! starts on. They are added as an incremental update, so every byte the print
//! path wrote stays as it is.

use std::collections::{BTreeMap, HashMap};
use std::io;

use lopdf::content::{Content, Operation};
use lopdf::{Dictionary, Document, IncrementalDocument, Object, ObjectId, Stream, dictionary};

/// The font's name in the resources of a page; a standard font, so nothing is embedded.
const FONT: &str = "SchnipslePageNumber";

/// The width of a digit of Helvetica, in thousandths of its size.
const DIGIT_WIDTH: f32 = 0.556;

/// The size of a page number at the foot of a page, in points.
const FOOTER_SIZE: f32 = 9.0;

/// What to number.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Numbering {
    /// The margin on all sides of a page, in points.
    pub margin: f32,
    /// A number at the foot of every page.
    pub footer: bool,
    /// The document starts with a table of contents whose entries get the page
    /// they link to.
    pub contents: bool,
}

/// A number to draw: its baseline starts at `x`, `y`.
#[derive(Debug, Clone, PartialEq)]
struct Label {
    x: f32,
    y: f32,
    size: f32,
    text: String,
    /// Where a row of dots towards it starts, as in the contents of ODT and DOCX.
    dots_from: Option<f32>,
}

/// `pdf` with its pages numbered.
///
/// # Errors
/// If the file cannot be read as a PDF; the caller keeps it without numbers.
pub(crate) fn number_pages(pdf: &[u8], numbering: Numbering) -> io::Result<Vec<u8>> {
    let mut document: IncrementalDocument = pdf.try_into().map_err(io::Error::other)?;
    let texts = texts(document.get_prev_documents(), numbering);
    if texts.is_empty() {
        return Ok(pdf.to_vec());
    }
    let font = document.new_document.add_object(dictionary! {
        "Type" => "Font",
        "Subtype" => "Type1",
        "BaseFont" => "Helvetica",
        "Encoding" => "WinAnsiEncoding",
    });
    // The page's own drawing in `q … Q`, so it cannot leave a state behind for ours.
    let save = document
        .new_document
        .add_object(Stream::new(Dictionary::new(), b"q\n".to_vec()));
    let restore = document
        .new_document
        .add_object(Stream::new(Dictionary::new(), b"\nQ\n".to_vec()));
    for (page, texts) in texts {
        let drawing = drawing(&texts)?;
        let drawing = document
            .new_document
            .add_object(Stream::new(Dictionary::new(), drawing));
        add_font(&mut document, page, font)?;
        document
            .opt_clone_object_to_new_document(page)
            .map_err(io::Error::other)?;
        let page = document
            .new_document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(io::Error::other)?;
        let mut contents = vec![Object::Reference(save)];
        match page.get(b"Contents") {
            Ok(Object::Array(parts)) => contents.extend(parts.iter().cloned()),
            Ok(part) => contents.push(part.clone()),
            Err(_) => {}
        }
        contents.extend([Object::Reference(restore), Object::Reference(drawing)]);
        page.set("Contents", contents);
    }
    let mut out = Vec::with_capacity(pdf.len() + 4096);
    document.save_to(&mut out).map_err(io::Error::other)?;
    Ok(out)
}

/// The numbers to draw, by page.
fn texts(document: &Document, numbering: Numbering) -> BTreeMap<ObjectId, Vec<Label>> {
    let pages = document.get_pages();
    let mut texts: BTreeMap<ObjectId, Vec<Label>> = BTreeMap::new();
    if numbering.footer {
        for (&number, &page) in &pages {
            let (width, _) = page_size(document, page);
            let text = number.to_string();
            texts.entry(page).or_default().push(Label {
                x: (width - text_width(&text, FOOTER_SIZE)) / 2.0,
                y: numbering.margin * 0.45,
                size: FOOTER_SIZE,
                text,
                dots_from: None,
            });
        }
    }
    if numbering.contents {
        let number_of: HashMap<ObjectId, u32> = pages.iter().map(|(&n, &id)| (id, n)).collect();
        for (page, rect, target) in contents_entries(document, &pages, &number_of) {
            let (width, _) = page_size(document, page);
            let height = rect[3] - rect[1];
            let size = (height / 1.2).clamp(8.0, 14.0);
            let text = target.to_string();
            texts.entry(page).or_default().push(Label {
                x: width - numbering.margin - text_width(&text, size),
                y: rect[1] + height * 0.22,
                size,
                text,
                dots_from: Some(rect[2]),
            });
        }
    }
    texts
}

/// The entries of the table of contents at the start of the document: the page
/// and rectangle of the end of each one's last line, and the page its link goes
/// to. The table ends where the first note starts; every note starts on a page
/// of its own, so the page an entry links to tells it apart from the others.
///
/// `WebKit` gives a link over two lines one rectangle around both, so an entry
/// ends with a second link of one space ([`crate::html`]): of an entry's
/// rectangles on its last page, the one whose top is lowest and, of those, the
/// one furthest right is where its last line ends.
fn contents_entries(
    document: &Document,
    pages: &BTreeMap<u32, ObjectId>,
    number_of: &HashMap<ObjectId, u32>,
) -> Vec<(ObjectId, [f32; 4], u32)> {
    let Some(&first) = pages.get(&1) else {
        return Vec::new();
    };
    let Some(first_note) = links(document, first, number_of)
        .iter()
        .map(|(_, target)| *target)
        .min()
    else {
        return Vec::new();
    };
    // By the page it links to.
    let mut entries: BTreeMap<u32, (u32, ObjectId, [f32; 4])> = BTreeMap::new();
    for (&number, &page) in pages.range(..first_note) {
        for (rect, target) in links(document, page, number_of) {
            let later = entries.get(&target).is_none_or(|(at, _, last)| {
                let lower = rect[3] < last[3] - 1.0;
                let same_line = (rect[3] - last[3]).abs() <= 1.0;
                number > *at || (number == *at && (lower || (same_line && rect[2] > last[2])))
            });
            if later {
                entries.insert(target, (number, page, rect));
            }
        }
    }
    entries
        .into_iter()
        .map(|(target, (_, page, rect))| (page, rect, target))
        .collect()
}

/// The links on `page` to a page of this document: their rectangle as
/// `[left, bottom, right, top]`, and the number of the page they go to.
fn links(
    document: &Document,
    page: ObjectId,
    number_of: &HashMap<ObjectId, u32>,
) -> Vec<([f32; 4], u32)> {
    let Ok(annotations) = document.get_page_annotations(page) else {
        return Vec::new();
    };
    annotations
        .into_iter()
        .filter(|annotation| {
            annotation
                .get(b"Subtype")
                .and_then(Object::as_name)
                .is_ok_and(|name| name == b"Link")
        })
        .filter_map(|annotation| {
            let rect = rect(document, annotation.get(b"Rect").ok()?)?;
            let destination = if let Ok(destination) = annotation.get(b"Dest") {
                destination
            } else {
                let action = resolve(document, annotation.get(b"A").ok()?)
                    .as_dict()
                    .ok()?;
                action.get(b"D").ok()?
            };
            let destination = resolve(document, destination).as_array().ok()?;
            let target = destination.first()?.as_reference().ok()?;
            Some((rect, *number_of.get(&target)?))
        })
        .collect()
}

fn resolve<'d>(document: &'d Document, object: &'d Object) -> &'d Object {
    document
        .dereference(object)
        .map_or(object, |(_, object)| object)
}

fn rect(document: &Document, object: &Object) -> Option<[f32; 4]> {
    let values = resolve(document, object).as_array().ok()?;
    let numbers: Vec<f32> = values.iter().filter_map(|v| v.as_float().ok()).collect();
    let [x1, y1, x2, y2] = numbers.as_slice() else {
        return None;
    };
    Some([x1.min(*x2), y1.min(*y2), x1.max(*x2), y1.max(*y2)])
}

/// Width and height of `page`, which may inherit its box from the page tree.
fn page_size(document: &Document, page: ObjectId) -> (f32, f32) {
    let mut id = Some(page);
    while let Some(current) = id {
        let Ok(dictionary) = document.get_dictionary(current) else {
            break;
        };
        if let Some(rect) = dictionary
            .get(b"MediaBox")
            .ok()
            .and_then(|b| rect(document, b))
        {
            return (rect[2] - rect[0], rect[3] - rect[1]);
        }
        id = dictionary
            .get(b"Parent")
            .and_then(Object::as_reference)
            .ok();
    }
    // A4, which a page without a box should not be.
    (595.28, 841.89)
}

fn text_width(text: &str, size: f32) -> f32 {
    let digits = u16::try_from(text.chars().count()).unwrap_or(u16::MAX);
    f32::from(digits) * DIGIT_WIDTH * size
}

/// The drawing of `texts` in grey.
fn drawing(texts: &[Label]) -> io::Result<Vec<u8>> {
    let mut operations = vec![
        Operation::new("q", vec![]),
        Operation::new("g", vec![0.43.into()]),
    ];
    for text in texts {
        if let Some(from) = text.dots_from {
            let (start, end) = (from + text.size * 0.5, text.x - text.size * 0.5);
            if end > start {
                // Round dots: lines of no length with round ends.
                operations.extend([
                    Operation::new("q", vec![]),
                    Operation::new("G", vec![0.6.into()]),
                    Operation::new("w", vec![rounded(text.size * 0.12)]),
                    Operation::new("J", vec![1.into()]),
                    Operation::new(
                        "d",
                        vec![vec![0.into(), rounded(text.size * 0.3)].into(), 0.into()],
                    ),
                    Operation::new("m", vec![rounded(start), rounded(text.y)]),
                    Operation::new("l", vec![rounded(end), rounded(text.y)]),
                    Operation::new("S", vec![]),
                    Operation::new("Q", vec![]),
                ]);
            }
        }
        operations.extend([
            Operation::new("BT", vec![]),
            Operation::new("Tf", vec![Object::Name(FONT.into()), rounded(text.size)]),
            Operation::new("Td", vec![rounded(text.x), rounded(text.y)]),
            Operation::new("Tj", vec![Object::string_literal(text.text.as_str())]),
            Operation::new("ET", vec![]),
        ]);
    }
    operations.push(Operation::new("Q", vec![]));
    Content { operations }.encode().map_err(io::Error::other)
}

/// A length to a hundredth of a point, which is finer than anyone sees.
fn rounded(value: f32) -> Object {
    Object::Real((value * 100.0).round() / 100.0)
}

/// Makes the font known to `page`, in a copy of its resources.
fn add_font(document: &mut IncrementalDocument, page: ObjectId, font: ObjectId) -> io::Result<()> {
    let resources = document
        .get_or_create_resources(page)
        .map_err(io::Error::other)?;
    let resources = resources.as_dict_mut().map_err(io::Error::other)?;
    let fonts_id = match resources.get(b"Font") {
        Ok(Object::Reference(id)) => Some(*id),
        Ok(_) => None,
        Err(_) => {
            resources.set("Font", Dictionary::new());
            None
        }
    };
    let fonts = match fonts_id {
        Some(id) => {
            document
                .opt_clone_object_to_new_document(id)
                .map_err(io::Error::other)?;
            document.new_document.get_object_mut(id)
        }
        None => document
            .get_or_create_resources(page)
            .and_then(Object::as_dict_mut)
            .and_then(|resources| resources.get_mut(b"Font")),
    }
    .and_then(Object::as_dict_mut)
    .map_err(io::Error::other)?;
    fonts.set(FONT, Object::Reference(font));
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A PDF as the print path writes one: a contents page whose two entries
    /// link to the pages of two notes. The second runs over two lines, which
    /// `WebKit` gives one rectangle, and ends with the link of one space.
    fn printed() -> Vec<u8> {
        let mut document = Document::with_version("1.3");
        let pages_id = document.new_object_id();
        let resources = document.add_object(dictionary! { "Font" => dictionary! {} });
        let mut kids = Vec::new();
        let mut page_ids = Vec::new();
        for _ in 0..3 {
            let content = document.add_object(Stream::new(Dictionary::new(), b"0 0 m".to_vec()));
            let page = document.add_object(dictionary! {
                "Type" => "Page",
                "Parent" => pages_id,
                "Contents" => content,
                "Resources" => resources,
            });
            kids.push(Object::Reference(page));
            page_ids.push(page);
        }
        let link = |document: &mut Document, target: ObjectId, rect: [f32; 4]| {
            let dest = document.add_object(vec![
                Object::Reference(target),
                "XYZ".into(),
                Object::Real(56.0),
                Object::Real(780.0),
                Object::Integer(0),
            ]);
            document.add_object(dictionary! {
                "Type" => "Annot",
                "Subtype" => "Link",
                "Rect" => rect.iter().map(|v| Object::Real(*v)).collect::<Vec<_>>(),
                "Dest" => dest,
            })
        };
        let first = link(&mut document, page_ids[1], [60.0, 700.0, 200.0, 714.4]);
        let second_a = link(&mut document, page_ids[2], [60.0, 671.2, 540.0, 700.0]);
        let second_b = link(&mut document, page_ids[2], [120.0, 671.2, 124.0, 685.6]);
        let annots = document.add_object(vec![
            Object::Reference(second_b),
            Object::Reference(first),
            Object::Reference(second_a),
        ]);
        document
            .get_object_mut(page_ids[0])
            .and_then(Object::as_dict_mut)
            .unwrap()
            .set("Annots", annots);
        document.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => 3,
                "MediaBox" => vec![0.into(), 0.into(), Object::Real(595.0), Object::Real(842.0)],
            }),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    const BOTH: Numbering = Numbering {
        margin: 56.7,
        footer: true,
        contents: true,
    };

    #[test]
    fn every_page_and_every_entry_gets_its_number() {
        let pdf = printed();
        let document = Document::load_mem(&pdf).unwrap();
        let texts = texts(&document, BOTH);
        let pages = document.get_pages();
        let first: Vec<&str> = texts[&pages[&1]].iter().map(|t| t.text.as_str()).collect();
        assert_eq!(first, ["1", "2", "3"]);
        assert_eq!(texts[&pages[&3]].len(), 1);
        // The second entry's number stands on its last line, at the right margin.
        let entry = &texts[&pages[&1]][2];
        assert!((entry.y - (671.2 + 14.4 * 0.22)).abs() < 0.01, "{entry:?}");
        assert!((entry.x + text_width("3", entry.size) - (595.0 - 56.7)).abs() < 0.01);
        assert_eq!(entry.dots_from, Some(124.0));
        // The footer is centred.
        let footer = &texts[&pages[&2]][0];
        assert!((footer.x - (595.0 - text_width("2", 9.0)) / 2.0).abs() < 0.01);
    }

    #[test]
    fn the_numbers_are_added_after_what_was_printed() {
        let pdf = printed();
        let numbered = number_pages(&pdf, BOTH).unwrap();
        assert!(numbered.starts_with(&pdf));
        assert!(numbered.len() > pdf.len());
        let document = Document::load_mem(&numbered).unwrap();
        let page = document.get_pages()[&1];
        let contents = document.get_page_content(page);
        let text = String::from_utf8_lossy(&contents);
        let order: Vec<&str> = text.split_whitespace().take(3).collect();
        assert_eq!(order, ["q", "0", "0"], "{text}");
        assert!(text.contains("/SchnipslePageNumber 9"), "{text}");
        assert!(text.contains("(1) Tj"), "{text}");
        let fonts = document
            .get_dictionary(page)
            .and_then(|p| p.get(b"Resources"))
            .and_then(|r| document.dereference(r))
            .and_then(|(_, r)| r.as_dict())
            .and_then(|r| r.get(b"Font"))
            .and_then(Object::as_dict)
            .unwrap();
        assert!(fonts.has(FONT.as_bytes()));
    }

    #[test]
    fn only_the_footer_or_nothing() {
        let pdf = printed();
        let document = Document::load_mem(&pdf).unwrap();
        let footer = Numbering {
            contents: false,
            ..BOTH
        };
        assert!(texts(&document, footer).values().all(|t| t.len() == 1));
        let nothing = Numbering {
            footer: false,
            contents: false,
            ..BOTH
        };
        assert_eq!(number_pages(&pdf, nothing).unwrap(), pdf);
    }

    #[test]
    fn what_is_no_pdf_is_an_error() {
        assert!(number_pages(b"no pdf", BOTH).is_err());
    }
}
