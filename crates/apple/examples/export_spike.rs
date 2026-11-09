// SPDX-License-Identifier: Apache-2.0
//! Spike for M6, findings in `docs/spikes/abc-export.md`: how a score as SVG
//! fares in every export format. Exports the notes of a vault as one document
//! in HTML, PDF, ODT and, with Pandoc, DOCX and EPUB.
//!
//! ```sh
//! cargo run -p schnipsle-apple --example export_spike -- vault out-dir note.md … [block.abc=tune.svg …]
//! ```
//!
//! An argument `block.abc=tune.svg` passes the SVG as the engraved score of an
//! `abc` block whose content is that of `block.abc` (ADR-0033).

#[cfg(target_os = "macos")]
mod spike {
    use std::cell::RefCell;
    use std::io;
    use std::path::Path;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::{NSDate, NSRunLoop};
    use schnipsle_apple::{PdfPage, print_pdf};
    use schnipsle_core::vault::Vault;
    use schnipsle_export::{
        Exporter, Format, HtmlExporter, Input, Layout, OdtExporter, Options, Pandoc,
        PandocExporter, Paper, PdfExporter, PrintJob, Printer, Score,
    };

    /// Prints on this, the main thread, running its run loop meanwhile.
    struct Here;

    impl Printer for Here {
        fn print(&self, job: PrintJob) -> io::Result<Vec<u8>> {
            let result = Rc::new(RefCell::new(None));
            let slot = result.clone();
            let page = PdfPage {
                html: job.html,
                width: job.width,
                height: job.height,
                margin: job.margin,
                title: job.title,
            };
            print_pdf(page, move |r| *slot.borrow_mut() = Some(r));
            let start = Instant::now();
            while result.borrow().is_none() {
                assert!(start.elapsed() < Duration::from_secs(60), "no result");
                NSRunLoop::currentRunLoop()
                    .runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
            }
            result.borrow_mut().take().unwrap()
        }
    }

    pub fn main() {
        let mut args = std::env::args().skip(1);
        let vault = Vault::open(args.next().expect("vault")).expect("open vault");
        let out = args.next().expect("out dir");
        let (pairs, notes): (Vec<String>, Vec<String>) = args.partition(|a| a.contains('='));
        let scores = pairs
            .iter()
            .map(|pair| {
                let (abc, svg) = pair.split_once('=').unwrap();
                Score {
                    source: std::fs::read_to_string(abc).expect("abc"),
                    transpose: 0,
                    tunes: vec![std::fs::read_to_string(svg).expect("svg")],
                }
            })
            .collect();
        let input = Input {
            notes,
            name: Some("Scores".to_owned()),
            scores,
        };
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();

        let options = Options {
            layout: Layout::SingleFile,
            keep_tag_line: false,
            abc_source: false,
            paper: Paper::A4,
            contents: false,
            page_numbers: false,
        };
        let mut exporters: Vec<Box<dyn Exporter>> = vec![
            Box::new(HtmlExporter),
            Box::new(OdtExporter),
            Box::new(PdfExporter::new(Here)),
        ];
        if let Some(pandoc) = Pandoc::find() {
            exporters.push(Box::new(PandocExporter::new(Format::Docx, pandoc.clone())));
            exporters.push(Box::new(PandocExporter::new(Format::Epub, pandoc)));
        }
        for exporter in exporters {
            let to = Path::new(&out).join(format!("scores.{}", exporter.format().extension()));
            let report = exporter
                .export(&vault, &input, &options, &to)
                .expect("export");
            println!(
                "{}: {:?} {:?}",
                to.display(),
                report.warnings,
                report.problems
            );
        }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    spike::main();
}

#[cfg(not(target_os = "macos"))]
fn main() {}
