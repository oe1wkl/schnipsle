// SPDX-License-Identifier: Apache-2.0
//! The share menu of macOS for an exported file or folder: Mail, Messages, Notes,
//! `AirDrop` and other apps, as the share sheet offers on iOS.

use std::cell::RefCell;
use std::io;
use std::path::Path;

use objc2::rc::Retained;
use objc2::{AnyThread as _, MainThreadMarker};
use objc2_app_kit::{NSApplication, NSSharingServicePicker};
use objc2_foundation::{NSArray, NSPoint, NSRect, NSRectEdge, NSSize, NSString, NSURL};

thread_local! {
    /// The picker shown last, kept while its menu is open.
    static PICKER: RefCell<Option<Retained<NSSharingServicePicker>>> = const { RefCell::new(None) };
}

/// Shows the share menu for the file or folder at `path`, next to the place
/// `anchor` (left, top, width, height) in the key window's content, measured
/// from its top left as the page measures it. Returns at once; the menu does
/// the rest. Call on the main thread.
///
/// # Errors
/// If not on the main thread, or there is no window to show it in.
pub fn share_files(path: &Path, anchor: [f64; 4]) -> io::Result<()> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| io::Error::other("The share menu can only be shown from the main thread"))?;
    let app = NSApplication::sharedApplication(mtm);
    let window = app
        .keyWindow()
        .or_else(|| app.mainWindow())
        .ok_or_else(|| io::Error::other("There is no window to show the share menu in"))?;
    let view = window
        .contentView()
        .ok_or_else(|| io::Error::other("The window has no content to show the share menu in"))?;
    let [left, top, width, height] = anchor;
    let bounds = view.bounds();
    // AppKit counts from the bottom left unless the view is flipped.
    let y = if view.isFlipped() {
        top
    } else {
        bounds.size.height - top - height
    };
    let rect = NSRect::new(NSPoint::new(left, y), NSSize::new(width, height));
    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let items = NSArray::from_retained_slice(&[Retained::into_super(Retained::into_super(url))]);
    // SAFETY: the item is a file URL, which the picker can share.
    let picker =
        unsafe { NSSharingServicePicker::initWithItems(NSSharingServicePicker::alloc(), &items) };
    picker.showRelativeToRect_ofView_preferredEdge(rect, &view, NSRectEdge::MinY);
    PICKER.with(|kept| *kept.borrow_mut() = Some(picker));
    Ok(())
}
