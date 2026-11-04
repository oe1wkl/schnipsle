// SPDX-License-Identifier: Apache-2.0
//! PDF pages from an HTML page on macOS, through the print path (ADR-0022, findings
//! in `docs/spikes/pdf-print.md`).
//!
//! [`print_pdf`] loads the page into a web view that is never shown, with
//! JavaScript off, and prints it to a temporary file with no panels. Everything
//! happens in callbacks on the main thread, which must keep running; the result
//! arrives through a callback there too.

use std::cell::{Cell, OnceCell};
use std::ffi::c_void;
use std::io;
use std::ptr::NonNull;

use block2::RcBlock;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObjectProtocol, ProtocolObject};
use objc2::{
    AnyThread as _, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
};
use objc2_app_kit::{
    NSBackingStoreType, NSPrintInfo, NSPrintJobSavingURL, NSPrintOperation, NSPrintSaveJob,
    NSPrintingPaginationMode, NSWindow, NSWindowStyleMask,
};
use objc2_foundation::{NSError, NSObject, NSPoint, NSRect, NSRunLoop, NSSize, NSString, NSURL};
use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView, WKWebViewConfiguration};

/// A page to print. Lengths are in points, 1/72 of an inch.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfPage {
    /// The whole HTML page. It loads without a base URL, so nothing relative loads, and
    /// a relative link does not become a link in the PDF.
    pub html: String,
    /// The width of the paper.
    pub width: f64,
    /// The height of the paper.
    pub height: f64,
    /// The margin on all four sides.
    pub margin: f64,
    /// The PDF's title.
    pub title: String,
}

type Done = Box<dyn FnOnce(io::Result<Vec<u8>>)>;

struct State {
    page: PdfPage,
    dir: tempfile::TempDir,
    done: Cell<Option<Done>>,
    web_view: OnceCell<Retained<WKWebView>>,
    window: OnceCell<Retained<NSWindow>>,
    operation: OnceCell<Retained<NSPrintOperation>>,
    /// The job itself, as nothing else holds on to it until it has finished.
    keep: Cell<Option<Retained<PrintJob>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SchnipslePdfPrintJob"]
    #[ivars = State]
    struct PrintJob;

    unsafe impl NSObjectProtocol for PrintJob {}

    unsafe impl WKNavigationDelegate for PrintJob {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
            self.print();
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            self.finish(Err(failed(&error.localizedDescription().to_string())));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            _web_view: &WKWebView,
            _navigation: Option<&WKNavigation>,
            error: &NSError,
        ) {
            self.finish(Err(failed(&error.localizedDescription().to_string())));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn did_terminate(&self, _web_view: &WKWebView) {
            self.finish(Err(failed("the web view stopped while printing")));
        }
    }

    impl PrintJob {
        #[unsafe(method(printOperationDidRun:success:contextInfo:))]
        fn did_run(&self, _operation: &NSPrintOperation, success: Bool, _context: *mut c_void) {
            self.finish(if success.as_bool() {
                self.result()
            } else {
                Err(failed("printing did not succeed"))
            });
        }
    }
);

fn failed(why: &str) -> io::Error {
    io::Error::other(format!("Could not make the PDF: {why}"))
}

impl PrintJob {
    fn new(mtm: MainThreadMarker, state: State) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(state);
        // SAFETY: `NSObject`'s init on a fresh object.
        unsafe { msg_send![super(this), init] }
    }

    fn load(&self, mtm: MainThreadMarker) {
        let page = &self.ivars().page;
        let frame = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(page.width - 2.0 * page.margin, page.height),
        );
        // SAFETY: a plain new configuration.
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        // An exported page has no scripts; none may run if one got in.
        // SAFETY: the configuration's own preferences object.
        let preferences = unsafe { configuration.defaultWebpagePreferences() };
        // SAFETY: a plain flag on a valid object.
        unsafe { preferences.setAllowsContentJavaScript(false) };
        // SAFETY: `configuration` is valid; the frame is finite.
        let web_view = unsafe {
            WKWebView::initWithFrame_configuration(WKWebView::alloc(mtm), frame, &configuration)
        };
        // SAFETY: the web view holds its delegate weakly, and `self` lives until it is done.
        unsafe { web_view.setNavigationDelegate(Some(ProtocolObject::from_ref(self))) };
        // SAFETY: a valid string, and no base URL.
        let _ = unsafe { web_view.loadHTMLString_baseURL(&NSString::from_str(&page.html), None) };
        let _ = self.ivars().web_view.set(web_view);
    }

    /// Prints the loaded page to the temporary file; `did_run` reports back.
    fn print(&self) {
        let state = self.ivars();
        let (Some(web_view), None) = (state.web_view.get(), state.operation.get()) else {
            return;
        };
        let mtm = self.mtm();
        let page = &state.page;
        let info = NSPrintInfo::init(NSPrintInfo::alloc());
        info.setPaperSize(NSSize::new(page.width, page.height));
        info.setTopMargin(page.margin);
        info.setBottomMargin(page.margin);
        info.setLeftMargin(page.margin);
        info.setRightMargin(page.margin);
        info.setHorizontalPagination(NSPrintingPaginationMode::Fit);
        info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
        info.setHorizontallyCentered(false);
        info.setVerticallyCentered(false);
        // SAFETY: the system's constant.
        info.setJobDisposition(unsafe { NSPrintSaveJob });
        let url = file_url(&self.printed_file());
        let url: &AnyObject = url.as_ref();
        // SAFETY: the print info's own dictionary.
        let dictionary = unsafe { info.dictionary() };
        // SAFETY: the system's constant.
        let key = ProtocolObject::from_ref(unsafe { NSPrintJobSavingURL });
        // SAFETY: this key takes an NSURL, per NSPrintInfo's documentation.
        unsafe { dictionary.setObject_forKey(url, key) };

        // SAFETY: `info` is a valid print info and the page has loaded.
        let operation = unsafe { web_view.printOperationWithPrintInfo(&info) };
        operation.setShowsPrintPanel(false);
        operation.setShowsProgressPanel(false);
        operation.setJobTitle(Some(&NSString::from_str(&page.title)));
        let rect = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(page.width, page.height));
        // SAFETY: a borderless window that is never shown; `State` holds it, not closing.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: as said above.
        unsafe { window.setReleasedWhenClosed(false) };
        let delegate: &AnyObject = self.as_ref();
        // SAFETY: the selector has the signature the method asks for; `self` and the
        // window live until it was called, and the context is not read.
        unsafe {
            operation.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
                &window,
                Some(delegate),
                Some(sel!(printOperationDidRun:success:contextInfo:)),
                NonNull::<c_void>::dangling().as_ptr(),
            );
        }
        let _ = state.operation.set(operation);
        let _ = state.window.set(window);
    }

    fn printed_file(&self) -> std::path::PathBuf {
        self.ivars().dir.path().join("printed.pdf")
    }

    /// The PDF. Success of the print operation does not mean there is a file, so
    /// a missing one counts as a failure.
    fn result(&self) -> io::Result<Vec<u8>> {
        let file = self.printed_file();
        if !file.is_file() {
            return Err(failed("printing wrote no file"));
        }
        let bytes = std::fs::read(file)?;
        if bytes.starts_with(b"%PDF") {
            Ok(bytes)
        } else {
            Err(failed("printing wrote no PDF"))
        }
    }

    /// Reports `result`, the first time. Called from the callbacks of the objects
    /// the job holds, so these go only after the callback has returned.
    fn finish(&self, result: io::Result<Vec<u8>>) {
        let Some(done) = self.ivars().done.take() else {
            return;
        };
        done(result);
        let keep = Cell::new(self.ivars().keep.take());
        let release = RcBlock::new(move || drop(keep.take()));
        // SAFETY: the main run loop runs the block on this, the main thread, which
        // is the only one that may hold the job.
        unsafe { NSRunLoop::mainRunLoop().performBlock(&release) };
    }
}

fn file_url(path: &std::path::Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

/// Prints `page` to a PDF, and calls `done` with its bytes on the main thread.
/// Shows nothing and returns at once. Call on the main thread, with its run loop
/// running; on another thread `done` gets an error right away.
pub fn print_pdf(page: PdfPage, done: impl FnOnce(io::Result<Vec<u8>>) + 'static) {
    let Some(mtm) = MainThreadMarker::new() else {
        done(Err(failed("it was not started on the main thread")));
        return;
    };
    let dir = match tempfile::tempdir() {
        Ok(dir) => dir,
        Err(error) => {
            done(Err(error));
            return;
        }
    };
    let job = PrintJob::new(
        mtm,
        State {
            page,
            dir,
            done: Cell::new(Some(Box::new(done))),
            web_view: OnceCell::new(),
            window: OnceCell::new(),
            operation: OnceCell::new(),
            keep: Cell::new(None),
        },
    );
    job.ivars().keep.set(Some(job.clone()));
    job.load(mtm);
}
