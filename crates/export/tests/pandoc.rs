// SPDX-License-Identifier: Apache-2.0
//! DOCX and EPUB through Pandoc (ADR-0025): the Markdown and what else goes to
//! Pandoc against the golden files in `tests/fixtures/export/pandoc/expected/`,
//! and with `--ignored` the real Pandoc writing all of it. Run with
//! `SCHNIPSLE_BLESS=1` to write `expected/` after a deliberate change, and
//! `PANDOC` to name the program.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashSet};
use std::fmt::Write as _;
use std::fs;
use std::io::{self, Cursor, Read as _, Write as _};
use std::path::{Path, PathBuf};

use schnipsle_core::vault::Vault;
use schnipsle_export::{
    ConversionJob, Converted, Converter, Exporter, Format, Input, Layout, Options, Pandoc,
    PandocExporter, Paper, ProblemKind, Report, Score,
};
use zip::ZipArchive;
use zip::write::{SimpleFileOptions, ZipWriter};

const NOTES: [&str; 6] = [
    "welcome.md",
    "packing-list.md",
    "Radio/antenna-tuner.md",
    "Recipes/bread-rolls.md",
    "Songs/abendlied.md",
    "raw-html.md",
];

/// What Pandoc would have to read back as written.
const TRICKY: &str = "# Grüße {#mine}\n\nCode `a`{.b} and [link](https://example.org){.c}, $x$ and $y$, :smile:, {braces}. See [the other](other.md).\n\n::: not a div\n\n```{=html}\n<b>kept as code</b>\n```\n\nA note.[^1]\n\n[^1]: Its text.\n";
const OTHER: &str = "# Andere Überschrift\n\nBack to [Grüße {#mine}](gr%C3%BC%C3%9Fe.md).\n";

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/export")
}

/// The kanon of `Songs/abendlied.md`, engraved by abcjs 6.7.2 as the app does
/// (ADR-0033).
/// The warnings of a report but that a DOCX holds the score of `scores` as SVG
/// only, which it does where `rsvg-convert` is missing, as in CI (ADR-0033).
fn other_warnings(report: &Report) -> Vec<&str> {
    report
        .warnings
        .iter()
        .map(String::as_str)
        .filter(|w| !w.contains("“Kanon” is in the document as SVG only"))
        .collect()
}

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

fn tricky_vault(dir: &Path) -> Vault {
    fs::create_dir_all(dir).unwrap();
    let vault = Vault::create(dir).unwrap();
    vault.create_note("grüße.md", TRICKY).unwrap();
    vault.create_note("other.md", OTHER).unwrap();
    vault
}

fn all_notes() -> Input {
    Input {
        notes: NOTES.iter().map(|p| (*p).to_owned()).collect(),
        name: Some("Samples".to_owned()),
        scores: scores(),
    }
}

fn tricky_notes() -> Input {
    Input {
        notes: vec!["grüße.md".to_owned(), "other.md".to_owned()],
        name: Some("Tricky".to_owned()),
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

/// Keeps the jobs and answers with a small package of the parts a DOCX has.
#[derive(Default)]
struct FakeConverter {
    jobs: RefCell<Vec<ConversionJob>>,
    fail: bool,
}

impl Converter for &FakeConverter {
    fn convert(&self, job: &ConversionJob) -> io::Result<Converted> {
        self.jobs.borrow_mut().push(job.clone());
        if self.fail {
            return Err(io::Error::other("Pandoc stopped: no"));
        }
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, text) in [
            ("docProps/core.xml", "<c><dc:title></dc:title></c>"),
            ("word/document.xml", "<w:body><w:sectPr /></w:body>"),
        ] {
            writer.start_file(name, SimpleFileOptions::default())?;
            writer.write_all(text.as_bytes())?;
        }
        Ok(Converted {
            bytes: writer.finish()?.into_inner(),
            warnings: vec!["A warning".to_owned()],
        })
    }
}

fn pretty(report: &Report) -> Vec<u8> {
    let mut json = serde_json::to_vec_pretty(report).unwrap();
    json.push(b'\n');
    json
}

/// The golden form of a job: its Markdown, and the rest in a small text file.
fn add_job(actual: &mut BTreeMap<String, Vec<u8>>, name: &str, job: &ConversionJob) {
    actual.insert(format!("{name}.md"), job.markdown.clone().into_bytes());
    let mut text = format!(
        "format: {}\ntitle: {}\ncreated: {:?}\nmodified: {:?}\n",
        job.format.extension(),
        job.title,
        job.created,
        job.modified_ms
    );
    for picture in &job.pictures {
        let _ = writeln!(text, "picture: {} {}", picture.name, picture.bytes.len());
    }
    actual.insert(format!("{name}.job.txt"), text.into_bytes());
}

/// Exports the samples as one document and a file per note.
fn add_format(actual: &mut BTreeMap<String, Vec<u8>>, format: Format) {
    let vault = vault();
    let prefix = format.extension();

    let fake = FakeConverter::default();
    let exporter = PandocExporter::new(format, &fake);
    assert_eq!(exporter.format(), format);
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join(format!("all.{prefix}"));
    let report = exporter
        .export(&vault, &all_notes(), &options(Layout::SingleFile), &to)
        .unwrap();
    assert!(to.is_file());
    assert_eq!(report.files, [format!("all.{prefix}")]);
    let jobs = fake.jobs.borrow();
    assert_eq!(jobs.len(), 1);
    add_job(actual, &format!("{prefix}/one-document/all"), &jobs[0]);
    actual.insert(
        format!("{prefix}/one-document/report.json"),
        pretty(&report),
    );

    let fake = FakeConverter::default();
    let to = dir.path().join("out");
    let report = PandocExporter::new(format, &fake)
        .export(&vault, &all_notes(), &options(Layout::FilePerNote), &to)
        .unwrap();
    let jobs = fake.jobs.borrow();
    assert_eq!(report.files.len(), 6);
    for (name, job) in report.files.iter().zip(jobs.iter()) {
        assert!(to.join(name).is_file(), "{name}");
        let stem = Path::new(name).file_stem().unwrap().to_string_lossy();
        add_job(actual, &format!("{prefix}/file-per-note/{stem}"), job);
    }
    actual.insert(
        format!("{prefix}/file-per-note/report.json"),
        pretty(&report),
    );
}

#[test]
fn golden_files() {
    let mut actual = BTreeMap::new();
    add_format(&mut actual, Format::Docx);
    add_format(&mut actual, Format::Epub);
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
    let expected_dir = fixtures().join("pandoc/expected");
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

/// The one job of exporting `input` from `vault`.
fn job_of(vault: &Vault, input: &Input, format: Format) -> ConversionJob {
    let fake = FakeConverter::default();
    let dir = tempfile::tempdir().unwrap();
    PandocExporter::new(format, &fake)
        .export(
            vault,
            input,
            &options(Layout::SingleFile),
            &dir.path().join("out"),
        )
        .unwrap();
    fake.jobs.borrow()[0].clone()
}

#[test]
fn what_markdown_could_take_for_something_else_is_neutral() {
    let dir = tempfile::tempdir().unwrap();
    let vault = tricky_vault(&dir.path().join("vault"));
    let job = job_of(&vault, &tricky_notes(), Format::Docx);
    let text = &job.markdown;
    // No braces left to be read as attributes, but for the identifiers we add.
    assert!(text.contains("`a`&#123;.b}"), "{text}");
    assert!(text.contains("# Grüße &#123;\\#mine} {#note-"), "{text}");
    assert!(!text.contains("```{=html}"), "{text}");
    assert!(text.contains("<b>kept as code</b>"), "{text}");
    // The link inside the document names its anchor undecoded.
    assert!(text.contains("(#note-andere-überschrift)"), "{text}");
    assert_eq!(text.matches("{=openxml}").count(), 1, "{text}");
}

#[test]
fn a_note_is_a_section_only_among_others() {
    let dir = tempfile::tempdir().unwrap();
    let vault = tricky_vault(&dir.path().join("vault"));
    let one = Input {
        notes: vec!["other.md".to_owned()],
        name: None,
        scores: scores(),
    };
    let job = job_of(&vault, &one, Format::Docx);
    assert!(job.markdown.starts_with("# Andere Überschrift\n"));
    assert_eq!(job.title, "Andere Überschrift");
    assert!(job.created.is_some());
    assert!(job.modified_ms.is_some());
    assert!(!job.markdown.contains("{=openxml}"));

    let job = job_of(&vault, &tricky_notes(), Format::Epub);
    assert_eq!(job.title, "Tricky");
    assert_eq!(job.created, None);
    assert_eq!(job.modified_ms, None);
    // Each note is a chapter of an e-book anyway.
    assert!(!job.markdown.contains("{=openxml}"));
}

#[test]
fn pictures_go_by_file_each_once() {
    let job = job_of(
        &vault(),
        &Input {
            notes: vec!["Recipes/bread-rolls.md".to_owned()],
            name: None,
            scores: scores(),
        },
        Format::Docx,
    );
    assert!(!job.markdown.contains("data:"), "{}", job.markdown);
    assert_eq!(job.pictures.len(), 1);
    assert_eq!(job.pictures[0].name, "picture-1.jpg");
    assert!(job.markdown.contains("(picture-1.jpg"), "{}", job.markdown);
}

#[test]
fn links_between_files_stay_only_in_docx() {
    let dir = tempfile::tempdir().unwrap();
    let vault = tricky_vault(&dir.path().join("vault"));
    let to = dir.path().join("out");

    let fake = FakeConverter::default();
    let report = PandocExporter::new(Format::Docx, &fake)
        .export(
            &vault,
            &tricky_notes(),
            &options(Layout::FilePerNote),
            &to.join("docx"),
        )
        .unwrap();
    let kinds: Vec<_> = report.problems.iter().map(|p| p.kind).collect();
    assert!(!kinds.contains(&ProblemKind::NoteInOtherFile), "{kinds:?}");
    let jobs = fake.jobs.borrow();
    assert!(
        jobs[0]
            .markdown
            .contains("(Andere%20%C3%9Cberschrift.docx)"),
        "{}",
        jobs[0].markdown
    );

    let fake = FakeConverter::default();
    let report = PandocExporter::new(Format::Epub, &fake)
        .export(
            &vault,
            &tricky_notes(),
            &options(Layout::FilePerNote),
            &to.join("epub"),
        )
        .unwrap();
    let kinds: Vec<_> = report.problems.iter().map(|p| p.kind).collect();
    assert_eq!(
        kinds
            .iter()
            .filter(|k| **k == ProblemKind::NoteInOtherFile)
            .count(),
        2
    );
    let jobs = fake.jobs.borrow();
    assert!(
        jobs[0].markdown.contains("See the other."),
        "{}",
        jobs[0].markdown
    );
}

#[test]
fn a_folder_export_never_replaces_what_is_there() {
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Samples.docx"), "mine").unwrap();
    let fake = FakeConverter::default();
    let report = PandocExporter::new(Format::Docx, &fake)
        .export(&vault(), &all_notes(), &options(Layout::Folder), dir.path())
        .unwrap();
    assert_eq!(report.files, ["Samples-2.docx"]);
    assert_eq!(report.numbered, ["Samples-2.docx"]);
    assert_eq!(fs::read(dir.path().join("Samples.docx")).unwrap(), b"mine");
}

#[test]
fn what_the_converter_says_is_reported() {
    let dir = tempfile::tempdir().unwrap();
    let fake = FakeConverter::default();
    let report = PandocExporter::new(Format::Epub, &fake)
        .export(
            &vault(),
            &all_notes(),
            &options(Layout::SingleFile),
            &dir.path().join("a.epub"),
        )
        .unwrap();
    assert_eq!(report.warnings, ["Samples: A warning"]);
}

#[test]
fn a_failed_conversion_writes_nothing_and_keeps_what_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("a.docx");
    fs::write(&to, "mine").unwrap();
    let fake = FakeConverter {
        fail: true,
        ..FakeConverter::default()
    };
    let error = PandocExporter::new(Format::Docx, &fake)
        .export(&vault(), &all_notes(), &options(Layout::SingleFile), &to)
        .unwrap_err();
    assert!(error.to_string().contains("Pandoc stopped: no"), "{error}");
    assert_eq!(fs::read(&to).unwrap(), b"mine");
}

fn pandoc() -> Pandoc {
    match std::env::var_os("PANDOC") {
        Some(path) => Pandoc::at(PathBuf::from(path)).unwrap(),
        None => Pandoc::find().expect("Pandoc is needed for this test"),
    }
}

fn part(file: &Path, name: &str) -> String {
    let mut zip = ZipArchive::new(fs::File::open(file).unwrap()).unwrap();
    let mut text = String::new();
    zip.by_name(name)
        .unwrap_or_else(|_| panic!("{name} in {}", file.display()))
        .read_to_string(&mut text)
        .unwrap();
    text
}

fn names(file: &Path) -> Vec<String> {
    let zip = ZipArchive::new(fs::File::open(file).unwrap()).unwrap();
    (0..zip.len())
        .map(|i| zip.name_for_index(i).unwrap().unwrap().to_string())
        .collect()
}

/// The values of `attribute` in `xml`.
fn values<'x>(xml: &'x str, attribute: &str) -> Vec<&'x str> {
    let open = format!("{attribute}=\"");
    xml.split(&open)
        .skip(1)
        .map(|rest| &rest[..rest.find('"').unwrap()])
        .collect()
}

/// Keeps the results for a look by hand if `SCHNIPSLE_PANDOC_KEEP` names a folder.
fn keep(folder: &Path) {
    if let Some(keep) = std::env::var_os("SCHNIPSLE_PANDOC_KEEP") {
        for entry in fs::read_dir(folder).unwrap() {
            let entry = entry.unwrap();
            if entry.path().is_file() {
                fs::create_dir_all(&keep).unwrap();
                fs::copy(entry.path(), Path::new(&keep).join(entry.file_name())).unwrap();
            }
        }
    }
}

#[test]
#[ignore = "needs Pandoc"]
fn pandoc_writes_docx() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let vault = tricky_vault(&dir.path().join("vault"));
    let exporter = PandocExporter::new(Format::Docx, pandoc());

    let to = out.join("tricky.docx");
    let report = exporter
        .export(&vault, &tricky_notes(), &options(Layout::SingleFile), &to)
        .unwrap();
    assert_eq!(other_warnings(&report), Vec::<&str>::new());
    let xml = part(&to, "word/document.xml");
    for text in [
        "`a`",
        "{.b}",
        "{.c}",
        "$x$",
        ":smile:",
        "{braces}",
        "::: not a div",
        "<b>kept as code</b>",
        "Its text",
    ] {
        let text = text
            .trim_matches('`')
            .replace('<', "&lt;")
            .replace('>', "&gt;");
        let notes = part(&to, "word/footnotes.xml");
        assert!(
            xml.contains(&text) || notes.contains(&text),
            "{text}: {xml}"
        );
    }
    assert!(xml.contains("{#mine}"), "{xml}");
    // A new page between the notes, and links that find their bookmarks.
    assert_eq!(xml.matches("w:type=\"page\"").count(), 1, "{xml}");
    let bookmarks: HashSet<_> = values(&xml, "w:name").into_iter().collect();
    let anchors = values(&xml, "w:anchor");
    assert_eq!(anchors.len(), 2, "{xml}");
    for anchor in anchors {
        assert!(bookmarks.contains(anchor), "{anchor}: {xml}");
    }
    // The title in the properties only, and the paper.
    assert!(!xml.contains("w:val=\"Title\""), "{xml}");
    assert!(
        part(&to, "docProps/core.xml").contains("<dc:title>Tricky</dc:title>"),
        "the title"
    );
    assert!(
        xml.contains("<w:pgSz w:w=\"11906\" w:h=\"16838\"/>"),
        "{xml}"
    );

    // A note's dates, its picture, and a file that links to another.
    let samples = self::vault();
    let bread = Input {
        notes: vec!["Recipes/bread-rolls.md".to_owned()],
        name: None,
        scores: scores(),
    };
    let to = out.join("bread.docx");
    let report = exporter
        .export(
            &samples,
            &bread,
            &Options {
                paper: Paper::Letter,
                ..options(Layout::SingleFile)
            },
            &to,
        )
        .unwrap();
    assert_eq!(other_warnings(&report), Vec::<&str>::new());
    let core = part(&to, "docProps/core.xml");
    assert!(core.contains("<dc:title>Bread rolls</dc:title>"), "{core}");
    assert!(!core.contains("1970-"), "{core}");
    assert!(
        names(&to)
            .iter()
            .any(|name| name.starts_with("word/media/")),
        "the picture"
    );
    let xml = part(&to, "word/document.xml");
    assert!(xml.contains("w:w=\"12240\""), "{xml}");
    assert!(!xml.contains("data:"), "the picture stays out of the text");

    let report = exporter
        .export(&samples, &all_notes(), &options(Layout::FilePerNote), &out)
        .unwrap();
    assert_eq!(report.files.len(), 6);
    assert_eq!(other_warnings(&report), Vec::<&str>::new());
    let tuner = out.join("Antenna tuner.docx");
    let links = part(&tuner, "word/_rels/document.xml.rels");
    assert!(links.contains("Welcome.docx"), "{links}");
    keep(&out);
}

#[test]
#[ignore = "needs Pandoc"]
fn pandoc_writes_epub() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let vault = tricky_vault(&dir.path().join("vault"));
    let exporter = PandocExporter::new(Format::Epub, pandoc());

    let to = out.join("tricky.epub");
    let report = exporter
        .export(&vault, &tricky_notes(), &options(Layout::SingleFile), &to)
        .unwrap();
    assert_eq!(other_warnings(&report), Vec::<&str>::new());
    let opf = part(&to, "EPUB/content.opf");
    assert!(opf.contains(">Tricky</dc:title>"), "{opf}");
    assert!(names(&to)[0] == "mimetype", "{:?}", names(&to));
    // Each note a chapter, and a link that reaches the other one.
    let first = part(&to, "EPUB/text/ch001.xhtml");
    let second = part(&to, "EPUB/text/ch002.xhtml");
    assert!(first.contains("{#mine}"), "{first}");
    assert!(
        first.contains("{.b}") && first.contains("{braces}"),
        "{first}"
    );
    assert!(
        first.contains("$x$") && first.contains(":smile:"),
        "{first}"
    );
    assert!(first.contains("&lt;b&gt;kept as code"), "{first}");
    assert!(first.contains("href=\"ch002.xhtml#note-andere-"), "{first}");
    assert!(second.contains("href=\"ch001.xhtml#note-gr"), "{second}");
    // Our rules for the pages of a reader come after Pandoc's stylesheet.
    let ours = first.find("max-height: 90vh").expect("our stylesheet");
    assert!(
        first.find("stylesheet1.css").is_some_and(|at| at < ours),
        "{first}"
    );

    // A note's dates and picture.
    let samples = self::vault();
    let bread = Input {
        notes: vec!["Recipes/bread-rolls.md".to_owned()],
        name: None,
        scores: scores(),
    };
    let to = out.join("bread.epub");
    let report = exporter
        .export(&samples, &bread, &options(Layout::SingleFile), &to)
        .unwrap();
    assert_eq!(other_warnings(&report), Vec::<&str>::new());
    let opf = part(&to, "EPUB/content.opf");
    assert!(opf.contains(">Bread rolls</dc:title>"), "{opf}");
    assert!(opf.contains("<dc:date"), "{opf}");
    assert!(!opf.contains("1970-"), "{opf}");
    assert!(
        names(&to)
            .iter()
            .any(|name| name.starts_with("EPUB/media/"))
    );

    let report = exporter
        .export(&samples, &all_notes(), &options(Layout::FilePerNote), &out)
        .unwrap();
    assert_eq!(report.files.len(), 6);
    assert_eq!(other_warnings(&report), Vec::<&str>::new());
    assert!(
        report
            .problems
            .iter()
            .any(|p| p.kind == ProblemKind::NoteInOtherFile)
    );
    keep(&out);
}

#[test]
#[ignore = "needs Pandoc"]
fn an_svg_stays_in_the_docx_and_a_missing_converter_is_explained() {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("vault")).unwrap();
    let vault = Vault::create(dir.path().join("vault")).unwrap();
    let svg = "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"40\" height=\"20\"><rect width=\"40\" height=\"20\"/></svg>";
    fs::create_dir_all(vault.root().join("attachments/ab")).unwrap();
    fs::write(vault.root().join("attachments/ab/plan.svg"), svg).unwrap();
    vault
        .create_note(
            "plan.md",
            "# Plan\n\n![The plan](../attachments/ab/plan.svg)\n",
        )
        .unwrap();
    let to = dir.path().join("plan.docx");
    let input = Input {
        notes: vec!["plan.md".to_owned()],
        name: None,
        scores: scores(),
    };
    let report = PandocExporter::new(Format::Docx, pandoc())
        .export(&vault, &input, &options(Layout::SingleFile), &to)
        .unwrap();
    let svg = |name: &String| Path::new(name).extension().is_some_and(|e| e == "svg");
    assert!(names(&to).iter().any(svg));
    // Without rsvg-convert Pandoc adds no PNG copy, and the report says so plainly.
    for warning in &report.warnings {
        assert!(
            warning.starts_with(
                "Plan: The SVG drawing \u{201c}The plan\u{201d} is in the document as SVG only"
            ),
            "{warning}"
        );
    }
}

#[test]
#[ignore = "needs Pandoc"]
fn pandoc_writes_contents_and_page_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let out = dir.path().join("out");
    fs::create_dir(&out).unwrap();
    let with_contents = Options {
        contents: true,
        ..options(Layout::SingleFile)
    };

    let to = out.join("samples.docx");
    let report = PandocExporter::new(Format::Docx, pandoc())
        .export(&vault(), &all_notes(), &with_contents, &to)
        .unwrap();
    assert_eq!(other_warnings(&report), Vec::<&str>::new());
    let xml = part(&to, "word/document.xml");
    assert_eq!(xml.matches(" PAGEREF _Schnipsle").count(), 6, "{xml}");
    for index in 1..=6 {
        let name = format!("w:name=\"_Schnipsle{index}\"");
        assert!(xml.contains(&name), "{name} in {xml}");
    }
    assert!(xml.contains("<w:footerReference w:type=\"default\" r:id=\"rIdSchnipsleFooter\"/>"));
    assert!(part(&to, "word/schnipsle-footer.xml").contains(" PAGE "));

    let to = out.join("samples.epub");
    let report = PandocExporter::new(Format::Epub, pandoc())
        .export(&vault(), &all_notes(), &with_contents, &to)
        .unwrap();
    assert_eq!(other_warnings(&report), Vec::<&str>::new());
    let opf = part(&to, "EPUB/content.opf");
    assert!(opf.contains("<itemref idref=\"nav\""), "{opf}");
    assert!(part(&to, "EPUB/nav.xhtml").contains(">Contents<"));
    keep(&out);
}
