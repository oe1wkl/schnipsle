// SPDX-License-Identifier: Apache-2.0
// Data the app shell sends and receives (mirrors of schnipsle-core and
// src-tauri types, serialized with camelCase names).

export interface VaultInfo {
  name: string;
  path: string;
  /** Why the vault is read-only (format.md §2), if it is. */
  readOnly: string | null;
}
/** A picked folder that is not a vault yet (src-tauri `Opened::Setup`). */
export interface FolderSetup {
  name: string;
  path: string;
  /** Visible items at its top level that would move into notes/. */
  items: string[];
  notes: number;
  /** Items only in iCloud; their download was requested. */
  notDownloaded: string[];
}
/** Notes outside notes/, e.g. from an older build (schnipsle-core `StrayNotes`). */
export interface StrayNotes {
  items: string[];
  notes: number;
  notDownloaded: string[];
}
/** What moving them did (schnipsle-core `MovedNotes`). */
export interface MovedNotes {
  moved: string[];
  skipped: string[];
  /** Notes whose links were rewritten to keep working (ADR-0016). */
  relinked: string[];
}
/** Result of picking or restoring a folder (src-tauri `Opened`). */
export type Opened = ({ kind: "vault" } & VaultInfo) | ({ kind: "setup" } & FolderSetup);
export interface NoteEntry {
  path: string;
  title: string;
  modifiedMs: number;
  available: boolean;
  /** Front matter `pinned: true` (format.md §4); listed first. */
  pinned: boolean;
}
/** A version a sync service kept because it conflicts (schnipsle-core `ConflictVersion`). */
export interface ConflictVersion {
  text: string;
  hash: string;
  modifiedMs: number;
  device: string | null;
  /** Set for another file with the same id, e.g. iCloud's copy `Name 2.md`. */
  copyPath: string | null;
}
export interface NoteContent {
  path: string;
  title: string;
  text: string;
  hash: string;
  conflicts: ConflictVersion[];
  conflictsError: string | null;
}
export interface Tag {
  name: string;
  key: string;
}
/** Payload of the `vault-changed` event (schnipsle-core `VaultChanges`). */
export interface VaultChanges {
  paths: string[];
  rescan: boolean;
  /** The smart folders may have changed (`.notesapp/smart-folders.yaml`). */
  smartFolders: boolean;
}
/** What a save wrote: the text on disk, with the front matter the core maintains. */
export interface SavedNote {
  text: string;
  hash: string;
}
/** A note in the trash (schnipsle-core `TrashedNote`). */
export interface TrashedNote {
  key: string;
  title: string;
  originalPath: string;
  deleted: string;
}
/** A stored attachment (schnipsle-core `AddedAttachment`). */
export interface AddedAttachment {
  path: string;
  link: string;
  markdown: string;
}
/** A version in a note's history (schnipsle-core `Version`). */
export interface Version {
  name: string;
  takenMs: number | null;
}
export interface NewNote {
  path: string;
  saved: SavedNote;
}
export type SortOrder = "modified" | "title";
export interface CommandError {
  kind: string;
  message: string;
  currentHash: string | null;
}

/** A piece of a search snippet (schnipsle-core `SnippetPart`). */
export interface SnippetPart {
  text: string;
  hit: boolean;
}

/** A search result (schnipsle-core `SearchHit`). */
export interface SearchHit {
  path: string;
  title: string;
  modifiedMs: number;
  snippet: SnippetPart[];
  /** In the trash: `path` is below the vault root and the note must be restored first. */
  inTrash: boolean;
  pinned: boolean;
}

/** Where an import reads from (schnipsle-import `Source`). */
export type ImportSource = "apple-notes" | "simplenote" | "html" | "markdown";
/** What an import does with a note (schnipsle-import `PlannedAction`). */
export type PlannedAction = "add" | "update" | "skip";
/** Why a note is not imported (schnipsle-import `SkipReason`, format.md §12.3). */
export type SkipReason =
  | "duplicate"
  | "changedHere"
  | "notNewer"
  | "noModifiedTime"
  | "missing"
  | "changedMeanwhile"
  | "failed";
/** One note in an import report (schnipsle-import `PlanItem`). */
export interface PlanItem {
  sourceId: string;
  title: string;
  action: PlannedAction;
  reason: SkipReason | null;
  /** Relative to notes/: where it goes, or the note it updates or that blocks it. */
  path: string | null;
  problems: string[];
}
/** A report line about the import as a whole or a note (schnipsle-import `Notice`). */
export interface ImportNotice {
  sourceId: string | null;
  title: string | null;
  message: string;
}
/** The dry run (schnipsle-import `Plan`). */
export interface ImportPlan {
  source: ImportSource;
  input: string;
  items: PlanItem[];
  notices: ImportNotice[];
}
/** What `plan_import` returns. */
export interface ImportPreview {
  plan: ImportPlan;
  path: string;
  /** Apple Notes: whether the export has `apple-notes-metadata.json`. */
  hasMetadata: boolean;
}
/** What `apply_import` returns (schnipsle-import `Outcome`). */
export interface ImportOutcome {
  runId: string | null;
  plan: ImportPlan;
  written: { path: string; action: "added" | "updated" }[];
}
