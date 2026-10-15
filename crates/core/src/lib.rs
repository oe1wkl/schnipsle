// SPDX-License-Identifier: Apache-2.0
//! Schnipsle core: everything that touches notes, independent of any UI.
//!
//! The vault format is specified in `docs/format.md`; architecture decisions are in `docs/adr/`.
//! This crate must build and test on every platform without a `WebView`.

pub mod coordination;
pub mod tags;
pub mod vault;

pub use coordination::{Coordinator, Uncoordinated};
pub use tags::{Tag, extract_tags};
pub use vault::{NoteContent, NoteEntry, Vault, VaultError};
