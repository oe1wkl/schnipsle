// SPDX-License-Identifier: Apache-2.0
//! Schnipsle core: everything that touches notes, independent of any UI.
//!
//! The vault format is specified in `docs/format.md`; architecture decisions are in `docs/adr/`.
//! This crate must build and test on every platform without a `WebView`.

pub mod attachments;
pub mod chordpro;
pub mod conflicts;
pub mod coordination;
pub mod dates;
mod flat_yaml;
pub mod front_matter;
pub mod history;
pub mod import_log;
pub mod index;
pub mod index_sync;
pub mod layout;
pub mod links;
pub mod names;
pub mod query;
pub mod search;
pub mod smart_folders;
pub mod stamp;
pub mod strays;
pub mod tag_folders;
pub mod tags;
pub mod trash;
pub mod vault;
pub mod watch;

pub use attachments::{AddedAttachment, LinkTarget};
pub use conflicts::{
    ConflictVersion, NoIdLookup, NoSyncConflicts, NotesById, RawVersion, SyncConflicts,
};
pub use coordination::{Coordinator, Uncoordinated};
pub use history::{Retention, Version};
pub use import_log::{ImportLog, ImportLogError, LogAction, LogEntry};
pub use index::{Index, IndexError};
pub use index_sync::IndexSync;
pub use layout::{Folder, VaultIdentity, inspect};
pub use names::{NewNote, slug};
pub use search::{SearchHit, SnippetPart};
pub use smart_folders::{FolderSort, SmartFolder, SmartFolderError, SmartFolders};
pub use strays::{MovedNotes, StrayNotes};
pub use tag_folders::{FolderKind, TagFolder};
pub use tags::{Tag, extract_tags};
pub use trash::TrashedNote;
pub use vault::{NoteContent, NoteEntry, PathKind, SavedNote, SortOrder, Vault, VaultError};
pub use watch::{Hint, VaultChanges};
