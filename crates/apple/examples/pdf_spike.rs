// SPDX-License-Identifier: Apache-2.0
//! Spike for M5 package 3, findings in `docs/spikes/pdf-print.md`: how to turn
//! an exported HTML page into a PDF with page breaks, a paper size and margins.
//!
//! ```sh
//! cargo run -p schnipsle-apple --example pdf_spike -- in.html out.pdf --method modal --paper a4 --margin 20
//! ```
//!
//! `--method` is `create` (`WKWebView.createPDF`), `run` (`NSPrintOperation.runOperation`)
//! or `modal` (`runOperationModalForWindow:…`).

#[cfg(target_os = "macos")]
mod spike {
    use std::cell::Cell;
    use std::ffi::c_void;
    use std::ptr::NonNull;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, Bool, NSObjectProtocol};
    use objc2::{
        AnyThread as _, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send, sel,
    };
    use objc2_app_kit::{
        NSApplication, NSApplicationActivationPolicy, NSBackingStoreType, NSPrintInfo,
        NSPrintJobSavingURL, NSPrintOperation, NSPrintSaveJob, NSPrintingPaginationMode, NSWindow,
        NSWindowStyleMask,
    };
    use objc2_foundation::{
        NSData, NSDate, NSDefaultRunLoopMode, NSError, NSObject, NSPoint, NSRect, NSRunLoop,
        NSSize, NSString, NSURL,
    };
    use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView, WKWebViewConfiguration};

    const MM: f64 = 72.0 / 25.4;

    #[derive(Default)]
    struct Flags {
        loaded: Cell<bool>,
        printed: Cell<Option<bool>>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SchnipslePdfSpikeDelegate"]
        #[ivars = Flags]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl WKNavigationDelegate for Delegate {
            #[unsafe(method(webView:didFinishNavigation:))]
            fn did_finish(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
                self.ivars().loaded.set(true);
            }
        }

        impl Delegate {
            #[unsafe(method(printOperationDidRun:success:contextInfo:))]
            fn did_run(&self, _operation: &NSPrintOperation, success: Bool, _context: *mut c_void) {
                self.ivars().printed.set(Some(success.as_bool()));
            }
        }
    );

    /// Runs the main run loop until `done`, at most 30 s.
    fn pump(done: impl Fn() -> bool) -> bool {
        let start = Instant::now();
        while !done() {
            if start.elapsed() > Duration::from_secs(30) {
                return false;
            }
            let until = NSDate::dateWithTimeIntervalSinceNow(0.02);
            // SAFETY: the mode is the system's constant, for the current thread's loop.
            let mode = unsafe { NSDefaultRunLoopMode };
            NSRunLoop::currentRunLoop().runMode_beforeDate(mode, &until);
        }
        true
    }

    struct Args {
        input: String,
        output: String,
        method: String,
        paper: (f64, f64),
        margin: f64,
        fit: bool,
        set_frame: bool,
        content_view: bool,
    }

    fn args() -> Args {
        let mut list = std::env::args().skip(1);
        let input = list.next().expect("input html");
        let output = list.next().expect("output pdf");
        let mut parsed = Args {
            input,
            output,
            method: "modal".to_owned(),
            paper: (210.0 * MM, 297.0 * MM),
            margin: 20.0 * MM,
            fit: true,
            set_frame: true,
            content_view: true,
        };
        while let Some(flag) = list.next() {
            let value = list.next().expect("value");
            match flag.as_str() {
                "--method" => parsed.method = value,
                "--paper" if value == "letter" => parsed.paper = (612.0, 792.0),
                "--paper" => {}
                "--margin" => parsed.margin = value.parse::<f64>().expect("mm") * MM,
                "--fit" => parsed.fit = value == "yes",
                "--frame" => parsed.set_frame = value == "yes",
                "--content-view" => parsed.content_view = value == "yes",
                other => panic!("unknown {other}"),
            }
        }
        parsed
    }

    pub fn main() {
        let args = args();
        let html = std::fs::read_to_string(&args.input).expect("read input");
        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();

        let width = args.paper.0 - 2.0 * args.margin;
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(width, args.paper.1));
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
        let delegate: Retained<Delegate> = {
            let this = Delegate::alloc(mtm).set_ivars(Flags::default());
            // SAFETY: `NSObject`'s init on a fresh object.
            unsafe { msg_send![super(this), init] }
        };
        let protocol = objc2::runtime::ProtocolObject::from_ref(&*delegate);
        // SAFETY: `delegate` outlives the web view's use of it: both live until the end of main.
        unsafe { web_view.setNavigationDelegate(Some(protocol)) };
        // SAFETY: a valid string; no base URL, so nothing relative loads.
        let _ = unsafe { web_view.loadHTMLString_baseURL(&NSString::from_str(&html), None) };
        assert!(pump(|| delegate.ivars().loaded.get()), "page did not load");
        println!("loaded");

        match args.method.as_str() {
            "create" => create_pdf(&web_view, &args.output),
            method => print(mtm, &web_view, &delegate, &args, method),
        }
    }

    fn create_pdf(web_view: &WKWebView, output: &str) {
        let done = std::rc::Rc::new(Cell::new(false));
        let output_for_block = output.to_owned();
        let done_for_block = done.clone();
        let block = RcBlock::new(move |data: *mut NSData, error: *mut NSError| {
            // SAFETY: WebKit passes valid objects or null to this completion handler.
            let data = unsafe { data.as_ref() };
            // SAFETY: as above.
            let error = unsafe { error.as_ref() };
            match (data, error) {
                (Some(data), _) => {
                    std::fs::write(&output_for_block, data.to_vec()).expect("write");
                }
                (None, error) => println!("createPDF failed: {error:?}"),
            }
            done_for_block.set(true);
        });
        // SAFETY: the block lives until called; no configuration means the whole page.
        unsafe { web_view.createPDFWithConfiguration_completionHandler(None, &block) };
        assert!(pump(|| done.get()), "createPDF did not finish");
    }

    fn print(
        mtm: MainThreadMarker,
        web_view: &WKWebView,
        delegate: &Delegate,
        args: &Args,
        method: &str,
    ) {
        let info = NSPrintInfo::init(NSPrintInfo::alloc());
        info.setPaperSize(NSSize::new(args.paper.0, args.paper.1));
        info.setTopMargin(args.margin);
        info.setBottomMargin(args.margin);
        info.setLeftMargin(args.margin);
        info.setRightMargin(args.margin);
        info.setHorizontalPagination(if args.fit {
            NSPrintingPaginationMode::Fit
        } else {
            NSPrintingPaginationMode::Automatic
        });
        info.setVerticalPagination(NSPrintingPaginationMode::Automatic);
        info.setHorizontallyCentered(false);
        info.setVerticallyCentered(false);
        // SAFETY: the system's constant.
        info.setJobDisposition(unsafe { NSPrintSaveJob });
        let url = NSURL::fileURLWithPath(&NSString::from_str(&args.output));
        let any: &AnyObject = url.as_ref();
        // SAFETY: the print info's own dictionary.
        let dictionary = unsafe { info.dictionary() };
        // SAFETY: the system's constant.
        let key = unsafe { NSPrintJobSavingURL };
        let key = objc2::runtime::ProtocolObject::from_ref(key);
        // SAFETY: this key takes an NSURL, per NSPrintInfo's documentation.
        unsafe { dictionary.setObject_forKey(any, key) };

        // SAFETY: `info` is a valid print info and the page has loaded.
        let operation: Retained<NSPrintOperation> =
            unsafe { web_view.printOperationWithPrintInfo(&info) };
        operation.setShowsPrintPanel(false);
        operation.setShowsProgressPanel(false);
        operation.setJobTitle(Some(&NSString::from_str("Spike title")));
        if let Some(view) = operation.view().filter(|_| args.set_frame) {
            view.setFrame(NSRect::new(
                NSPoint::new(0.0, 0.0),
                NSSize::new(args.paper.0 - 2.0 * args.margin, args.paper.1),
            ));
        }
        if method == "run" {
            println!("runOperation: {}", operation.runOperation());
            return;
        }
        let rect = NSRect::new(
            NSPoint::new(0.0, 0.0),
            NSSize::new(args.paper.0, args.paper.1),
        );
        // SAFETY: a borderless window that is never shown; released by us, not on close.
        let window = unsafe {
            NSWindow::initWithContentRect_styleMask_backing_defer(
                NSWindow::alloc(mtm),
                rect,
                NSWindowStyleMask::Borderless,
                NSBackingStoreType::Buffered,
                false,
            )
        };
        // SAFETY: as documented above.
        unsafe { window.setReleasedWhenClosed(false) };
        if args.content_view {
            window.setContentView(Some(web_view));
        }
        let delegate_any: &AnyObject = delegate.as_ref();
        // SAFETY: the selector has the signature the method asks for, and `delegate` and
        // `window` stay alive until it was called, as `pump` waits for that.
        unsafe {
            operation.runOperationModalForWindow_delegate_didRunSelector_contextInfo(
                &window,
                Some(delegate_any),
                Some(sel!(printOperationDidRun:success:contextInfo:)),
                NonNull::<c_void>::dangling().as_ptr(),
            );
        }
        assert!(
            pump(|| delegate.ivars().printed.get().is_some()),
            "print did not finish"
        );
        println!("modal print: {:?}", delegate.ivars().printed.get());
    }
}

fn main() {
    #[cfg(target_os = "macos")]
    spike::main();
}
