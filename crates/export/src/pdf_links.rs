// SPDX-License-Identifier: Apache-2.0
//! Links put back into a PDF whose print path lost them (ADR-0037). The printer
//! has found where the text of each [`Anchor`] is; a link gets an annotation
//! over each line of its text, to a web address or to the place of its target.
//! As with the page numbers ([`crate::pdf_pages`]), they are added as an
//! incremental update, so every byte the print path wrote stays as it is.

use std::collections::{BTreeMap, HashMap};
use std::io;

use lopdf::{IncrementalDocument, Object, ObjectId, StringFormat, dictionary};
use percent_encoding::percent_decode_str;

use crate::anchors::{Anchor, AnchorKind, Place};

/// How far above the top of its first line a link to a target shows the page,
/// in points, so the line is not at the very edge.
const ROOM_ABOVE: f64 = 12.0;

/// `pdf` with the links of `anchors` at their `places`, and how many links
/// could not be put back: their text or their target was not found.
///
/// # Errors
/// If the file cannot be read as a PDF, or `places` does not fit `anchors`.
pub(crate) fn add_links(
    pdf: &[u8],
    anchors: &[Anchor],
    places: &[Option<Place>],
) -> io::Result<(Vec<u8>, usize)> {
    if places.len() != anchors.len() {
        return Err(io::Error::other(format!(
            "{} places for {} anchors",
            places.len(),
            anchors.len()
        )));
    }
    let mut document: IncrementalDocument = pdf.try_into().map_err(io::Error::other)?;
    let pages: Vec<ObjectId> = document
        .get_prev_documents()
        .get_pages()
        .into_values()
        .collect();
    let targets = targets(anchors, places);
    let mut by_page: BTreeMap<ObjectId, Vec<Object>> = BTreeMap::new();
    let mut missed = 0;
    for (anchor, place) in anchors.iter().zip(places) {
        let AnchorKind::Link { href, .. } = &anchor.kind else {
            continue;
        };
        let Some(place) = place else {
            missed += 1;
            continue;
        };
        let to = if let Some(id) = href.strip_prefix('#') {
            let id = percent_decode_str(id).decode_utf8_lossy();
            let Some(&(page, top)) = targets.get(id.as_ref()) else {
                missed += 1;
                continue;
            };
            let Some(&page) = pages.get(page) else {
                missed += 1;
                continue;
            };
            let destination: Vec<Object> = vec![
                Object::Reference(page),
                Object::Name(b"XYZ".to_vec()),
                Object::Null,
                rounded(top + ROOM_ABOVE),
                Object::Null,
            ];
            ("Dest", Object::Array(destination))
        } else {
            let action = dictionary! {
                "S" => "URI",
                "URI" => Object::String(uri(href), StringFormat::Literal),
            };
            ("A", Object::Dictionary(action))
        };
        for line in &place.lines {
            let Some(&page) = pages.get(line.page) else {
                continue;
            };
            let mut annotation = dictionary! {
                "Type" => "Annot",
                "Subtype" => "Link",
                "Rect" => line.rect.iter().map(|&v| rounded(v)).collect::<Vec<_>>(),
                "Border" => vec![0.into(), 0.into(), 0.into()],
            };
            annotation.set(to.0, to.1.clone());
            let id = document.new_document.add_object(annotation);
            by_page.entry(page).or_default().push(Object::Reference(id));
        }
    }
    if by_page.is_empty() {
        return Ok((pdf.to_vec(), missed));
    }
    for (page, new) in by_page {
        let mut annotations = existing_annotations(&document, page);
        annotations.extend(new);
        document
            .opt_clone_object_to_new_document(page)
            .map_err(io::Error::other)?;
        document
            .new_document
            .get_object_mut(page)
            .and_then(Object::as_dict_mut)
            .map_err(io::Error::other)?
            .set("Annots", annotations);
    }
    let mut out = Vec::with_capacity(pdf.len() + 4096);
    document.save_to(&mut out).map_err(io::Error::other)?;
    Ok((out, missed))
}

/// Where each target is: its page and the top of its first line. A footnote
/// reference is a link and a target.
fn targets<'a>(anchors: &'a [Anchor], places: &[Option<Place>]) -> HashMap<&'a str, (usize, f64)> {
    let mut targets = HashMap::new();
    for (anchor, place) in anchors.iter().zip(places) {
        let id = match &anchor.kind {
            AnchorKind::Target { id } => Some(id),
            AnchorKind::Link { id, .. } => id.as_ref(),
        };
        if let (Some(id), Some(line)) = (id, place.as_ref().and_then(|p| p.lines.first())) {
            targets
                .entry(id.as_str())
                .or_insert((line.page, line.rect[3]));
        }
    }
    targets
}

/// The annotations `page` has already.
fn existing_annotations(document: &IncrementalDocument, page: ObjectId) -> Vec<Object> {
    let previous = document.get_prev_documents();
    let Ok(page) = previous.get_dictionary(page) else {
        return Vec::new();
    };
    let Ok(annotations) = page.get(b"Annots") else {
        return Vec::new();
    };
    let annotations = previous
        .dereference(annotations)
        .map_or(annotations, |(_, object)| object);
    annotations.as_array().cloned().unwrap_or_default()
}

/// `href` in the seven-bit characters a URI in a PDF has: anything else
/// percent-encoded, as a browser would send it.
fn uri(href: &str) -> Vec<u8> {
    let mut out = Vec::with_capacity(href.len());
    for byte in href.bytes() {
        if (0x21..0x7f).contains(&byte) {
            out.push(byte);
        } else {
            out.extend(format!("%{byte:02X}").bytes());
        }
    }
    out
}

/// A length to a hundredth of a point.
#[allow(clippy::cast_possible_truncation)] // page lengths
fn rounded(value: f64) -> Object {
    Object::Real(((value * 100.0).round() / 100.0) as f32)
}

/// For the tests: a dictionary of a link annotation, by page.
#[cfg(test)]
fn links_of(pdf: &[u8]) -> Vec<(u32, lopdf::Dictionary)> {
    let document = lopdf::Document::load_mem(pdf).unwrap();
    let mut out = Vec::new();
    for (number, page) in document.get_pages() {
        for annotation in document.get_page_annotations(page).unwrap_or_default() {
            out.push((number, annotation.clone()));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use lopdf::content::Content;
    use lopdf::{Dictionary, Document, Stream};

    use super::*;
    use crate::anchors::Line;

    /// A PDF of `count` empty A4 pages, as a print path without links makes it.
    fn pages(count: usize) -> Vec<u8> {
        let mut document = Document::with_version("1.4");
        let tree = document.new_object_id();
        let content = document.add_object(Stream::new(
            Dictionary::new(),
            Content { operations: vec![] }.encode().unwrap(),
        ));
        let kids: Vec<Object> = (0..count)
            .map(|_| {
                document
                    .add_object(dictionary! {
                        "Type" => "Page",
                        "Parent" => tree,
                        "MediaBox" => vec![0.into(), 0.into(), 595.into(), 842.into()],
                        "Contents" => content,
                    })
                    .into()
            })
            .collect();
        document.objects.insert(
            tree,
            Object::Dictionary(dictionary! {
                "Type" => "Pages",
                "Kids" => kids,
                "Count" => i64::try_from(count).unwrap(),
            }),
        );
        let catalog = document.add_object(dictionary! { "Type" => "Catalog", "Pages" => tree });
        document.trailer.set("Root", catalog);
        let mut out = Vec::new();
        document.save_to(&mut out).unwrap();
        out
    }

    fn link(href: &str, id: Option<&str>) -> Anchor {
        Anchor {
            before: String::new(),
            text: "x".to_owned(),
            kind: AnchorKind::Link {
                href: href.to_owned(),
                id: id.map(str::to_owned),
            },
        }
    }

    fn target(id: &str) -> Anchor {
        Anchor {
            before: String::new(),
            text: "x".to_owned(),
            kind: AnchorKind::Target { id: id.to_owned() },
        }
    }

    #[allow(clippy::unnecessary_wraps)] // a place as the printer gives it
    fn at(lines: &[(usize, f64)]) -> Option<Place> {
        Some(Place {
            lines: lines
                .iter()
                .map(|&(page, top)| Line {
                    page,
                    rect: [60.0, top - 12.0, 200.0, top],
                })
                .collect(),
        })
    }

    #[test]
    fn web_links_and_links_to_targets_one_per_line() {
        let pdf = pages(3);
        let anchors = [
            link("#note-b%C3%A4r", None),
            link("https://example.org/a b", None),
            target("note-bär"),
        ];
        let places = [
            at(&[(0, 700.0), (0, 686.0)]),
            at(&[(1, 500.0)]),
            at(&[(2, 780.0)]),
        ];
        let (linked, missed) = add_links(&pdf, &anchors, &places).unwrap();
        assert_eq!(missed, 0);
        assert!(linked.starts_with(&pdf), "an incremental update");
        let links = links_of(&linked);
        assert_eq!(links.len(), 3);
        assert_eq!(links[0].0, 1);
        assert_eq!(links[1].0, 1);
        let destination = links[0].1.get(b"Dest").unwrap().as_array().unwrap();
        let document = Document::load_mem(&linked).unwrap();
        let third = document.get_pages()[&3];
        assert_eq!(destination[0].as_reference().unwrap(), third);
        assert!((destination[3].as_float().unwrap() - 792.0).abs() < 0.01);
        let action = links[2].1.get(b"A").unwrap().as_dict().unwrap();
        assert_eq!(
            action.get(b"URI").unwrap().as_str().unwrap(),
            b"https://example.org/a%20b"
        );
        assert_eq!(links[2].0, 2);
    }

    #[test]
    fn a_footnote_reference_is_a_target_too() {
        let pdf = pages(2);
        let anchors = [
            link("#fn-1", Some("fnref-1")),
            target("fn-1"),
            link("#fnref-1", None),
        ];
        let places = [at(&[(0, 400.0)]), at(&[(1, 300.0)]), at(&[(1, 280.0)])];
        let (linked, missed) = add_links(&pdf, &anchors, &places).unwrap();
        assert_eq!(missed, 0);
        let links = links_of(&linked);
        let back = links[1].1.get(b"Dest").unwrap().as_array().unwrap();
        assert!((back[3].as_float().unwrap() - 412.0).abs() < 0.01);
    }

    #[test]
    fn what_is_not_found_is_counted_and_left_out() {
        let pdf = pages(1);
        let anchors = [
            link("https://example.org", None),
            link("#nowhere", None),
            target("unused"),
        ];
        let places = [None, at(&[(0, 500.0)]), None];
        let (linked, missed) = add_links(&pdf, &anchors, &places).unwrap();
        assert_eq!(missed, 2);
        assert_eq!(linked, pdf, "nothing to add");
    }

    #[test]
    fn places_must_fit_the_anchors() {
        assert!(add_links(&pages(1), &[target("a")], &[]).is_err());
    }

    #[test]
    fn links_already_there_stay() {
        let pdf = pages(1);
        let anchors = [link("https://example.org", None)];
        let (once, _) = add_links(&pdf, &anchors, &[at(&[(0, 500.0)])]).unwrap();
        let (twice, _) = add_links(&once, &anchors, &[at(&[(0, 300.0)])]).unwrap();
        assert_eq!(links_of(&twice).len(), 2);
    }
}
