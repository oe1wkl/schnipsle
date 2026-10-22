// SPDX-License-Identifier: Apache-2.0
//! Schnipsle core: everything that touches notes, independent of any UI.
//!
//! The vault format is specified in `docs/format.md`; architecture decisions are in `docs/adr/`.
//! This crate must build and test on every platform without a `WebView`.

pub mod attachments;
pub mod conflicts;
pub mod coordination;
mod flat_yaml;
pub mod front_matter;
pub mod history;
pub mod index;
pub mod layout;
pub mod links;
pub mod names;
pub mod stamp;
pub mod strays;
pub mod tags;
pub mod trash;
pub mod vault;
pub mod watch;

pub use attachments::AddedAttachment;
pub use conflicts::{ConflictVersion, NoSyncConflicts, RawVersion, SyncConflicts};
pub use coordination::{Coordinator, Uncoordinated};
pub use history::{Retention, Version};
pub use layout::{Folder, VaultIdentity, inspect};
pub use names::{NewNote, slug};
pub use strays::{MovedNotes, StrayNotes};
pub use tags::{Tag, extract_tags};
pub use trash::TrashedNote;
pub use vault::{NoteContent, NoteEntry, PathKind, SavedNote, SortOrder, Vault, VaultError};
pub use watch::{Hint, VaultChanges};
