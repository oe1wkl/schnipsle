// SPDX-License-Identifier: Apache-2.0
//! Schnipsle core: everything that touches notes, independent of any UI.
//!
//! The vault format is specified in `docs/format.md`; architecture decisions are in `docs/adr/`.
//! This crate must build and test on every platform without a `WebView`.

pub mod abc;
pub mod attachments;
pub mod chordpro;
pub mod conflicts;
pub mod coordination;
pub mod dates;
pub mod diff;
mod flat_yaml;
pub mod front_matter;
pub mod history;
pub mod import_log;
pub mod index;
pub mod index_sync;
pub mod layout;
pub mod leftovers;
pub mod links;
pub mod listing;
pub mod names;
pub mod query;
pub mod retag;
pub mod search;
pub mod settings;
pub mod settle;
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
    ConflictVersion, KnownNote, KnownNotes, NoIdLookup, NoSyncConflicts, NotesById, RawVersion,
    SyncConflicts,
};
pub use coordination::{Coordinator, Uncoordinated};
pub use diff::{Comparison, KeyChange, Part, Row};
pub use history::{Retention, Thinning, Version};
pub use import_log::{ImportLog, ImportLogError, LogAction, LogEntry};
pub use index::{Index, IndexError};
pub use index_sync::IndexSync;
pub use layout::{Folder, VaultIdentity, inspect};
pub use leftovers::Leftovers;
pub use listing::{EntryKind, FileStat, FolderEntry, FolderLister, PlainLister};
pub use names::{NewNote, slug};
pub use retag::{
    InvalidRename, PlannedFolder, PlannedNote, RetagOutcome, RetagPlan, RetagUndo, SkippedNote,
    TagRename,
};
pub use search::{SearchHit, SnippetPart};
pub use settings::{Change, Deviation, DeviceSettings, Setting, VaultSettings};
pub use settle::OtherUsed;
pub use smart_folders::{FolderSort, SmartFolder, SmartFolderError, SmartFolders};
pub use strays::{MovedNotes, StrayNotes};
pub use tag_folders::{FolderKind, TagFolder};
pub use tags::{Tag, extract_tags};
pub use trash::TrashedNote;
pub use vault::{NoteContent, NoteEntry, PathKind, SavedNote, SortOrder, Vault, VaultError};
pub use watch::{Hint, VaultChanges};
