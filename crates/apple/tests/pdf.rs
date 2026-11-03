// SPDX-License-Identifier: Apache-2.0
//! The real print path (ADR-0022) with a small page, read back with `PDFKit`: pages,
//! paper size, title, dates and links. Not in the test harness, as it needs the
//! main thread.

// The blocks here hold only plain getters on valid objects, one after the other.
#![allow(clippy::multiple_unsafe_ops_per_block)]

#[cfg(target_os = "macos")]
mod macos {
    use std::cell::RefCell;
    use std::rc::Rc;
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
            created_ms: Some(1_767_225_600_000),
            modified_ms: Some(1_770_000_000_000),
        }
    }

    fn print(page: PdfPage) -> std::io::Result<Vec<u8>> {
        let result = Rc::new(RefCell::new(None));
        let slot = result.clone();
        print_pdf(page, move |r| *slot.borrow_mut() = Some(r));
        let start = Instant::now();
        while result.borrow().is_none() {
            assert!(start.elapsed() < Duration::from_secs(60), "no result");
            NSRunLoop::currentRunLoop().runUntilDate(&NSDate::dateWithTimeIntervalSinceNow(0.02));
        }
        result.borrow_mut().take().unwrap()
    }

    fn document(bytes: &[u8]) -> Retained<PDFDocument> {
        let data = NSData::with_bytes(bytes);
        // SAFETY: valid data; `None` is handled by the caller's unwrap.
        unsafe { PDFDocument::initWithData(PDFDocument::alloc(), &data) }.unwrap()
    }

    fn pages_and_paper_and_metadata() {
        let bytes = print(page(HTML)).unwrap();
        assert!(bytes.starts_with(b"%PDF"));
        let document = document(&bytes);
        // SAFETY: plain getters on a valid document.
        unsafe {
            assert_eq!(document.pageCount(), 2);
            let first = document.pageAtIndex(0).unwrap();
            let size = first.boundsForBox(PDFDisplayBox::MediaBox).size;
            assert!((size.width - 595.0).abs() < 1.0, "{size:?}");
            assert!((size.height - 842.0).abs() < 1.0, "{size:?}");
            assert!(first.string().unwrap().to_string().contains("First"));

            let attributes = document.documentAttributes().unwrap();
            let get = |key: &NSString| attributes.objectForKey(key.as_ref()).unwrap();
            let title = get(PDFDocumentTitleAttribute);
            assert_eq!(
                title.downcast::<NSString>().unwrap().to_string(),
                "Grüße & mehr"
            );
            // Each date by itself in a failure: macOS versions differ in what they keep.
            let date = |key: &NSString| {
                let date = get(key).downcast::<NSDate>().unwrap();
                date.timeIntervalSince1970()
            };
            let found = (
                date(PDFDocumentCreationDateAttribute),
                date(PDFDocumentModificationDateAttribute),
            );
            let now = NSDate::now().timeIntervalSince1970();
            assert!(
                (found.0 - 1_767_225_600.0).abs() < 1.0 && (found.1 - 1_770_000_000.0).abs() < 1.0,
                "created and modified are {found:?}, printed at {now}"
            );

            // The web link and the link to the anchor stay.
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

    /// The sample notes as the exporter prints them: a page each, the wide ones too.
    fn the_samples_start_a_page_per_note() {
        let golden = concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/fixtures/export/pdf/expected/one-document/all.html"
        );
        let html = std::fs::read_to_string(golden).unwrap();
        let bytes = print(page(&html)).unwrap();
        let document = document(&bytes);
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

    /// TEMPORARY, to learn what macOS 26 does with the dates; prints and asserts nothing.
    fn report_what_this_macos_keeps() {
        fn raw(bytes: &[u8]) -> Vec<String> {
            let text = String::from_utf8_lossy(bytes);
            [
                "/CreationDate",
                "/ModDate",
                "/Title",
                "/Info",
                "/ObjStm",
                "/Producer",
            ]
            .iter()
            .map(|key| {
                let count = text.matches(key).count();
                let first = text.find(key).map_or(String::new(), |at| {
                    text[at..].chars().take(50).collect::<String>()
                });
                format!("{key} x{count} {first:?}")
            })
            .collect()
        }
        fn dates(document: &PDFDocument) -> (Option<f64>, Option<f64>) {
            // SAFETY: plain getters on a valid document.
            unsafe {
                let attributes = document.documentAttributes().unwrap();
                let get = |key: &NSString| {
                    attributes
                        .objectForKey(key.as_ref())
                        .and_then(|o| o.downcast::<NSDate>().ok())
                        .map(|d| d.timeIntervalSince1970())
                };
                (
                    get(PDFDocumentCreationDateAttribute),
                    get(PDFDocumentModificationDateAttribute),
                )
            }
        }
        let bytes = print(page(HTML)).unwrap();
        println!("DIAG header {:?}", String::from_utf8_lossy(&bytes[..8]));
        println!("DIAG now {}", NSDate::now().timeIntervalSince1970());
        println!("DIAG print_pdf raw {:#?}", raw(&bytes));
        println!("DIAG print_pdf read {:?}", dates(&document(&bytes)));

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("in.pdf");
        std::fs::write(&file, &bytes).unwrap();
        let url =
            objc2_foundation::NSURL::fileURLWithPath(&NSString::from_str(&file.to_string_lossy()));
        // SAFETY: valid objects; attributes of the types PDFKit documents.
        unsafe {
            let from_file = PDFDocument::initWithURL(PDFDocument::alloc(), &url).unwrap();
            let attributes = objc2_foundation::NSMutableDictionary::<
                objc2::runtime::AnyObject,
                objc2::runtime::AnyObject,
            >::new();
            for (key, seconds) in [
                (PDFDocumentCreationDateAttribute, 1_767_225_600.0),
                (PDFDocumentModificationDateAttribute, 1_770_000_000.0),
            ] {
                let date = NSDate::dateWithTimeIntervalSince1970(seconds);
                let value: &objc2::runtime::AnyObject = date.as_ref();
                attributes.setObject_forKey(value, objc2::runtime::ProtocolObject::from_ref(key));
            }
            from_file.setDocumentAttributes(Some(&attributes));
            println!("DIAG right after setting {:?}", dates(&from_file));

            let data = from_file.dataRepresentation().unwrap().to_vec();
            println!("DIAG dataRepresentation raw {:#?}", raw(&data));
            println!("DIAG dataRepresentation read {:?}", dates(&document(&data)));

            let out = dir.path().join("out.pdf");
            let out_url = objc2_foundation::NSURL::fileURLWithPath(&NSString::from_str(
                &out.to_string_lossy(),
            ));
            println!("DIAG writeToURL ok {}", from_file.writeToURL(&out_url));
            let written = std::fs::read(&out).unwrap();
            println!("DIAG writeToURL raw {:#?}", raw(&written));
            println!("DIAG writeToURL read {:?}", dates(&document(&written)));

            // A document opened from the rewritten file, set and written once more.
            let again = PDFDocument::initWithURL(PDFDocument::alloc(), &out_url).unwrap();
            again.setDocumentAttributes(Some(&attributes));
            let data = again.dataRepresentation().unwrap().to_vec();
            println!("DIAG second round read {:?}", dates(&document(&data)));
        }
    }

    pub fn main() {
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();
        report_what_this_macos_keeps();
        pages_and_paper_and_metadata();
        several_jobs_one_after_the_other();
        the_samples_start_a_page_per_note();
        println!("pdf: 3 checks passed");
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    macos::main();
}
