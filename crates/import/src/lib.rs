// SPDX-License-Identifier: Apache-2.0
//! Importer adapters and the shared import pipeline (ADR-0012, ADR-0019,
//! format.md §12).
//!
//! An adapter reads a source (an export folder or file) into a [`Batch`] of
//! [`ImportedNote`]s: Markdown text, the source's folders, dates, tags and
//! attachments. The pipeline does the rest, the same for every source:
//!
//! 1. [`plan`] is the dry run. It turns each note into the file it will
//!    become (front matter, attachment links, a final tag line, §12.1), picks
//!    its place, and checks the import logs: a note imported before is
//!    updated only if it is unchanged here and newer in the source (§12.3).
//!    Nothing is written; the [`Plan`] is the report to show.
//! 2. [`apply`] writes what the plan says: attachments, then each note,
//!    atomically and coordinated, a history snapshot before every update,
//!    and the import log (§12.2) ahead of the notes it lists.
//!
//! The caller updates the search index for the paths in the [`Outcome`] in
//! one go.
//!
//! Adapters: [`apple_notes`], [`simplenote`], [`html`].

pub mod apple_notes;
pub mod html;
mod note;
mod pipeline;
mod prepare;
pub mod simplenote;

pub use note::{AttachmentData, Batch, ImportedAttachment, ImportedNote, Notice, Source};
pub use pipeline::{
    Env, ImportError, Outcome, Plan, PlanItem, PlannedAction, SkipReason, SystemEnv, Written,
    apply, apply_with_progress, plan,
};
pub use prepare::{folder_tag, tag_from};
