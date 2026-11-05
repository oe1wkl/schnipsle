// SPDX-License-Identifier: Apache-2.0
//! Completes the DOCX Pandoc writes (ADR-0025). Pandoc takes the document's title
//! only together with a title paragraph on the first page, which a note exported
//! as written has not (ADR-0021 rule 2), and it has no way to say the paper
//! without a reference document. So both are set in the finished package; all
//! other parts are copied as they are.

use std::fmt::Write as _;
use std::io::{self, Cursor, Read as _, Write as _};

use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipArchive, ZipWriter};

use crate::Paper;
use crate::odt_content::escape;

const CORE: &str = "docProps/core.xml";
const DOCUMENT: &str = "word/document.xml";

/// Width and height in twentieths of a point.
const fn page_size(paper: Paper) -> (u32, u32) {
    match paper {
        Paper::A4 => (11906, 16838),
        Paper::Letter => (12240, 15840),
    }
}

/// 20 mm, as in PDF and ODT.
const MARGIN: u32 = 1134;

/// The package `docx` with its title, its creation date if known, and its paper.
pub(crate) fn finish(
    docx: &[u8],
    title: &str,
    created: Option<&str>,
    paper: Paper,
) -> io::Result<Vec<u8>> {
    let mut archive = ZipArchive::new(Cursor::new(docx)).map_err(io::Error::other)?;
    let mut writer = ZipWriter::new(Cursor::new(Vec::with_capacity(docx.len())));
    for index in 0..archive.len() {
        let name = archive
            .by_index_raw(index)
            .map_err(io::Error::other)?
            .name()
            .map_err(io::Error::other)?
            .into_owned();
        if name != CORE && name != DOCUMENT {
            let part = archive.by_index_raw(index).map_err(io::Error::other)?;
            writer.raw_copy_file(part).map_err(io::Error::other)?;
            continue;
        }
        let mut part = archive.by_index(index).map_err(io::Error::other)?;
        let mut xml = String::new();
        part.read_to_string(&mut xml)?;
        let xml = if name == CORE {
            core(&xml, title, created)
        } else {
            document(&xml, paper)
        };
        let mut options =
            SimpleFileOptions::default().compression_method(CompressionMethod::Deflated);
        if let Some(time) = part.last_modified() {
            options = options.last_modified_time(time);
        }
        writer.start_file(name, options).map_err(io::Error::other)?;
        writer.write_all(xml.as_bytes())?;
    }
    let finished = writer.finish().map_err(io::Error::other)?;
    Ok(finished.into_inner())
}

fn core(xml: &str, title: &str, created: Option<&str>) -> String {
    let xml = set_element(xml, "dc:title", &escape(title));
    match created {
        Some(created) => set_element(&xml, "dcterms:created", &escape(created)),
        None => xml,
    }
}

/// Sets the content of the first element `name`, which may be empty as `<name/>`.
/// A package without it stays as it is.
fn set_element(xml: &str, name: &str, content: &str) -> String {
    let open = format!("<{name}");
    let Some(start) = xml.find(&open) else {
        return xml.to_owned();
    };
    let Some(tag_end) = xml[start..].find('>').map(|i| start + i) else {
        return xml.to_owned();
    };
    let tag = &xml[start..tag_end];
    let close = format!("</{name}>");
    if let Some(attributes) = tag.strip_suffix('/') {
        let attributes = attributes.trim_end();
        return format!(
            "{}{attributes}>{content}{close}{}",
            &xml[..start],
            &xml[tag_end + 1..]
        );
    }
    let Some(end) = xml[tag_end..].find(&close).map(|i| tag_end + i) else {
        return xml.to_owned();
    };
    format!("{}{content}{}", &xml[..=tag_end], &xml[end..])
}

/// Adds the page's size and margins to the last section, which holds the document.
fn document(xml: &str, paper: Paper) -> String {
    let (width, height) = page_size(paper);
    let mut page = String::new();
    let half = MARGIN / 2;
    let _ = write!(
        page,
        "<w:pgSz w:w=\"{width}\" w:h=\"{height}\"/><w:pgMar w:top=\"{MARGIN}\" w:right=\"{MARGIN}\" w:bottom=\"{MARGIN}\" w:left=\"{MARGIN}\" w:header=\"{half}\" w:footer=\"{half}\" w:gutter=\"0\"/>"
    );
    let Some(start) = xml.rfind("<w:sectPr") else {
        return xml.to_owned();
    };
    let Some(tag_end) = xml[start..].find('>').map(|i| start + i) else {
        return xml.to_owned();
    };
    if xml[start..tag_end].ends_with('/') {
        let tag = xml[start..tag_end].trim_end_matches('/').trim_end();
        return format!(
            "{}{tag}>{page}</w:sectPr>{}",
            &xml[..start],
            &xml[tag_end + 1..]
        );
    }
    let Some(end) = xml[tag_end..].find("</w:sectPr>").map(|i| tag_end + i) else {
        return xml.to_owned();
    };
    format!("{}{page}{}", &xml[..end], &xml[end..])
}

#[cfg(test)]
mod tests {
    use super::*;

    const CORE_XML: &str = "<cp:coreProperties><dc:title></dc:title><dcterms:created xsi:type=\"dcterms:W3CDTF\">2020-01-01T00:00:00Z</dcterms:created><dcterms:modified xsi:type=\"dcterms:W3CDTF\">2021-01-01T00:00:00Z</dcterms:modified></cp:coreProperties>";

    #[test]
    fn the_title_and_date_go_into_the_properties() {
        let xml = core(CORE_XML, "Fish & <chips>", Some("2026-11-05T10:00:00Z"));
        assert!(
            xml.contains("<dc:title>Fish &amp; &lt;chips&gt;</dc:title>"),
            "{xml}"
        );
        assert!(
            xml.contains("W3CDTF\">2026-11-05T10:00:00Z</dcterms:created>"),
            "{xml}"
        );
        assert!(xml.contains("2021-01-01T00:00:00Z</dcterms:modified>"));
        // A document of several notes has no date of its own.
        assert!(core(CORE_XML, "T", None).contains("2020-01-01T00:00:00Z</dcterms:created>"));
    }

    #[test]
    fn an_empty_element_is_opened() {
        assert_eq!(
            set_element("<a><dc:title /></a>", "dc:title", "T"),
            "<a><dc:title>T</dc:title></a>"
        );
        assert_eq!(set_element("<a></a>", "dc:title", "T"), "<a></a>");
    }

    #[test]
    fn the_paper_goes_into_the_last_section() {
        let xml = document(
            "<w:body><w:p/><w:sectPr><w:footnotePr/></w:sectPr></w:body>",
            Paper::A4,
        );
        assert!(
            xml.contains("<w:footnotePr/><w:pgSz w:w=\"11906\" w:h=\"16838\"/><w:pgMar"),
            "{xml}"
        );
        let xml = document("<w:body><w:sectPr /></w:body>", Paper::Letter);
        assert!(
            xml.starts_with("<w:body><w:sectPr><w:pgSz w:w=\"12240\" w:h=\"15840\"/>"),
            "{xml}"
        );
        assert!(xml.ends_with("</w:sectPr></w:body>"), "{xml}");
        assert_eq!(document("<w:body/>", Paper::A4), "<w:body/>");
    }

    #[test]
    fn other_parts_stay_and_the_result_is_a_package() {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default();
        for (name, text) in [
            ("[Content_Types].xml", "<Types/>"),
            (DOCUMENT, "<w:body><w:sectPr /></w:body>"),
            (CORE, CORE_XML),
            ("word/media/a.png", "bytes"),
        ] {
            writer.start_file(name, options).unwrap();
            writer.write_all(text.as_bytes()).unwrap();
        }
        let docx = writer.finish().unwrap().into_inner();

        let done = finish(&docx, "Title", None, Paper::A4).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(done)).unwrap();
        let mut read = |name: &str| {
            let mut text = String::new();
            archive
                .by_name(name)
                .unwrap()
                .read_to_string(&mut text)
                .unwrap();
            text
        };
        assert_eq!(read("[Content_Types].xml"), "<Types/>");
        assert_eq!(read("word/media/a.png"), "bytes");
        assert!(read(CORE).contains("<dc:title>Title</dc:title>"));
        assert!(read(DOCUMENT).contains("w:pgSz"));
    }

    #[test]
    fn something_else_is_an_error() {
        assert!(finish(b"not a package", "T", None, Paper::A4).is_err());
    }
}
