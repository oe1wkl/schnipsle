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
