// SPDX-License-Identifier: Apache-2.0
//! Apple platform services for Schnipsle (ADR-0015).
//!
//! [`FileCoordinator`] implements `schnipsle_core::Coordinator` with
//! `NSFileCoordinator`, so vault reads and writes on macOS and iOS cooperate with
//! iCloud Drive and other apps (format.md §10). [`VaultPresenter`] is an
//! `NSFilePresenter` on the vault folder that reports changes made elsewhere, and
//! [`ICloudVersions`] lists and resolves iCloud conflict versions (ADR-0006).
//! On macOS, [`print_pdf`] turns an HTML page into a PDF with real pages (ADR-0022).
//! [`convert_picture`] turns a picture many programs cannot show into one they can
//! (ADR-0027), and [`is_turned`] tells one that only its orientation turns upright
//! (ADR-0028).
//!
//! This is one of two crates allowed to use `unsafe`. Every `unsafe` block carries
//! a `// SAFETY:` comment, and the public API is safe. On other platforms the
//! crate is empty.

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod coordinator;

#[cfg(target_os = "macos")]
mod pdf;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod picture;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod presenter;
#[cfg(any(target_os = "macos", target_os = "ios"))]
mod versions;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use coordinator::FileCoordinator;
#[cfg(target_os = "macos")]
pub use pdf::{PdfPage, print_pdf};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use picture::{ConvertedPicture, convert_picture, is_turned};
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use presenter::VaultPresenter;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use versions::ICloudVersions;
