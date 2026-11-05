// SPDX-License-Identifier: Apache-2.0
//! Pictures few programs show (ADR-0027), in the formats that hold them and in a
//! folder of copies. The conversion itself is tested in `schnipsle-apple`.

use std::fs;

use schnipsle_core::vault::Vault;
use schnipsle_export::{Exporter as _, HtmlExporter, Input, Layout, Options, Paper, ProblemKind};

fn options(layout: Layout) -> Options {
    Options {
        layout,
        keep_tag_line: false,
        paper: Paper::A4,
    }
}

fn one(path: &str) -> Input {
    Input {
        notes: vec![path.to_owned()],
        name: None,
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
}
