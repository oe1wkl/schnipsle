// SPDX-License-Identifier: Apache-2.0
//! The real print path (ADR-0022), read back with `PDFKit`: pages, paper size, title,
//! links, and with the exporter the dates of a note (ADR-0024). Not in the test
//! harness, as it needs the main thread.

// The blocks here hold only plain getters on valid objects, one after the other.
#![allow(clippy::multiple_unsafe_ops_per_block)]

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;
    use std::io;
    use std::path::PathBuf;
    use std::rc::Rc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use objc2::rc::Retained;
    use objc2::{AnyThread as _, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::{NSData, NSDate, NSRunLoop, NSString};
    use objc2_pdf_kit::{
        PDFDisplayBox, PDFDocument, PDFDocumentCreationDateAttribute,
        PDFDocumentModificationDateAttribute, PDFDocumentTitleAttribute,
    };
    use schnipsle_apple::{PdfPage, print_pdf};
    use schnipsle_core::dates::parse_rfc3339;
    use schnipsle_core::vault::Vault;
    use schnipsle_export::{
        Exporter as _, Input, Layout, Options, Paper, PdfExporter, PrintJob, Printer,
    };

    const HTML: &str = r##"<!doctype html><html><head><meta charset="utf-8"><style>
        section + section { break-before: page }
        </style></head><body>
        <section id="a"><h1>First</h1><a href="https://example.com/x">web</a>
        <a href="#b">inner</a></section>
        <section id="b"><h1>Second</h1></section>
        </body></html>"##;

    fn page(html: &str) -> PdfPage {
        PdfPage {
            html: html.to_owned(),
            width: 595.0,
            height: 842.0,
            margin: 56.0,
            title: "Grüße & mehr".to_owned(),
        }
    }

    fn run_loop_for_a_moment() {
        NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
    }

    fn print(page: PdfPage) -> io::Result<Vec<u8>> {
        let result = Rc::new(RefCell::new(None));
        let slot = result.clone();
        print_pdf(page, move |r| *slot.borrow_mut() = Some(r));
        let start = Instant::now();
        while result.borrow().is_none() {
            assert!(start.elapsed() < Duration::from_secs(60), "no result");
            run_loop_for_a_moment();
        }
        result.borrow_mut().take().unwrap()
    }

    fn document(bytes: &[u8]) -> Retained<PDFDocument> {
        let data = NSData::with_bytes(bytes);
        // SAFETY: valid data; `None` is handled by the unwrap.
        unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }.unwrap()
    }

    fn title(document: &PDFDocument) -> String {
        // SAFETY: plain getters on a valid document.
        unsafe {
            let attributes = document.documentAttributes().unwrap();
            let title = attributes
                .objectForKey(PDFDocumentTitleAttribute.as_ref())
                .unwrap();
            title.downcast::<NSString>().unwrap().to_string()
        }
    }

    fn pages_and_paper_and_title_and_links() {
        let bytes = print(page(HTML)).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        let document = document(&bytes);
        assert_eq!(title(&document), "Grüße & mehr");
        // SAFETY: plain getters on a valid document.
        unsafe {
            assert_eq!(document.pageCount(), 2);
            let first = document.pageAtIndex(0).unwrap();
            let size = first.boundsForBox(PDFDisplayBox::MediaBox).size;
            assert!((size.width - 595.0).abs() < 1.0, "{size:?}");
            assert!((size.height - 842.0).abs() < 1.0, "{size:?}");
            assert!(first.string().unwrap().to_string().contains("First"));
            let kinds: Vec<String> = first
                .annotations()
                .iter()
                .filter_map(|annotation| annotation.action())
                .map(|action| action.r#type().to_string())
                .collect();
            assert_eq!(kinds, ["URI", "GoTo"]);
        }
    }

    fn several_jobs_one_after_the_other() {
        for _ in 0..3 {
            let bytes = print(page("<p>Short")).unwrap();
            // SAFETY: a plain getter on a valid document.
            assert_eq!(unsafe { document(&bytes).pageCount() }, 1);
        }
    }

    type Request = (PrintJob, mpsc::Sender<io::Result<Vec<u8>>>);

    /// Hands the job to the main thread, as the app shell does.
    struct MainThreadPrinter(mpsc::Sender<Request>);

    impl Printer for MainThreadPrinter {
        fn print(&self, job: PrintJob) -> io::Result<Vec<u8>> {
            let (sender, receiver) = mpsc::channel();
            self.0.send((job, sender)).unwrap();
            receiver.recv().unwrap()
        }
    }

    /// Exports `notes` of the sample vault on a thread and prints here.
    fn export(notes: &[&str]) -> Vec<u8> {
        let vault =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/export/vault");
        let input = Input {
            notes: notes.iter().map(|n| (*n).to_owned()).collect(),
            name: Some("Samples".to_owned()),
        };
        let dir = tempfile::tempdir().unwrap();
        let to = dir.path().join("out.pdf");
        let target = to.clone();
        let (requests, jobs) = mpsc::channel();
        let exporting = thread::spawn(move || {
            let options = Options {
                layout: Layout::SingleFile,
                keep_tag_line: false,
                paper: Paper::A4,
            };
            PdfExporter::new(MainThreadPrinter(requests))
                .export(&Vault::open(vault).unwrap(), &input, &options, &target)
                .unwrap();
        });
        let start = Instant::now();
        while !exporting.is_finished() {
            assert!(start.elapsed() < Duration::from_secs(60), "no result");
            if let Ok((job, answer)) = jobs.try_recv() {
                let page = PdfPage {
                    html: job.html,
                    width: job.width,
                    height: job.height,
                    margin: job.margin,
                    title: job.title,
                };
                print_pdf(page, move |result| answer.send(result).unwrap());
            }
            run_loop_for_a_moment();
        }
        exporting.join().unwrap();
        std::fs::read(&to).unwrap()
    }

    /// The dates of the note reach the file on every macOS, whatever `PDFKit` does.
    fn a_note_keeps_its_dates() {
        let bytes = export(&["welcome.md"]);
        let document = document(&bytes);
        assert_eq!(title(&document), "Welcome");
        // SAFETY: plain getters on a valid document.
        let found = unsafe {
            let attributes = document.documentAttributes().unwrap();
            let date = |key: &NSString| {
                let date = attributes.objectForKey(key.as_ref()).unwrap();
                date.downcast::<NSDate>().unwrap().timeIntervalSince1970()
            };
            (
                date(PDFDocumentCreationDateAttribute),
                date(PDFDocumentModificationDateAttribute),
            )
        };
        #[allow(clippy::cast_precision_loss)]
        let seconds = |text| parse_rfc3339(text).unwrap() as f64 / 1000.0;
        let expected = (
            seconds("2026-10-01T07:00:00Z"),
            seconds("2026-10-20T16:30:00Z"),
        );
        assert!(
            (found.0 - expected.0).abs() < 1.0 && (found.1 - expected.1).abs() < 1.0,
            "created and modified are {found:?}, not {expected:?}"
        );
    }

    fn the_samples_start_a_page_per_note() {
        let bytes = export(&[
            "welcome.md",
            "packing-list.md",
            "Radio/antenna-tuner.md",
            "Recipes/bread-rolls.md",
            "Songs/abendlied.md",
            "raw-html.md",
        ]);
        let document = document(&bytes);
        assert_eq!(title(&document), "Samples");
        // SAFETY: plain getters on a valid document.
        unsafe {
            assert!(document.pageCount() >= 6, "{}", document.pageCount());
            let text = document.string().unwrap().to_string();
            for title in [
                "Welcome",
                "Packing list for Field Day",
                "Abendlied",
                "Raw HTML",
            ] {
                assert!(text.contains(title), "{title}");
            }
        }
    }

    pub fn main() {
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        pages_and_paper_and_title_and_links();
        several_jobs_one_after_the_other();
        a_note_keeps_its_dates();
        the_samples_start_a_page_per_note();
        println!("pdf: 4 checks passed");
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    macos::main();
}
