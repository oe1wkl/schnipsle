// SPDX-License-Identifier: Apache-2.0
//! What an adapter hands to the pipeline (ADR-0012).

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// Where notes come from (format.md §12.1). Serialized as the `source`
/// value, e.g. `"apple-notes"`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Source {
    /// The Apple Notes Markdown export plus `apple-notes-metadata.json`.
    AppleNotes,
    /// The Simplenote export.
    Simplenote,
    /// HTML files.
    Html,
    /// A folder of Markdown files.
    Markdown,
}

impl Source {
    /// The `source` value in front matter and import logs.
    #[must_use]
    pub fn id(self) -> &'static str {
        match self {
            Self::AppleNotes => "apple-notes",
            Self::Simplenote => "simplenote",
            Self::Html => "html",
            Self::Markdown => "markdown",
        }
    }

    /// The folder in `notes/` its notes go to.
    #[must_use]
    pub fn folder(self) -> &'static str {
        match self {
            Self::AppleNotes => "Apple Notes",
            Self::Simplenote => "Simplenote",
            Self::Html => "HTML",
            Self::Markdown => "Markdown",
        }
    }
}

/// Everything an adapter read from one export.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Batch {
    /// The source.
    pub source: Source,
    /// The name of the chosen file or folder, for display and the log.
    pub input: String,
    /// The notes, in the order to import them.
    pub notes: Vec<ImportedNote>,
    /// What the adapter wants the report to say, e.g. notes the export lost.
    #[serde(default)]
    pub notices: Vec<Notice>,
}

/// One note as the adapter read it, converted to Markdown.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedNote {
    /// The note's identifier in the source (format.md §12.1).
    pub source_id: String,
    /// Its title, which names the file; the first line of `body` if empty.
    #[serde(default)]
    pub title: String,
    /// The text in Markdown. It may start with front matter of another tool,
    /// which is kept (format.md §4).
    pub body: String,
    /// The source's folders, from the top level down.
    #[serde(default)]
    pub folders: Vec<String>,
    /// Creation time in the source, RFC 3339.
    #[serde(default)]
    pub created: Option<String>,
    /// Last modification in the source, RFC 3339.
    #[serde(default)]
    pub modified: Option<String>,
    /// Tags the source keeps outside the text (format.md §12.1 rule 5).
    #[serde(default)]
    pub tags: Vec<String>,
    /// Pinned in the source.
    #[serde(default)]
    pub pinned: bool,
    /// In the source's trash: a new note goes to the trash (format.md §8,
    /// §12.1 rule 6).
    #[serde(default)]
    pub trashed: bool,
    /// Files the text links to.
    #[serde(default)]
    pub attachments: Vec<ImportedAttachment>,
}

/// A file a note links to, stored under `attachments/` on import
/// (format.md §6).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedAttachment {
    /// The link destination exactly as it appears in the body, e.g.
    /// `Attachments/7C1A….png`; links to it are rewritten.
    pub link: String,
    /// The original file name; its extension is kept.
    pub file_name: String,
    /// The content.
    pub data: AttachmentData,
}

/// Where an attachment's content is.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum AttachmentData {
    /// A file in the export.
    File(PathBuf),
    /// Content the adapter already has, e.g. from a `data:` URL.
    Bytes(Vec<u8>),
}

/// A line for the import report. Serialized with camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Notice {
    /// The note it is about, if any.
    #[serde(default)]
    pub source_id: Option<String>,
    /// Its title, if any.
    #[serde(default)]
    pub title: Option<String>,
    /// What to tell the user.
    pub message: String,
}

impl Notice {
    /// A notice about the whole import.
    #[must_use]
    pub fn general(message: impl Into<String>) -> Self {
        Self {
            source_id: None,
            title: None,
            message: message.into(),
        }
    }
}
