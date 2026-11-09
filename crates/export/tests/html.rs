// SPDX-License-Identifier: Apache-2.0
//! The HTML export of the sample vault against the golden files in
//! `tests/fixtures/export/html/expected/`. Run with `SCHNIPSLE_BLESS=1` to write
//! `expected/` from the current output after a deliberate change.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use schnipsle_core::vault::Vault;
use schnipsle_export::{
    Exporter, Format, HtmlExporter, Input, Layout, Options, Paper, ProblemKind, Report, Score,
};

/// The notes in the order of the README's table.
const NOTES: [&str; 6] = [
    "welcome.md",
    "packing-list.md",
    "Radio/antenna-tuner.md",
    "Recipes/bread-rolls.md",
    "Songs/abendlied.md",
    "raw-html.md",
];

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/export")
}

/// The kanon of `Songs/abendlied.md`, engraved by abcjs 6.7.2 as the app does
/// (ADR-0033).
fn scores() -> Vec<Score> {
    vec![Score {
        source: "X:1\nT:Kanon\nM:4/4\nL:1/4\nK:D\nf2 e2 | d2 c2 |\n".to_owned(),
        transpose: 0,
        tunes: vec![fs::read_to_string(fixtures().join("scores/kanon.svg")).unwrap()],
    }]
}

fn vault() -> Vault {
    Vault::open(fixtures().join("vault")).unwrap()
}

fn all_notes() -> Input {
    Input {
        notes: NOTES.iter().map(|p| (*p).to_owned()).collect(),
        name: Some("Samples".to_owned()),
        scores: scores(),
    }
}

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

fn files(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Vec<u8>>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let entry = entry.unwrap();
        let name = format!("{prefix}{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            files(&entry.path(), &format!("{name}/"), out);
        } else {
            out.insert(name, fs::read(entry.path()).unwrap());
        }
    }
}

/// What a folder holds as golden files: the pages, and the other files by name
/// after checking they equal those in the vault.
fn golden(folder: &Path, prefix: &str, report: &Report, out: &mut BTreeMap<String, Vec<u8>>) {
    let mut found = BTreeMap::new();
    files(folder, "", &mut found);
    let mut copies = String::new();
    for (path, bytes) in found {
        if Path::new(&path).extension().is_some_and(|e| e == "html") {
            out.insert(format!("{prefix}/{path}"), bytes);
        } else {
            assert_eq!(
                bytes,
                fs::read(fixtures().join("vault").join(&path)).unwrap()
            );
            copies.push_str(&path);
            copies.push('\n');
        }
    }
    if !copies.is_empty() {
        out.insert(format!("{prefix}/copied.txt"), copies.into_bytes());
    }
    let mut json = serde_json::to_vec_pretty(report).unwrap();
    json.push(b'\n');
    out.insert(format!("{prefix}/report.json"), json);
}

#[test]
fn golden_files() {
    let vault = vault();
    let exporter = HtmlExporter;
    assert_eq!(exporter.format(), Format::Html);
    let mut actual = BTreeMap::new();

    // Each note alone, holding its pictures.
    for note in NOTES {
        let dir = tempfile::tempdir().unwrap();
        let input = Input {
            notes: vec![note.to_owned()],
            name: None,
            scores: scores(),
        };
        let to = dir.path().join("out.html");
        let report = exporter
            .export(&vault, &input, &options(Layout::SingleFile), &to)
            .unwrap();
        let stem = Path::new(note).file_stem().unwrap().to_string_lossy();
        assert_eq!(report.files, ["out.html"]);
        actual.insert(format!("single-file/{stem}.html"), fs::read(&to).unwrap());
        let mut json = serde_json::to_vec_pretty(&report.problems).unwrap();
        json.push(b'\n');
        actual.insert(format!("single-file/{stem}.report.json"), json);
    }

    // All notes as one document holding its pictures.
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("all.html");
    let report = exporter
        .export(&vault, &all_notes(), &options(Layout::SingleFile), &to)
        .unwrap();
    actual.insert("one-document/all.html".to_owned(), fs::read(&to).unwrap());
    let mut json = serde_json::to_vec_pretty(&report).unwrap();
    json.push(b'\n');
    actual.insert("one-document/report.json".to_owned(), json);

    // All notes as one document in a folder, and as a document each.
    for (layout, prefix) in [
        (Layout::Folder, "folder"),
        (Layout::FilePerNote, "file-per-note"),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("out");
        let report = exporter
            .export(&vault, &all_notes(), &options(layout), &to)
            .unwrap();
        golden(&to, prefix, &report, &mut actual);
    }

    check(&actual);
}

fn check(actual: &BTreeMap<String, Vec<u8>>) {
    let expected_dir = fixtures().join("html/expected");
    if std::env::var_os("SCHNIPSLE_BLESS").is_some() {
        let _ = fs::remove_dir_all(&expected_dir);
        for (path, bytes) in actual {
            let file = expected_dir.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, bytes).unwrap();
        }
        return;
    }
    let mut expected = BTreeMap::new();
    files(&expected_dir, "", &mut expected);
    assert_eq!(
        actual.keys().collect::<Vec<_>>(),
        expected.keys().collect::<Vec<_>>()
    );
    for (path, bytes) in actual {
        assert_eq!(
            String::from_utf8_lossy(bytes),
            String::from_utf8_lossy(&expected[path]),
            "{path}"
        );
    }
}

fn export_note(input: &Input, options: Options) -> (String, Report) {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("out.html");
    let report = HtmlExporter.export(&vault(), input, &options, &to).unwrap();
    (fs::read_to_string(&to).unwrap_or_default(), report)
}

fn one(note: &str) -> Input {
    Input {
        notes: vec![note.to_owned()],
        name: None,
        scores: scores(),
    }
}

#[test]
fn the_final_tag_line_is_left_out_unless_kept() {
    let (html, _) = export_note(&one("welcome.md"), options(Layout::SingleFile));
    assert!(!html.contains("#schnipsle"));
    assert!(html.contains("Tags such as #radio and #projects/tuner stay text"));
    let keep = Options {
        layout: Layout::SingleFile,
        keep_tag_line: true,
        abc_source: false,
        paper: Paper::A4,
        contents: false,
        page_numbers: false,
    };
    let (html, _) = export_note(&one("welcome.md"), keep);
    assert!(html.contains("#schnipsle #demo"));
}

#[test]
fn a_note_outside_notes_is_never_exported() {
    let (html, report) = export_note(
        &one("../.notesapp/trash/01JC4X00000000000000000007.md"),
        options(Layout::SingleFile),
    );
    assert_eq!(html, "");
    assert!(report.files.is_empty());
    assert_eq!(report.problems.len(), 1);
    assert_eq!(report.problems[0].kind, ProblemKind::NoteNotRead);
}

#[test]
fn nothing_scripted_gets_through() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path()).unwrap();
    let text = "# Bad\n\n[a](javascript:alert(1)) [b](JAVASCRIPT:x) [c](data:text/html,x)\n\n\
                ![d](javascript:alert(2)) <img src=x onerror=alert(3)> <svg onload=alert(4)>\n\n\
                [e](#fragment) [f](/etc/passwd) [g](../../secret.txt)\n";
    vault.create_note("bad.md", text).unwrap();
    let to = dir.path().join("out.html");
    let report = HtmlExporter
        .export(&vault, &one("bad.md"), &options(Layout::SingleFile), &to)
        .unwrap();
    let html = fs::read_to_string(&to).unwrap();
    let body = &html[html.find("<body>").unwrap()..];
    for bad in [
        "javascript",
        "JAVASCRIPT",
        "onerror",
        "onload",
        "<img",
        "<svg",
        "href",
    ] {
        assert!(!body.contains(bad), "{bad} in {body}");
    }
    assert!(body.contains("a b c"), "{body}");
    assert!(body.contains("[d]"), "{body}");
    assert_eq!(report.problems.len(), 1, "{:?}", report.problems);
    assert_eq!(report.problems[0].kind, ProblemKind::PictureMissing);
}

#[test]
fn a_folder_export_never_replaces_what_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path();
    fs::write(to.join("Welcome.html"), "mine").unwrap();
    let report = HtmlExporter
        .export(
            &vault(),
            &one("welcome.md"),
            &options(Layout::FilePerNote),
            to,
        )
        .unwrap();
    assert_eq!(report.files, ["Welcome-2.html"]);
    assert_eq!(report.numbered, ["Welcome-2.html"]);
    assert_eq!(fs::read_to_string(to.join("Welcome.html")).unwrap(), "mine");

    // Exporting again reuses the attachments copied before.
    let input = one("Radio/antenna-tuner.md");
    let first = HtmlExporter
        .export(&vault(), &input, &options(Layout::Folder), to)
        .unwrap();
    let again = HtmlExporter
        .export(&vault(), &input, &options(Layout::Folder), to)
        .unwrap();
    assert_eq!(first.files[0], "Antenna tuner.html");
    assert_eq!(again.files[0], "Antenna tuner-2.html");
    assert!(first.numbered.is_empty());
    assert_eq!(again.numbered, ["Antenna tuner-2.html"]);
    assert_eq!(first.files[1..], again.files[1..]);
    assert_eq!(first.problems, again.problems);
}

#[test]
fn notes_with_the_same_title_have_their_own_anchors_and_footnotes() {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path()).unwrap();
    let text = "Same\n\nA note.[^1] See [[Same]].\n\n[^1]: Its footnote.\n";
    vault.create_note("a.md", text).unwrap();
    vault.create_note("b.md", text).unwrap();
    let to = dir.path().join("out.html");
    let input = Input {
        notes: vec!["a.md".to_owned(), "b.md".to_owned()],
        name: None,
        scores: scores(),
    };
    HtmlExporter
        .export(&vault, &input, &options(Layout::SingleFile), &to)
        .unwrap();
    let html = fs::read_to_string(&to).unwrap();
    assert!(html.contains("<title>Notes</title>"));
    assert!(html.contains("<section id=\"note-same\">"));
    assert!(html.contains("<section id=\"note-same-2\">"));
    assert!(html.contains("id=\"fn-note-same-1\""));
    assert!(html.contains("id=\"fn-note-same-2-1\""));
    // The first line is the title, so it becomes the heading and is not repeated.
    assert_eq!(html.matches("<h1>Same</h1>").count(), 2);
    assert!(!html.contains("<p>Same</p>"));
    assert_eq!(html.matches("<a href=\"#note-same\">Same</a>").count(), 2);
}
