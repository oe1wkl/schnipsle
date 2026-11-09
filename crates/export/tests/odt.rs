// SPDX-License-Identifier: Apache-2.0
//! The ODT export of the sample vault: the XML of each file against the golden
//! files in `tests/fixtures/export/odt/expected/`, and with `--ignored` `LibreOffice`
//! opening and converting all of them (ADR-0010). Run with `SCHNIPSLE_BLESS=1` to
//! write `expected/` after a deliberate change, and `SOFFICE` to name `LibreOffice`.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::io::Read as _;
use std::path::{Path, PathBuf};
use std::process::Command;

use schnipsle_core::vault::Vault;
use schnipsle_export::{
    Exporter, Format, Input, Layout, OdtExporter, Options, Paper, ProblemKind, Report, Score,
};
use zip::ZipArchive;

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

fn one(note: &str) -> Input {
    Input {
        notes: vec![note.to_owned()],
        name: None,
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

/// The parts of a file, in the order of the package.
fn parts(file: &Path) -> Vec<(String, Vec<u8>)> {
    let mut zip = ZipArchive::new(fs::File::open(file).unwrap()).unwrap();
    (0..zip.len())
        .map(|index| {
            let mut part = zip.by_index(index).unwrap();
            let mut bytes = Vec::new();
            part.read_to_end(&mut bytes).unwrap();
            (part.name().unwrap().into_owned(), bytes)
        })
        .collect()
}

fn part(file: &Path, name: &str) -> String {
    let (_, bytes) = parts(file).into_iter().find(|(n, _)| n == name).unwrap();
    String::from_utf8(bytes).unwrap()
}

/// The golden form of a file: its XML, and the size of its other parts.
fn add_file(actual: &mut BTreeMap<String, Vec<u8>>, prefix: &str, file: &Path) {
    let mut listing = String::new();
    for (name, bytes) in parts(file) {
        if name == "styles.xml" {
            actual.insert("styles-a4.xml".to_owned(), bytes);
        } else if Path::new(&name).extension().is_some_and(|e| e == "xml") {
            let name = name.replace("META-INF/", "");
            actual.insert(format!("{prefix}/{name}"), bytes);
        } else {
            let _ = writeln!(listing, "{name} {}", bytes.len());
        }
    }
    actual.insert(format!("{prefix}/files.txt"), listing.into_bytes());
}

fn pretty(report: &Report) -> Vec<u8> {
    let mut json = serde_json::to_vec_pretty(report).unwrap();
    json.push(b'\n');
    json
}

#[test]
fn golden_files() {
    let vault = vault();
    assert_eq!(OdtExporter.format(), Format::Odt);
    let mut actual = BTreeMap::new();

    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("all.odt");
    let report = OdtExporter
        .export(&vault, &all_notes(), &options(Layout::SingleFile), &to)
        .unwrap();
    assert_eq!(report.files, ["all.odt"]);
    add_file(&mut actual, "one-document/all", &to);
    actual.insert("one-document/report.json".to_owned(), pretty(&report));

    let to = dir.path().join("out");
    let report = OdtExporter
        .export(&vault, &all_notes(), &options(Layout::FilePerNote), &to)
        .unwrap();
    assert_eq!(report.files.len(), 6);
    for name in &report.files {
        let stem = Path::new(name).file_stem().unwrap().to_string_lossy();
        add_file(
            &mut actual,
            &format!("file-per-note/{stem}"),
            &to.join(name),
        );
    }
    actual.insert("file-per-note/report.json".to_owned(), pretty(&report));

    check(&actual);
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

fn check(actual: &BTreeMap<String, Vec<u8>>) {
    let expected_dir = fixtures().join("odt/expected");
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

#[test]
fn the_package_starts_with_its_type_unpacked() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("a.odt");
    OdtExporter
        .export(
            &vault(),
            &one("welcome.md"),
            &options(Layout::SingleFile),
            &to,
        )
        .unwrap();
    let mut zip = ZipArchive::new(fs::File::open(&to).unwrap()).unwrap();
    let first = zip.by_index(0).unwrap();
    assert_eq!(first.name().unwrap(), "mimetype");
    assert_eq!(first.compression(), zip::CompressionMethod::Stored);
    assert_eq!(first.size(), 39);
    drop(first);
    let bytes = fs::read(&to).unwrap();
    assert_eq!(&bytes[30..38], b"mimetype");
    assert_eq!(&bytes[38..77], b"application/vnd.oasis.opendocument.text");
}

#[test]
fn an_export_does_not_differ_by_when_it_ran() {
    let dir = tempfile::tempdir().unwrap();
    let (first, second) = (dir.path().join("1.odt"), dir.path().join("2.odt"));
    for to in [&first, &second] {
        OdtExporter
            .export(&vault(), &all_notes(), &options(Layout::SingleFile), to)
            .unwrap();
    }
    assert_eq!(fs::read(first).unwrap(), fs::read(second).unwrap());
}

#[test]
fn metadata_of_a_note_and_of_a_list() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("a.odt");
    OdtExporter
        .export(
            &vault(),
            &one("welcome.md"),
            &options(Layout::SingleFile),
            &to,
        )
        .unwrap();
    let meta = part(&to, "meta.xml");
    assert!(meta.contains("<dc:title>Welcome</dc:title>"), "{meta}");
    assert!(
        meta.contains("<meta:creation-date>2026-10-01T07:00:00Z<"),
        "{meta}"
    );
    assert!(meta.contains("<dc:date>2026-10-20T16:30:00Z<"), "{meta}");

    OdtExporter
        .export(&vault(), &all_notes(), &options(Layout::SingleFile), &to)
        .unwrap();
    let meta = part(&to, "meta.xml");
    assert!(meta.contains("<dc:title>Samples</dc:title>"), "{meta}");
    assert!(!meta.contains("creation-date"), "{meta}");
}

#[test]
fn letter_paper_and_a_folder_with_a_free_name() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Samples.odt"), "mine").unwrap();
    let report = OdtExporter
        .export(
            &vault(),
            &all_notes(),
            &Options {
                layout: Layout::Folder,
                keep_tag_line: false,
                abc_source: false,
                paper: Paper::Letter,
                contents: false,
                page_numbers: false,
            },
            dir.path(),
        )
        .unwrap();
    assert_eq!(report.files, ["Samples-2.odt"]);
    assert_eq!(report.numbered, ["Samples-2.odt"]);
    assert_eq!(fs::read(dir.path().join("Samples.odt")).unwrap(), b"mine");
    let styles = part(&dir.path().join("Samples-2.odt"), "styles.xml");
    assert!(styles.contains("fo:page-width=\"21.59cm\""), "{styles}");
    assert!(!styles.contains('{'), "{styles}");
    let content = part(&dir.path().join("Samples-2.odt"), "content.xml");
    assert!(content.contains("style:width=\"17.59cm\""), "{content}");
}

#[test]
fn links_follow_the_rules_of_the_layout() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("all.odt");
    let report = OdtExporter
        .export(&vault(), &all_notes(), &options(Layout::SingleFile), &to)
        .unwrap();
    let content = part(&to, "content.xml");
    assert!(
        content.contains("xlink:href=\"#note-welcome\""),
        "{content}"
    );
    assert!(content.contains("<text:bookmark text:name=\"note-welcome\"/>"));
    assert!(
        !report
            .problems
            .iter()
            .any(|p| p.kind == ProblemKind::NoteInOtherFile)
    );

    let to = dir.path().join("out");
    let report = OdtExporter
        .export(&vault(), &all_notes(), &options(Layout::FilePerNote), &to)
        .unwrap();
    let content = part(&to.join("Antenna tuner.odt"), "content.xml");
    assert!(
        content.contains("xlink:href=\"../Welcome.odt\""),
        "{content}"
    );
    assert!(
        !report
            .problems
            .iter()
            .any(|p| p.kind == ProblemKind::NoteInOtherFile)
    );
}

#[test]
fn attachments_stay_behind_and_pictures_are_inside() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("a.odt");
    let report = OdtExporter
        .export(
            &vault(),
            &one("Radio/antenna-tuner.md"),
            &options(Layout::SingleFile),
            &to,
        )
        .unwrap();
    let kinds: Vec<ProblemKind> = report.problems.iter().map(|p| p.kind).collect();
    assert!(kinds.contains(&ProblemKind::AttachmentNotIncluded));
    assert!(kinds.contains(&ProblemKind::WebPictureAsLink));
    assert!(kinds.contains(&ProblemKind::PictureMissing));
    let names: Vec<String> = parts(&to).into_iter().map(|(name, _)| name).collect();
    assert!(
        names.iter().any(|n| n.starts_with("Pictures/")),
        "{names:?}"
    );
    let manifest = part(&to, "META-INF/manifest.xml");
    assert!(
        manifest.contains("manifest:media-type=\"image/"),
        "{manifest}"
    );
    assert_eq!(fs::read_dir(dir.path()).unwrap().count(), 1);
}

/// A note with what the sample vault does not have.
const DIALECT: &str = "# Grüße & \\<mehr\\>\n\n\
Link to [itself](gr%C3%BC%C3%9Fe.md) and control\u{1} text.\n\n\
1. one\n2. two\n\nThen:\n\n\
5. five\n   - nested\n     - deeper\n\n\
- [ ] open\n- [x] done\n- plain\n\n\
- in a list\n\n  | a | b |\n  | - | - |\n  | c | d |\n\n\
> quote\n>\n> > deeper\n>\n> ```\n> code in   quote\n> ```\n\n\
Twice[^a] and[^a] nested[^b].\n\n\
[^a]: First note.\n\n\
[^b]: Holds[^c].\n\n\
[^c]: Inner note.\n\n\
![A drawing](drawing.svg)\n";

const SVG: &str = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"200\" height=\"100\"><rect width=\"200\" height=\"100\" fill=\"#8a6\"/></svg>";

fn dialect_vault(dir: &Path) -> Vault {
    fs::create_dir_all(dir).unwrap();
    let vault = Vault::create(dir).unwrap();
    vault.create_note("grüße.md", DIALECT).unwrap();
    fs::write(dir.join("notes/drawing.svg"), SVG).unwrap();
    vault
}

#[test]
fn what_the_format_cannot_hold_keeps_its_text() {
    let dir = tempfile::tempdir().unwrap();
    let vault = dialect_vault(&dir.path().join("vault"));
    let to = dir.path().join("a.odt");
    OdtExporter
        .export(&vault, &one("grüße.md"), &options(Layout::SingleFile), &to)
        .unwrap();
    let content = part(&to, "content.xml");
    assert!(!content.contains('\u{1}'));
    assert!(content.contains("Grüße &amp; &lt;mehr&gt;"), "{content}");
    assert!(content.contains("text:start-value=\"5\""), "{content}");
    // A table in a list has no place in the format.
    assert!(!content.contains("<table:table "), "{content}");
    assert!(content.contains("c<text:tab/>d"), "{content}");
    // Each reference has a note of its own, none inside another.
    assert_eq!(content.matches("<text:note ").count(), 3, "{content}");
    assert!(content.contains("Inner note."), "{content}");
    assert_eq!(content.matches("First note.").count(), 2, "{content}");
    // 200 × 100 pixels, as the SVG says.
    assert!(
        content.contains("svg:width=\"5.292cm\" svg:height=\"2.646cm\""),
        "{content}"
    );
    assert!(
        content.contains("xlink:href=\"#note-grüße-mehr\""),
        "{content}"
    );
    assert!(
        content.contains("<text:bookmark text:name=\"note-grüße-mehr\"/>"),
        "{content}"
    );
}

/// The start of a PNG file, as much as tells its size.
fn png_header(width: u32, height: u32) -> Vec<u8> {
    let mut bytes = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
    bytes.extend_from_slice(&width.to_be_bytes());
    bytes.extend_from_slice(&height.to_be_bytes());
    bytes.extend_from_slice(&[8, 6, 0, 0, 0]);
    bytes
}

#[test]
fn pictures_fit_the_page_and_the_cell_and_are_stored_once() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("vault");
    fs::create_dir_all(&root).unwrap();
    let vault = Vault::create(&root).unwrap();
    fs::write(root.join("notes/wide.png"), png_header(4000, 1000)).unwrap();
    fs::write(root.join("notes/small.png"), png_header(96, 48)).unwrap();
    fs::write(root.join("notes/tall.png"), png_header(2000, 8000)).unwrap();
    vault
        .create_note(
            "pictures.md",
            "![](small.png)\n\n![](wide.png)\n\n![](wide.png)\n\n![](tall.png)\n\n| a | b |\n| - | - |\n| ![](wide.png) | x |\n",
        )
        .unwrap();
    let to = dir.path().join("a.odt");
    OdtExporter
        .export(
            &vault,
            &one("pictures.md"),
            &options(Layout::SingleFile),
            &to,
        )
        .unwrap();
    let content = part(&to, "content.xml");
    assert!(
        content.contains("svg:width=\"2.540cm\" svg:height=\"1.270cm\""),
        "{content}"
    );
    assert!(
        content.contains("svg:width=\"17.000cm\" svg:height=\"4.250cm\""),
        "{content}"
    );
    assert!(
        content.contains("svg:width=\"8.200cm\" svg:height=\"2.050cm\""),
        "{content}"
    );
    // Taller than the page, an office program would cut it off.
    assert!(
        content.contains("svg:width=\"6.425cm\" svg:height=\"25.700cm\""),
        "{content}"
    );
    assert_eq!(content.matches("<draw:frame ").count(), 5, "{content}");
    let names: Vec<String> = parts(&to).into_iter().map(|(name, _)| name).collect();
    assert_eq!(
        names.iter().filter(|n| n.starts_with("Pictures/")).count(),
        3
    );
}

fn soffice() -> String {
    if let Some(path) = std::env::var_os("SOFFICE") {
        return path.to_string_lossy().into_owned();
    }
    let mac = "/Applications/LibreOffice.app/Contents/MacOS/soffice";
    if Path::new(mac).exists() {
        mac.to_owned()
    } else {
        "soffice".to_owned()
    }
}

/// Has `LibreOffice` convert `files` in `to`, with a profile of its own so a
/// running `LibreOffice` does not get in the way.
fn convert(files: &[PathBuf], format: &str, to: &Path, profile: &Path) {
    let status = Command::new(soffice())
        .arg(format!(
            "-env:UserInstallation=file://{}",
            profile.display()
        ))
        .args(["--headless", "--convert-to", format, "--outdir"])
        .arg(to)
        .args(files)
        .status()
        .expect("LibreOffice is needed for this test: set SOFFICE");
    assert!(status.success());
}

/// ADR-0010: `LibreOffice` opens and converts every file; the text shows it read them.
#[test]
#[ignore = "needs LibreOffice; CI runs it in a job of its own"]
fn libreoffice_converts_every_file() {
    let dir = tempfile::tempdir().unwrap();
    let odt_folder = dir.path().join("odt");
    let vault = vault();
    let mut odt = Vec::new();

    let all = odt_folder.join("all.odt");
    fs::create_dir_all(&odt_folder).unwrap();
    OdtExporter
        .export(&vault, &all_notes(), &options(Layout::SingleFile), &all)
        .unwrap();
    odt.push(all);
    let report = OdtExporter
        .export(
            &vault,
            &all_notes(),
            &options(Layout::FilePerNote),
            &odt_folder,
        )
        .unwrap();
    odt.extend(report.files.iter().map(|name| odt_folder.join(name)));
    let other = dialect_vault(&dir.path().join("vault"));
    let dialect = odt_folder.join("dialect.odt");
    OdtExporter
        .export(
            &other,
            &one("grüße.md"),
            &options(Layout::SingleFile),
            &dialect,
        )
        .unwrap();
    odt.push(dialect);

    let profile = dir.path().join("profile");
    let pdf_folder = dir.path().join("pdf");
    let text_folder = dir.path().join("txt");
    convert(&odt, "pdf", &pdf_folder, &profile);
    convert(&odt, "txt:Text (encoded):UTF8", &text_folder, &profile);
    if let Some(keep) = std::env::var_os("SCHNIPSLE_ODT_KEEP") {
        for folder in [&odt_folder, &pdf_folder, &text_folder] {
            let keep = Path::new(&keep).join(folder.file_name().unwrap());
            fs::create_dir_all(&keep).unwrap();
            for entry in fs::read_dir(folder).unwrap() {
                let entry = entry.unwrap();
                fs::copy(entry.path(), keep.join(entry.file_name())).unwrap();
            }
        }
    }
    for file in &odt {
        let stem = file.file_stem().unwrap().to_string_lossy().into_owned();
        let made = pdf_folder.join(format!("{stem}.pdf"));
        assert!(fs::metadata(&made).unwrap().len() > 1000, "{stem}");
    }
    let text = fs::read_to_string(text_folder.join("all.txt")).unwrap();
    for expected in [
        "Welcome",
        "Packing list for Field Day",
        "Antenna tuner",
        "Bread rolls",
        "Abendlied",
        "Raw HTML",
        "Bake at 230",
        "☐",
        "☑",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
    assert!(!text.contains("#schnipsle #demo"), "{text}");
    let text = fs::read_to_string(text_folder.join("dialect.txt")).unwrap();
    for expected in [
        "Grüße & <mehr>",
        "5. five",
        "deeper",
        "code in   quote",
        "c\td",
    ] {
        assert!(text.contains(expected), "{expected} in {text}");
    }
}

#[test]
fn contents_and_page_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("samples.odt");
    let with_contents = Options {
        contents: true,
        ..options(Layout::SingleFile)
    };
    OdtExporter
        .export(&vault(), &all_notes(), &with_contents, &to)
        .unwrap();
    let content = part(&to, "content.xml");
    let start = content.find("Contents_20_Heading").unwrap();
    let first_note = content
        .find("text:bookmark text:name=\"note-welcome\"")
        .unwrap();
    assert!(start < first_note, "{content}");
    assert_eq!(content.matches("text:reference-format=\"page\"").count(), 6);
    assert!(content.contains("xlink:href=\"#note-bread-rolls\" text:style-name=\"Index_20_Link\" text:visited-style-name=\"Index_20_Link\">Bread rolls</text:a><text:tab/><text:bookmark-ref text:reference-format=\"page\" text:ref-name=\"note-bread-rolls\">"), "{content}");
    // Every note starts a new page, the first one too.
    assert_eq!(content.matches("Note_20_Start").count(), 6);
    let styles = part(&to, "styles.xml");
    assert!(styles.contains("<text:page-number"), "{styles}");
    assert!(styles.contains("style:position=\"17.00cm\""), "{styles}");
    assert!(styles.contains("fo:margin-bottom=\"1cm\""), "{styles}");
    if let Some(keep) = std::env::var_os("SCHNIPSLE_ODT_KEEP") {
        fs::create_dir_all(&keep).unwrap();
        fs::copy(&to, Path::new(&keep).join("contents.odt")).unwrap();
    }

    // Page numbers alone, and no contents for a single note.
    let numbered = Options {
        page_numbers: true,
        contents: true,
        ..options(Layout::SingleFile)
    };
    OdtExporter
        .export(&vault(), &one("welcome.md"), &numbered, &to)
        .unwrap();
    assert!(!part(&to, "content.xml").contains("Contents_20"));
    assert!(part(&to, "styles.xml").contains("<text:page-number"));
}
