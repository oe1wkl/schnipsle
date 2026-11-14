// SPDX-License-Identifier: Apache-2.0
//! PDF pages from an HTML page on iOS (spike, `docs/spikes/ios-pdf.md`).
//!
//! iOS has no `NSPrintOperation`. [`print_pdf`] loads the page into a web view
//! that is never shown, with JavaScript off, and draws the pages of its print
//! formatter through a `UIPrintPageRenderer` into a PDF graphics context. As on
//! macOS, everything happens on the main thread, which must keep running; the
//! result arrives through a callback there.

use std::cell::{Cell, OnceCell};
use std::io;

use block2::RcBlock;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{AnyObject, NSObjectProtocol, ProtocolObject};
use objc2::{
    AnyThread as _, DefinedClass, MainThreadMarker, MainThreadOnly, Message as _, define_class,
    extern_class, extern_methods, msg_send,
};
use objc2_core_foundation::{CGPoint, CGRect, CGSize};
use objc2_foundation::{
    NSData, NSDictionary, NSError, NSMutableData, NSObject, NSRange, NSRunLoop, NSString, NSURL,
};
use objc2_ui_kit::{
    UIGraphicsBeginPDFContextToData, UIGraphicsBeginPDFPage, UIGraphicsEndPDFContext,
    UIGraphicsGetPDFContextBounds, UIGraphicsPDFRenderer, UIGraphicsPDFRendererContext,
    UIGraphicsPDFRendererFormat, UIPrintPageRenderer, UIResponder, UIView,
};
use objc2_web_kit::{
    WKNavigation, WKNavigationDelegate, WKPDFConfiguration, WKWebViewConfiguration,
    WKWebpagePreferences,
};

/// How the spike draws the pages.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Method {
    /// `UIPrintPageRenderer` into `UIGraphicsBeginPDFContextToData`.
    Context,
    /// `UIPrintPageRenderer` into `UIGraphicsPDFRenderer`.
    Renderer,
    /// `WKWebView.createPDF` of the whole page.
    CreatePdf,
}

/// A page to print. Lengths are in points, 1/72 of an inch.
#[derive(Debug, Clone, PartialEq)]
pub struct PdfPage {
    /// The whole HTML page. It loads without a base URL, so nothing relative loads.
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

extern_class!(
    /// `WKWebView` on iOS, a `UIView`. `objc2-web-kit` binds it on macOS only.
    #[unsafe(super(UIView, UIResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "WKWebView"]
    #[derive(Debug, PartialEq, Eq, Hash)]
    struct WebView;
);

impl WebView {
    extern_methods!(
        #[unsafe(method(initWithFrame:configuration:))]
        #[unsafe(method_family = init)]
        fn init_with_frame(
            this: Allocated<Self>,
            frame: CGRect,
            configuration: &WKWebViewConfiguration,
        ) -> Retained<Self>;

        #[unsafe(method(setNavigationDelegate:))]
        #[unsafe(method_family = none)]
        fn set_navigation_delegate(
            &self,
            delegate: Option<&ProtocolObject<dyn WKNavigationDelegate>>,
        );

        #[unsafe(method(loadHTMLString:baseURL:))]
        #[unsafe(method_family = none)]
        fn load_html(
            &self,
            html: &NSString,
            base: Option<&NSURL>,
        ) -> Option<Retained<WKNavigation>>;

        #[unsafe(method(createPDFWithConfiguration:completionHandler:))]
        #[unsafe(method_family = none)]
        fn create_pdf(
            &self,
            configuration: Option<&WKPDFConfiguration>,
            handler: &block2::DynBlock<dyn Fn(*mut NSData, *mut NSError)>,
        );
    );
}

/// The paper and the part of it to print on, as the renderer's properties are
/// read-only and a subclass is the way to set them.
struct Paper {
    paper: CGRect,
    printable: CGRect,
}

define_class!(
    #[unsafe(super(UIPrintPageRenderer, NSObject))]
    #[name = "SchnipslePdfPageRenderer"]
    #[ivars = Paper]
    struct PageRenderer;

    unsafe impl NSObjectProtocol for PageRenderer {}

    impl PageRenderer {
        #[unsafe(method(paperRect))]
        fn paper_rect(&self) -> CGRect {
            self.ivars().paper
        }

        #[unsafe(method(printableRect))]
        fn printable_rect(&self) -> CGRect {
            self.ivars().printable
        }
    }
);

impl PageRenderer {
    fn new(paper: Paper) -> Retained<Self> {
        let this = Self::alloc().set_ivars(paper);
        // SAFETY: `UIPrintPageRenderer`'s init on a fresh object.
        unsafe { msg_send![super(this), init] }
    }
}

type Done = Box<dyn FnOnce(io::Result<Vec<u8>>)>;

struct State {
    page: PdfPage,
    method: Method,
    done: Cell<Option<Done>>,
    web_view: OnceCell<Retained<WebView>>,
    /// The job itself, as nothing else holds on to it until it has finished.
    keep: Cell<Option<Retained<PrintJob>>>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "SchnipslePdfPrintJobIos"]
    #[ivars = State]
    struct PrintJob;

    unsafe impl NSObjectProtocol for PrintJob {}

    unsafe impl WKNavigationDelegate for PrintJob {
        #[unsafe(method(webView:didFinishNavigation:))]
        fn did_finish(&self, _web_view: &AnyObject, _navigation: Option<&AnyObject>) {
            if self.ivars().method == Method::CreatePdf {
                self.create_pdf();
                return;
            }
            let result = self.print();
            self.finish(result);
        }

        #[unsafe(method(webView:didFailNavigation:withError:))]
        fn did_fail(
            &self,
            _web_view: &AnyObject,
            _navigation: Option<&AnyObject>,
            error: &NSError,
        ) {
            self.finish(Err(failed(&error.localizedDescription().to_string())));
        }

        #[unsafe(method(webView:didFailProvisionalNavigation:withError:))]
        fn did_fail_provisional(
            &self,
            _web_view: &AnyObject,
            _navigation: Option<&AnyObject>,
            error: &NSError,
        ) {
            self.finish(Err(failed(&error.localizedDescription().to_string())));
        }

        #[unsafe(method(webViewWebContentProcessDidTerminate:))]
        fn did_terminate(&self, _web_view: &AnyObject) {
            self.finish(Err(failed("the web view stopped while printing")));
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
        let frame = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(page.width - 2.0 * page.margin, page.height),
        );
        // SAFETY: a plain new configuration.
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
        // An exported page has no scripts; none may run if one got in.
        // SAFETY: the configuration's own preferences object.
        let preferences: Retained<WKWebpagePreferences> =
            unsafe { configuration.defaultWebpagePreferences() };
        // SAFETY: a plain flag on a valid object.
        unsafe { preferences.setAllowsContentJavaScript(false) };
        let web_view = WebView::init_with_frame(WebView::alloc(mtm), frame, &configuration);
        web_view.set_navigation_delegate(Some(ProtocolObject::from_ref(self)));
        let _ = web_view.load_html(&NSString::from_str(&page.html), None);
        let _ = self.ivars().web_view.set(web_view);
    }

    /// Draws the pages of the loaded page into a PDF.
    fn print(&self) -> io::Result<Vec<u8>> {
        let state = self.ivars();
        let Some(web_view) = state.web_view.get() else {
            return Err(failed("there was no web view"));
        };
        let page = &state.page;
        let paper = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(page.width, page.height));
        let printable = CGRect::new(
            CGPoint::new(page.margin, page.margin),
            CGSize::new(
                page.width - 2.0 * page.margin,
                page.height - 2.0 * page.margin,
            ),
        );
        let renderer = PageRenderer::new(Paper { paper, printable });
        let formatter = web_view.viewPrintFormatter();
        renderer.addPrintFormatter_startingAtPageAtIndex(&formatter, 0);
        let pages = renderer.numberOfPages();
        if pages <= 0 {
            return Err(failed("the page has no pages to print"));
        }
        if state.method == Method::Renderer {
            let format = UIGraphicsPDFRendererFormat::new();
            let pdf = UIGraphicsPDFRenderer::initWithBounds_format(
                UIGraphicsPDFRenderer::alloc(),
                paper,
                &format,
            );
            #[allow(clippy::cast_sign_loss)] // checked above
            let range = NSRange::new(0, pages as usize);
            let actions = RcBlock::new(
                move |context: std::ptr::NonNull<UIGraphicsPDFRendererContext>| {
                    // SAFETY: the renderer passes a valid context for the time of the call.
                    let context = unsafe { context.as_ref() };
                    renderer.prepareForDrawingPages(range);
                    let bounds = context.pdfContextBounds();
                    for index in 0..pages {
                        context.beginPage();
                        renderer.drawPageAtIndex_inRect(index, bounds);
                    }
                },
            );
            // SAFETY: a valid block, called before the method returns.
            let data = unsafe { pdf.PDFDataWithActions(RcBlock::as_ptr(&actions)) };
            return Ok(data.to_vec());
        }
        let data = NSMutableData::new();
        let title = NSString::from_str(&page.title);
        let key = NSString::from_str("Title");
        let info: Retained<NSDictionary<NSString, AnyObject>> =
            NSDictionary::from_slices(&[&*key], &[title.as_ref()]);
        // SAFETY: only the generic types are forgotten; the keys stay strings.
        let info = unsafe { info.cast_unchecked::<AnyObject, AnyObject>() };
        // SAFETY: the dictionary holds a string for a key of the PDF's information,
        // as the function asks.
        unsafe { UIGraphicsBeginPDFContextToData(&data, paper, Some(info)) };
        #[allow(clippy::cast_sign_loss)] // checked above
        renderer.prepareForDrawingPages(NSRange::new(0, pages as usize));
        let bounds = UIGraphicsGetPDFContextBounds();
        for index in 0..pages {
            UIGraphicsBeginPDFPage();
            renderer.drawPageAtIndex_inRect(index, bounds);
        }
        UIGraphicsEndPDFContext();
        let bytes = data.to_vec();
        if bytes.starts_with(b"%PDF") {
            Ok(bytes)
        } else {
            Err(failed("drawing wrote no PDF"))
        }
    }

    /// `createPDF` of the whole page, reported when it is done.
    fn create_pdf(&self) {
        let Some(web_view) = self.ivars().web_view.get() else {
            return;
        };
        // SAFETY: a plain new configuration; its rect is null, the whole page.
        let configuration = unsafe { WKPDFConfiguration::new(self.mtm()) };
        let this = self.retain();
        let handler = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
            // SAFETY: WebKit passes a valid object or null.
            let data = unsafe { data.as_ref() };
            // SAFETY: as above.
            let error = unsafe { error.as_ref() };
            this.finish(match (data, error) {
                (Some(data), _) => Ok(data.to_vec()),
                (None, Some(error)) => Err(failed(&error.localizedDescription().to_string())),
                (None, None) => Err(failed("createPDF gave nothing")),
            });
        });
        web_view.create_pdf(Some(&configuration), &handler);
    }

    /// Reports `result`, the first time. Called from the web view's callbacks,
    /// so the job goes only after the callback has returned.
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

/// Prints `page` to a PDF, and calls `done` with its bytes on the main thread.
/// Shows nothing and returns at once. Call on the main thread, with its run loop
/// running; on another thread `done` gets an error right away.
pub fn print_pdf(page: PdfPage, done: impl FnOnce(io::Result<Vec<u8>>) + 'static) {
    print_pdf_with(page, Method::Context, done);
}

/// [`print_pdf`] by `method` (spike).
pub fn print_pdf_with(
    page: PdfPage,
    method: Method,
    done: impl FnOnce(io::Result<Vec<u8>>) + 'static,
) {
    let Some(mtm) = MainThreadMarker::new() else {
        done(Err(failed("it was not started on the main thread")));
        return;
    };
    let job = PrintJob::new(
        mtm,
        State {
            page,
            method,
            done: Cell::new(Some(Box::new(done))),
            web_view: OnceCell::new(),
            keep: Cell::new(None),
        },
    );
    job.ivars().keep.set(Some(job.clone()));
    job.load(mtm);
}
