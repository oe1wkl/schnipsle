// SPDX-License-Identifier: Apache-2.0
//! Apple platform services for Schnipsle (ADR-0015).
//!
//! [`FileCoordinator`] implements `schnipsle_core::Coordinator` with
//! `NSFileCoordinator`, so vault reads and writes on macOS and iOS cooperate with
//! iCloud Drive and other apps (format.md §10). [`VaultPresenter`] is an
//! `NSFilePresenter` on the vault folder that reports changes made elsewhere, and
//! [`ICloudVersions`] lists and resolves iCloud conflict versions (ADR-0006).
//! [`print_pdf`] turns an HTML page into a PDF with real pages (ADR-0022), and
//! [`find_anchors`] finds the texts of the links that printing on iOS loses (ADR-0037).
//! On macOS, [`share_files`] shows the share menu for an export, and
//! [`ask_before_terminating`] holds back quitting until the open note is saved
//! (ADR-0042).
//! [`convert_picture`] turns a picture many programs cannot show into one they can
//! (ADR-0027), and [`is_turned`] tells one that only its orientation turns upright
//! (ADR-0028).
//! [`footprint`] reads the app's memory, and [`device_model`] and
//! [`system_version`] tell what it runs on, for the measurements of ADR-0045.
//!
//! This is one of two crates allowed to use `unsafe`. Every `unsafe` block carries
//! a `// SAFETY:` comment, and the public API is safe. On other platforms the
//! crate is empty.

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod anchors;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod coordinator;

#[cfg(target_os = "macos")]
mod pdf;
#[cfg(target_os = "ios")]
mod pdf_ios;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod picture;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod presenter;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod process;
#[cfg(target_os = "macos")]
mod share;
#[cfg(target_os = "macos")]
mod terminate;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod versions;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use anchors::{AnchorText, FoundLine, find_anchors};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use coordinator::FileCoordinator;
#[cfg(target_os = "macos")]
pub use pdf::{PdfPage, print_pdf};
#[cfg(target_os = "ios")]
pub use pdf_ios::{PdfPage, print_pdf};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use picture::{ConvertedPicture, convert_picture, is_turned};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use presenter::VaultPresenter;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use process::{Footprint, device_model, footprint, system_version};
#[cfg(target_os = "macos")]
pub use share::share_files;
#[cfg(target_os = "macos")]
pub use terminate::{ask_before_terminating, reply_to_terminate};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use versions::ICloudVersions;
