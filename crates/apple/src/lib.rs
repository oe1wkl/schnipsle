// SPDX-License-Identifier: Apache-2.0
//! Apple platform services for Schnipsle (ADR-0015).
//!
//! [`FileCoordinator`] implements `schnipsle_core::Coordinator` with
//! `NSFileCoordinator`, so vault reads and writes on macOS and iOS cooperate with
//! iCloud Drive and other apps (format.md §10). [`VaultPresenter`] is an
//! `NSFilePresenter` on the vault folder that reports changes made elsewhere.
//!
//! This is one of two crates allowed to use `unsafe`. Every `unsafe` block carries
//! a `// SAFETY:` comment, and the public API is safe. On other platforms the
//! crate is empty.

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod coordinator;

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod presenter;

#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use coordinator::FileCoordinator;
#[cfg(any(target_os = "macos", target_os = "ios"))]
pub use presenter::VaultPresenter;
