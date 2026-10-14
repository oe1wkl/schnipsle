// SPDX-License-Identifier: Apache-2.0
//! Schnipsle core: everything that touches notes, independent of any UI.
//!
//! The vault format is specified in `docs/format.md`; architecture decisions are in `docs/adr/`.
//! This crate must build and test on every platform without a `WebView`.

pub mod tags;

pub use tags::{Tag, extract_tags};
