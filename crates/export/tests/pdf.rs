// SPDX-License-Identifier: Apache-2.0
//! The PDF export of the sample vault, with a stand-in for the printer: what it
//! is given against the golden files in `tests/fixtures/export/pdf/expected/`.
//! `crates/apple/tests/pdf.rs` prints such a page for real. Run with
//! `SCHNIPSLE_BLESS=1` to write `expected/` after a deliberate change.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use schnipsle_core::vault::Vault;
use schnipsle_export::{
    Exporter, Format, Input, Layout, Options, Paper, PdfExporter, PrintJob, Printer, ProblemKind,
    Report,
};

const NOTES: [&str; 6] = [
    "welcome.md",
    "packing-list.md",
    "Radio/antenna-tuner.md",
    "Recipes/bread-rolls.md",
    "Songs/abendlied.md",
    "raw-html.md",
];

#[derive(Default)]
struct FakePrinter {
    jobs: RefCell<Vec<PrintJob>>,
}

impl Printer for &FakePrinter {
    fn print(&self, job: PrintJob) -> io::Result<Vec<u8>> {
        self.jobs.borrow_mut().push(job);
        Ok(b"%PDF-fake".to_vec())
    }
}

/// Prints what a Mac prints: a file with an Info dictionary holding the time of printing.
struct DatedPrinter;

const PRINTED: &str = "%PDF-1.3\n9 0 obj\n<< /Title (x) /CreationDate (D:20261104141857Z00'00') /ModDate (D:20261104141857Z00'00') >>\nendobj\ntrailer\n<< /Info 9 0 R >>\n";

impl Printer for DatedPrinter {
    fn print(&self, _job: PrintJob) -> io::Result<Vec<u8>> {
        Ok(PRINTED.as_bytes().to_vec())
    }
}

struct FailingPrinter;

impl Printer for FailingPrinter {
    fn print(&self, _job: PrintJob) -> io::Result<Vec<u8>> {
        Err(io::Error::other("no printing here"))
    }
}

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/export")
}

fn vault() -> Vault {
    Vault::open(fixtures().join("vault")).unwrap()
}

fn all_notes() -> Input {
    Input {
        notes: NOTES.iter().map(|p| (*p).to_owned()).collect(),
        name: Some("Samples".to_owned()),
    }
}

fn options(layout: Layout) -> Options {
    Options {
        layout,
        keep_tag_line: false,
        paper: Paper::A4,
    }
}

fn pretty(report: &Report) -> Vec<u8> {
    let mut json = serde_json::to_vec_pretty(report).unwrap();
    json.push(b'\n');
    json
}

/// The golden form of a job: its page, and the rest in a small text file.
fn add_job(actual: &mut BTreeMap<String, Vec<u8>>, name: &str, job: &PrintJob) {
    actual.insert(format!("{name}.html"), job.html.clone().into_bytes());
    let text = format!(
        "title: {}\npaper: {:.0} x {:.0}\nmargin: {:.0}\n",
        job.title, job.width, job.height, job.margin
    );
    actual.insert(format!("{name}.job.txt"), text.into_bytes());
}

#[test]
fn golden_files() {
    let vault = vault();
    let mut actual = BTreeMap::new();

    let printer = FakePrinter::default();
    let exporter = PdfExporter::new(&printer);
    assert_eq!(exporter.format(), Format::Pdf);
    let dir = tempfile::tempdir().unwrap();
    let report = exporter
        .export(
            &vault,
            &all_notes(),
            &options(Layout::SingleFile),
            &dir.path().join("all.pdf"),
        )
        .unwrap();
    assert_eq!(fs::read(dir.path().join("all.pdf")).unwrap(), b"%PDF-fake");
    assert_eq!(report.files, ["all.pdf"]);
    let jobs = printer.jobs.borrow();
    assert_eq!(jobs.len(), 1);
    add_job(&mut actual, "one-document/all", &jobs[0]);
    actual.insert("one-document/report.json".to_owned(), pretty(&report));

    let printer = FakePrinter::default();
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("out");
    let report = PdfExporter::new(&printer)
        .export(&vault, &all_notes(), &options(Layout::FilePerNote), &to)
        .unwrap();
    let jobs = printer.jobs.borrow();
    assert_eq!(report.files.len(), 6);
    for (name, job) in report.files.iter().zip(jobs.iter()) {
        assert_eq!(fs::read(to.join(name)).unwrap(), b"%PDF-fake");
        let stem = Path::new(name).file_stem().unwrap().to_string_lossy();
        add_job(&mut actual, &format!("file-per-note/{stem}"), job);
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
    let expected_dir = fixtures().join("pdf/expected");
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

fn kinds(report: &Report) -> Vec<ProblemKind> {
    report.problems.iter().map(|p| p.kind).collect()
}

#[test]
fn nothing_loads_from_the_web_and_attachments_stay_behind() {
    let printer = FakePrinter::default();
    let dir = tempfile::tempdir().unwrap();
    let input = Input {
        notes: vec!["Radio/antenna-tuner.md".to_owned()],
        name: None,
    };
    let report = PdfExporter::new(&printer)
        .export(
            &vault(),
            &input,
            &options(Layout::SingleFile),
            &dir.path().join("a.pdf"),
        )
        .unwrap();
    let found = kinds(&report);
    assert!(found.contains(&ProblemKind::WebPictureAsLink));
    assert!(found.contains(&ProblemKind::AttachmentNotIncluded));
    let jobs = printer.jobs.borrow();
    assert!(!jobs[0].html.contains("<img src=\"http"));
    assert!(!jobs[0].html.contains("attachments/"));
}

#[test]
fn a_document_of_several_notes_has_the_title_of_the_list() {
    let printer = FakePrinter::default();
    let dir = tempfile::tempdir().unwrap();
    PdfExporter::new(&printer)
        .export(
            &vault(),
            &all_notes(),
            &options(Layout::SingleFile),
            &dir.path().join("a.pdf"),
        )
        .unwrap();
    let jobs = printer.jobs.borrow();
    assert_eq!(jobs[0].title, "Samples");
    assert!((jobs[0].width - 595.3).abs() < 0.1);
}

#[test]
fn letter_paper_and_a_folder_with_a_free_name() {
    let printer = FakePrinter::default();
    let dir = tempfile::tempdir().unwrap();
    fs::write(dir.path().join("Samples.pdf"), "mine").unwrap();
    let report = PdfExporter::new(&printer)
        .export(
            &vault(),
            &all_notes(),
            &Options {
                layout: Layout::Folder,
                keep_tag_line: false,
                paper: Paper::Letter,
            },
            dir.path(),
        )
        .unwrap();
    assert_eq!(report.files, ["Samples-2.pdf"]);
    assert_eq!(fs::read(dir.path().join("Samples.pdf")).unwrap(), b"mine");
    let jobs = printer.jobs.borrow();
    assert_eq!((jobs[0].width, jobs[0].height), (612.0, 792.0));
}

#[test]
fn links_to_notes_in_other_pdfs_become_text_and_are_reported() {
    let printer = FakePrinter::default();
    let dir = tempfile::tempdir().unwrap();
    let report = PdfExporter::new(&printer)
        .export(
            &vault(),
            &all_notes(),
            &options(Layout::FilePerNote),
            &dir.path().join("out"),
        )
        .unwrap();
    assert!(kinds(&report).contains(&ProblemKind::NoteInOtherFile));
    let jobs = printer.jobs.borrow();
    let tuner = jobs.iter().find(|j| j.title == "Antenna tuner").unwrap();
    assert!(!tuner.html.contains("href=\"Welcome"));
    assert!(!tuner.html.contains(".pdf"));
}

#[test]
fn a_single_note_gets_its_dates_and_several_keep_the_time_of_printing() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("a.pdf");
    let exporter = PdfExporter::new(DatedPrinter);
    let input = Input {
        notes: vec!["welcome.md".to_owned()],
        name: None,
    };
    exporter
        .export(&vault(), &input, &options(Layout::SingleFile), &to)
        .unwrap();
    let pdf = fs::read_to_string(&to).unwrap();
    assert!(
        pdf.contains("/CreationDate (D:20261001070000+00'00')"),
        "{pdf}"
    );
    assert!(pdf.contains("/ModDate (D:20261020163000+00'00')"), "{pdf}");

    exporter
        .export(&vault(), &all_notes(), &options(Layout::SingleFile), &to)
        .unwrap();
    assert_eq!(fs::read_to_string(&to).unwrap(), PRINTED);
}

#[test]
fn a_failed_print_writes_nothing_and_keeps_what_is_there() {
    let dir = tempfile::tempdir().unwrap();
    let to = dir.path().join("a.pdf");
    fs::write(&to, "mine").unwrap();
    let result = PdfExporter::new(FailingPrinter).export(
        &vault(),
        &all_notes(),
        &options(Layout::SingleFile),
        &to,
    );
    assert!(result.is_err());
    assert_eq!(fs::read(&to).unwrap(), b"mine");
}
