// SPDX-License-Identifier: Apache-2.0
//! Pictures few programs show (ADR-0027), in the formats that hold them and in a
//! folder of copies, and pictures turned by their orientation, which ODT gets
//! upright (ADR-0028). The conversion itself is tested in `schnipsle-apple`.

use std::fs;

use schnipsle_core::vault::Vault;
use schnipsle_export::{Exporter as _, HtmlExporter, Input, Layout, Options, Paper, ProblemKind};

fn options(layout: Layout) -> Options {
    Options {
        layout,
        keep_tag_line: false,
        abc_source: false,
        paper: Paper::A4,
        contents: false,
        page_numbers: false,
    }
}

fn one(path: &str) -> Input {
    Input {
        notes: vec![path.to_owned()],
        name: None,
        scores: Vec::new(),
    }
}

fn vault_with(dir: &std::path::Path, file: &str, bytes: &[u8]) -> Vault {
    let root = dir.join("vault");
    fs::create_dir_all(&root).unwrap();
    let vault = Vault::create(&root).unwrap();
    fs::write(root.join("notes").join(file), bytes).unwrap();
    vault
        .create_note("note.md", &format!("# Note\n\n![A view]({file})\n"))
        .unwrap();
    vault
}

#[test]
fn a_picture_that_cannot_be_converted_stays_and_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let vault = vault_with(dir.path(), "broken.heic", b"this is no picture");
    let to = dir.path().join("note.html");
    let report = HtmlExporter
        .export(&vault, &one("note.md"), &options(Layout::SingleFile), &to)
        .unwrap();
    assert_eq!(report.problems.len(), 1, "{report:?}");
    assert_eq!(report.problems[0].kind, ProblemKind::PictureNotConverted);
    assert_eq!(report.problems[0].target, "broken.heic");
    assert!(
        fs::read_to_string(&to)
            .unwrap()
            .contains("data:image/heic;base64,")
    );
}

#[test]
fn other_pictures_are_left_alone() {
    let dir = tempfile::tempdir().unwrap();
    let vault = vault_with(dir.path(), "view.png", b"\x89PNG not really");
    let to = dir.path().join("note.html");
    let report = HtmlExporter
        .export(&vault, &one("note.md"), &options(Layout::SingleFile), &to)
        .unwrap();
    assert_eq!(report.problems, []);
    assert!(
        fs::read_to_string(&to)
            .unwrap()
            .contains("data:image/png;base64,")
    );
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod with_a_converter {
    use super::*;
    use schnipsle_export::OdtExporter;

    /// One white pixel as an uncompressed little-endian TIFF.
    fn tiff() -> Vec<u8> {
        let mut file = b"II*\0".to_vec();
        file.extend(12u32.to_le_bytes());
        file.extend([0xff, 0xff, 0xff, 0]);
        let tags: [(u16, u16, u32, u32); 8] = [
            (256, 4, 1, 1),
            (257, 4, 1, 1),
            (258, 3, 3, 114),
            (259, 3, 1, 1),
            (262, 3, 1, 2),
            (273, 4, 1, 8),
            (277, 3, 1, 3),
            (279, 4, 1, 3),
        ];
        file.extend(8u16.to_le_bytes());
        for (tag, kind, count, value) in tags {
            file.extend(tag.to_le_bytes());
            file.extend(kind.to_le_bytes());
            file.extend(count.to_le_bytes());
            file.extend(value.to_le_bytes());
        }
        file.extend(0u32.to_le_bytes());
        file.extend([8u8, 0, 8, 0, 8, 0]);
        file
    }

    #[test]
    fn a_document_holds_the_picture_converted() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_with(dir.path(), "view.tiff", &tiff());
        let to = dir.path().join("note.html");
        let report = HtmlExporter
            .export(&vault, &one("note.md"), &options(Layout::SingleFile), &to)
            .unwrap();
        assert_eq!(report.problems, []);
        let page = fs::read_to_string(&to).unwrap();
        assert!(page.contains("data:image/jpeg;base64,"), "{page}");
        assert!(!page.contains("data:image/tiff"));
    }

    #[test]
    fn a_folder_gets_the_copy_converted_and_never_the_original() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_with(dir.path(), "view.tiff", &tiff());
        let to = dir.path().join("out");
        let report = HtmlExporter
            .export(&vault, &one("note.md"), &options(Layout::FilePerNote), &to)
            .unwrap();
        assert_eq!(report.problems, []);
        assert!(
            report.files.iter().any(|f| f == "notes/view.tiff.jpg"),
            "{report:?}"
        );
        assert_eq!(
            &fs::read(to.join("notes/view.tiff.jpg")).unwrap()[..2],
            [0xff, 0xd8]
        );
        assert!(!to.join("notes/view.tiff").exists());
        let page = fs::read_to_string(to.join("Note.html")).unwrap();
        assert!(page.contains("notes/view.tiff.jpg"), "{page}");
    }

    /// Stored 24 × 40, shown turned by its orientation as 40 × 24.
    const TURNED: &[u8] = include_bytes!("../../../tests/fixtures/export/pictures/turned.jpg");

    #[test]
    fn odt_gets_a_turned_picture_upright() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_with(dir.path(), "turned.jpg", TURNED);
        let to = dir.path().join("note.odt");
        let report = OdtExporter
            .export(&vault, &one("note.md"), &options(Layout::SingleFile), &to)
            .unwrap();
        assert_eq!(report.problems, []);
        let mut odt = zip::ZipArchive::new(fs::File::open(&to).unwrap()).unwrap();
        let mut picture = Vec::new();
        for index in 0..odt.len() {
            let mut part = odt.by_index(index).unwrap();
            if part.name().unwrap().starts_with("Pictures/") {
                std::io::Read::read_to_end(&mut part, &mut picture).unwrap();
            }
        }
        assert_ne!(picture, TURNED);
        let size = imagesize::blob_size(&picture).unwrap();
        assert_eq!((size.width, size.height), (40, 24));
    }

    #[test]
    fn html_keeps_a_turned_picture_as_it_is() {
        let dir = tempfile::tempdir().unwrap();
        let vault = vault_with(dir.path(), "turned.jpg", TURNED);
        let to = dir.path().join("note.html");
        HtmlExporter
            .export(&vault, &one("note.md"), &options(Layout::SingleFile), &to)
            .unwrap();
        let page = fs::read_to_string(&to).unwrap();
        let original = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, TURNED);
        assert!(page.contains(&original));
    }
}
