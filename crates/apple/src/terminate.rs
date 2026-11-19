// SPDX-License-Identifier: Apache-2.0
//! Holding back a request to quit the app (ADR-0042 rule 3).
//!
//! Quit in the Dock, the Apple Event "quit" (from a script or the system's
//! updates) and logging out call `-[NSApplication terminate:]`, which asks the
//! app delegate's `applicationShouldTerminate:` and quits at once if the
//! delegate has none. The delegate of a Tauri app is tao's
//! (`TaoAppDelegateParent`), which has none, so [`ask_before_terminating`]
//! adds it to that class. While the app saves, it answers "later", and
//! [`reply_to_terminate`] gives the answer; a logout waits for it.

use std::ffi::CString;
use std::io;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use objc2::encode::Encode;
use objc2::runtime::{AnyObject, Imp, Sel};
use objc2::{MainThreadMarker, ffi, sel};
use objc2_app_kit::{NSApplication, NSApplicationTerminateReply};

/// The question asked before quitting: `true` lets the app quit now.
type Ask = Box<dyn Fn() -> bool + Send + Sync>;

static ASK: OnceLock<Ask> = OnceLock::new();

/// `applicationShouldTerminate:` answered "later", and no reply was given yet.
static WAITING: AtomicBool = AtomicBool::new(false);

/// The signature of `applicationShouldTerminate:`.
type Method = extern "C-unwind" fn(&AnyObject, Sel, *mut AnyObject) -> NSApplicationTerminateReply;

/// `-[delegate applicationShouldTerminate:]`.
extern "C-unwind" fn should_terminate(
    _this: &AnyObject,
    _cmd: Sel,
    _sender: *mut AnyObject,
) -> NSApplicationTerminateReply {
    if ASK.get().is_none_or(|ask| ask()) {
        NSApplicationTerminateReply::TerminateNow
    } else {
        WAITING.store(true, Ordering::SeqCst);
        NSApplicationTerminateReply::TerminateLater
    }
}

/// From now on, a request to quit through `terminate:` asks `ask` first, on
/// the main thread: `true` quits now; `false` makes the request wait until
/// [`reply_to_terminate`] is called, which the app must then do. Call once,
/// on the main thread, after the app delegate is set (in Tauri, in `setup`).
///
/// # Errors
/// If not on the main thread, if called twice, if the app has no delegate, or
/// if the delegate already answers `applicationShouldTerminate:` itself.
pub fn ask_before_terminating(ask: impl Fn() -> bool + Send + Sync + 'static) -> io::Result<()> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| io::Error::other("Quitting can only be held back from the main thread"))?;
    let app = NSApplication::sharedApplication(mtm);
    let delegate = app
        .delegate()
        .ok_or_else(|| io::Error::other("The app has no delegate to hold quitting back"))?;
    let class = AsRef::<AnyObject>::as_ref(&*delegate).class();
    let selector = sel!(applicationShouldTerminate:);
    if class.instance_method(selector).is_some() {
        return Err(io::Error::other(
            "The app delegate already decides about quitting",
        ));
    }
    ASK.set(Box::new(ask))
        .map_err(|_| io::Error::other("Quitting is already held back"))?;
    let types = CString::new(format!(
        "{}{}{}{}",
        NSApplicationTerminateReply::ENCODING,
        <&AnyObject>::ENCODING,
        Sel::ENCODING,
        <*mut AnyObject>::ENCODING,
    ))
    .map_err(io::Error::other)?;
    let method: Method = should_terminate;
    // SAFETY: both are function pointers of the same size. The runtime calls an
    // `Imp` with the arguments of the method it is added as, so the
    // argument-less `Imp` type is how a method is meant to be passed; it is
    // only called as `applicationShouldTerminate:`, whose signature `method` has.
    let imp = unsafe { std::mem::transmute::<Method, Imp>(method) };
    // SAFETY: `imp` has the signature that `types` spells out, the one AppKit
    // calls `applicationShouldTerminate:` with (`self`, `_cmd`, the
    // `NSApplication`; returns `NSApplicationTerminateReply`, an `NSUInteger`).
    // `class` is a registered class, which may get methods added; `types`
    // lives until the call returns, and the runtime copies it.
    let added = unsafe {
        ffi::class_addMethod(
            std::ptr::from_ref(class).cast_mut(),
            selector,
            imp,
            types.as_ptr(),
        )
    };
    if added.as_bool() {
        Ok(())
    } else {
        Err(io::Error::other("Quitting could not be held back"))
    }
}

/// Answers a request to quit that is waiting (see [`ask_before_terminating`]):
/// `true` quits, `false` cancels it (a logout too). Returns whether a request
/// was waiting; if none was, nothing happens. Call on the main thread.
pub fn reply_to_terminate(quit: bool) -> bool {
    let Some(mtm) = MainThreadMarker::new() else {
        return false;
    };
    if !WAITING.swap(false, Ordering::SeqCst) {
        return false;
    }
    NSApplication::sharedApplication(mtm).replyToApplicationShouldTerminate(quit);
    true
}
