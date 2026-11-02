// SPDX-License-Identifier: Apache-2.0
//! Exporters: built-in HTML, PDF and ODT on all platforms, optional Pandoc on desktop.
//!
//! Design: [ADR-0010](../../docs/adr/0010-export-engines.md) for the engines,
//! [ADR-0021](../../docs/adr/0021-what-an-exported-note-looks-like.md) for what
//! an exported note contains. The rules live in `document`, which turns a note
//! into a syntax tree every format can write; so far HTML is written, by
//! [`HtmlExporter`].

mod document;
mod html;
mod output;
mod raw_html;

use std::path::Path;

use schnipsle_core::vault::{Result, Vault};
use serde::Serialize;

pub use html::HtmlExporter;

/// A format an [`Exporter`] writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    /// HTML with its own style sheet.
    Html,
}

/// What to export (ADR-0021 rule 3).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Input {
    /// The notes by their path relative to `notes/`, in the order of the list they come from.
    pub notes: Vec<String>,
    /// The name of the tag folder, search or smart folder they come from: the
    /// title of one document made of several notes.
    pub name: Option<String>,
}

/// Where the notes go.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Layout {
    /// One document, the file `to`, holding its pictures. Replaces what is there:
    /// the save dialog has asked.
    SingleFile,
    /// One document in the folder `to`, named after its title, with copies of the
    /// attachments it links to.
    Folder,
    /// One document per note in the folder `to`, named after its note's title,
    /// with copies of the attachments they link to.
    FilePerNote,
}

/// How to export.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Options {
    /// Where the notes go.
    pub layout: Layout,
    /// Keep the final line of tags of a note (ADR-0021 rule 4).
    pub keep_tag_line: bool,
}

/// What an export did (ADR-0021 rule 12). Serialized with camelCase names.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Report {
    /// The files written, relative to the folder they went to, with `/`.
    pub files: Vec<String>,
    /// What did not make it into the export as it is in the vault.
    pub problems: Vec<Problem>,
}

/// One thing an export could not carry over.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Problem {
    /// The note, by its path relative to `notes/`.
    pub path: String,
    /// The title of the note, or its path if it could not be read.
    pub title: String,
    /// What happened.
    pub kind: ProblemKind,
    /// The link as written in the note; empty for [`ProblemKind::NoteNotRead`].
    pub target: String,
}

/// What happened to a link, picture or note.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ProblemKind {
    /// The note could not be read, e.g. it is not downloaded yet, so it is not in the export.
    NoteNotRead,
    /// The picture is not in the vault; its description is shown in brackets.
    PictureMissing,
    /// A picture on the web is shown as a link, as the export works offline.
    WebPictureAsLink,
    /// The attachment is not in this export; the text of its link stays.
    AttachmentNotIncluded,
    /// The note linked to is not in this export; the text of its link stays.
    NoteNotExported,
}

/// Writes notes in one format (ADR-0010).
pub trait Exporter {
    /// The format this exporter writes.
    fn format(&self) -> Format;

    /// Exports `input` to `to`, a file or a folder as [`Layout`] says. A problem
    /// with a single note, picture or link goes into the [`Report`]; without a
    /// note to export nothing is written.
    ///
    /// # Errors
    /// I/O errors while writing; files written before stay.
    fn export(&self, vault: &Vault, input: &Input, options: &Options, to: &Path) -> Result<Report>;
}
