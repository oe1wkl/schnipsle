// SPDX-License-Identifier: Apache-2.0
//! Spike for M6, findings in `docs/spikes/abc-export.md`: can Rust have ABC
//! engraved to SVG by abcjs in a web view that is never shown, without the
//! editor's web view?
//!
//! ```sh
//! cargo run -p schnipsle-apple --example engrave_spike -- out-dir a.abc b.abc …
//! ```
//!
//! Loads `node_modules/abcjs/dist/abcjs-basic-min.js` into a hidden `WKWebView`
//! with JavaScript on, engraves every tune of each file and writes
//! `out-dir/<file>-<n>.svg`, with the times taken.

#[cfg(target_os = "macos")]
mod spike {
    use std::cell::{Cell, RefCell};
    use std::path::Path;
    use std::rc::Rc;
    use std::time::{Duration, Instant};

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::{AnyObject, NSObjectProtocol};
    use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
    use objc2_app_kit::{NSApplication, NSApplicationActivationPolicy};
    use objc2_foundation::{
        NSDate, NSDefaultRunLoopMode, NSError, NSObject, NSPoint, NSRect, NSRunLoop, NSSize,
        NSString,
    };
    use objc2_web_kit::{WKNavigation, WKNavigationDelegate, WKWebView, WKWebViewConfiguration};

    #[derive(Default)]
    struct Flags {
        loaded: Cell<bool>,
    }

    define_class!(
        #[unsafe(super(NSObject))]
        #[thread_kind = MainThreadOnly]
        #[name = "SchnipsleEngraveSpikeDelegate"]
        #[ivars = Flags]
        struct Delegate;

        unsafe impl NSObjectProtocol for Delegate {}

        unsafe impl WKNavigationDelegate for Delegate {
            #[unsafe(method(webView:didFinishNavigation:))]
            fn did_finish(&self, _web_view: &WKWebView, _navigation: Option<&WKNavigation>) {
                self.ivars().loaded.set(true);
            }
        }
    );

    fn pump(done: impl Fn() -> bool) -> bool {
        let start = Instant::now();
        while !done() {
            if start.elapsed() > Duration::from_secs(30) {
                return false;
            }
            let until = NSDate::dateWithTimeIntervalSinceNow(0.01);
            // SAFETY: the system's constant.
            let mode = unsafe { NSDefaultRunLoopMode };
            NSRunLoop::currentRunLoop().runMode_beforeDate(mode, &until);
        }
        true
    }

    /// The page: abcjs and a function that engraves a list of ABC sources, each
    /// tune to an SVG of its own, in black, without the editor's classes.
    const SCRIPT: &str = r##"
window.engrave = function (sources, staffwidth) {
  const serializer = new XMLSerializer();
  return JSON.stringify(sources.map(function (source) {
    const count = ABCJS.parseOnly(source).length;
    const targets = [];
    for (let i = 0; i < count; i++) {
      const div = document.createElement("div");
      document.body.appendChild(div);
      targets.push(div);
    }
    const tunes = ABCJS.renderAbc(targets, source, {
      staffwidth: staffwidth,
      foregroundColor: "#000000",
      paddingleft: 0, paddingright: 0,
    });
    const out = targets.map(function (div, i) {
      const svg = div.querySelector("svg");
      return {
        svg: svg ? serializer.serializeToString(svg) : null,
        warnings: (tunes[i] && tunes[i].warnings) || [],
      };
    });
    targets.forEach(function (div) { div.remove(); });
    return out;
  }));
};
"##;

    pub fn main() {
        let mut args = std::env::args().skip(1);
        let out = args.next().expect("out dir");
        let files: Vec<String> = args.collect();
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let abcjs =
            std::fs::read_to_string(root.join("node_modules/abcjs/dist/abcjs-basic-min.js"))
                .expect("abcjs; run pnpm install");
        let html = format!(
            "<!doctype html><meta charset=utf-8><body><script>{abcjs}</script><script>{SCRIPT}</script>"
        );

        let mtm = MainThreadMarker::new().expect("main thread");
        let app = NSApplication::sharedApplication(mtm);
        app.setActivationPolicy(NSApplicationActivationPolicy::Accessory);
        app.finishLaunching();

        let started = Instant::now();
        let frame = NSRect::new(NSPoint::new(0.0, 0.0), NSSize::new(800.0, 600.0));
        // SAFETY: a plain new configuration.
        let configuration = unsafe { WKWebViewConfiguration::new(mtm) };
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
        // SAFETY: the delegate lives until the end of `main`.
        unsafe { web_view.setNavigationDelegate(Some(protocol)) };
        // SAFETY: a valid string, and no base URL.
        let _ = unsafe { web_view.loadHTMLString_baseURL(&NSString::from_str(&html), None) };
        assert!(pump(|| delegate.ivars().loaded.get()), "page did not load");
        println!("loaded in {:?}", started.elapsed());

        let sources: Vec<String> = files
            .iter()
            .map(|f| std::fs::read_to_string(f).expect("read abc"))
            .collect();
        for round in 0..2 {
            let started = Instant::now();
            let call = format!(
                "engrave({}, 600)",
                serde_json::to_string(&sources).expect("json")
            );
            let result: Rc<RefCell<Option<Result<String, String>>>> = Rc::default();
            let slot = result.clone();
            let block = RcBlock::new(move |value: *mut AnyObject, error: *mut NSError| {
                // SAFETY: WebKit passes a valid object or null.
                let value = unsafe { value.as_ref() };
                // SAFETY: as above.
                let error = unsafe { error.as_ref() };
                *slot.borrow_mut() = Some(match (value, error) {
                    (Some(value), _) => value
                        .downcast_ref::<NSString>()
                        .map(ToString::to_string)
                        .ok_or_else(|| "not a string".to_owned()),
                    (None, Some(error)) => Err(error.localizedDescription().to_string()),
                    (None, None) => Err("nothing".to_owned()),
                });
            });
            // SAFETY: the page has loaded; the block lives as long as WebKit needs it.
            unsafe {
                web_view
                    .evaluateJavaScript_completionHandler(&NSString::from_str(&call), Some(&block));
            }
            assert!(
                pump(|| result.borrow().is_some()),
                "engraving did not finish"
            );
            let json = result.borrow_mut().take().unwrap().expect("engraved");
            println!(
                "round {round}: engraved in {:?}, {} bytes of JSON",
                started.elapsed(),
                json.len()
            );
            if round == 0 {
                let parsed: Vec<Vec<serde_json::Value>> =
                    serde_json::from_str(&json).expect("parse");
                for (file, tunes) in files.iter().zip(parsed) {
                    let stem = Path::new(file)
                        .file_stem()
                        .unwrap()
                        .to_string_lossy()
                        .into_owned();
                    for (n, tune) in tunes.iter().enumerate() {
                        let target = Path::new(&out).join(format!("{stem}-{}.svg", n + 1));
                        if let Some(svg) = tune["svg"].as_str() {
                            std::fs::write(&target, svg).expect("write svg");
                            println!(
                                "{} ({} bytes), warnings: {}",
                                target.display(),
                                svg.len(),
                                tune["warnings"]
                            );
                        }
                    }
                }
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn main() {
    spike::main();
}

#[cfg(not(target_os = "macos"))]
fn main() {}
