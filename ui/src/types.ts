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
/** Two versions of a note compared (schnipsle-core `Comparison`, ADR-0038). */
export interface Comparison {
  /** Front matter keys whose value differs. */
  frontMatter: KeyChange[];
  /** Lines of the body only the newer version has. */
  added: number;
  /** Lines of the body only the older version has. */
  removed: number;
  rows: DiffRow[];
}
export interface KeyChange {
  key: string;
  /** `null` if only the newer version has the key. */
  before: string | null;
  /** `null` if the newer version no longer has it. */
  after: string | null;
}
/** A piece of a changed line; `changed` marks words the other version lacks. */
export interface DiffPart {
  text: string;
  changed: boolean;
}
/** A row of the compared body; line numbers count from the top of the file. */
export type DiffRow =
  | { kind: "same"; before: number; after: number; text: string }
  | { kind: "removed"; before: number; parts: DiffPart[] }
  | { kind: "added"; after: number; parts: DiffPart[] }
  | { kind: "folded"; before: number; after: number; lines: string[] };
/** What "Use other version" left (schnipsle-core `OtherUsed`, ADR-0042). */
export interface OtherUsed {
  /** The note as it is on disk; `null` if it was deleted elsewhere. */
  note: NoteContent | null;
  /** Where the editor's text went if the note was deleted elsewhere. */
  trashed: TrashedNote | null;
}
export interface NewNote {
  path: string;
  saved: SavedNote;
}
export type SortOrder = "modified" | "title";
export interface CommandError {
  kind: string;
  /** In the user's language once `asError` has read it (ADR-0041 rule 5). */
  message: string;
  currentHash: string | null;
  /** The path or name the error is about, for the catalog's wording. */
  value?: string | null;
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
  | "inTrash"
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
  /** A new note that goes to the trash, as it is in the source's trash; absent otherwise. */
  trashed?: boolean;
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
  /** Tags added to every note, without `#` (format.md §12.1 rule 5); left out when none. */
  tags?: string[];
}
/** What `plan_import` returns. */
export interface ImportPreview {
  plan: ImportPlan;
  path: string;
  /** Apple Notes: whether the export has `apple-notes-metadata.json`; false for other sources. */
  hasMetadata: boolean;
}
/** One import run, from its log (schnipsle-import `ImportRun`, format.md §12.2). */
export interface ImportRun {
  id: string;
  /** As in format.md §12.1; empty for a log that cannot be read. */
  source: ImportSource | "";
  input: string | null;
  /** Tags the run added to every note, without `#`. */
  tags: string[];
  started: string;
  undone: string | null;
  added: number;
  updated: number;
  /** Why it cannot be undone: a damaged log or one of a newer format. */
  problem: string | null;
}
/** What undoing an import does with a note (schnipsle-import `UndoAction`, format.md §12.4). */
export type UndoAction = "trash" | "restore" | "keep" | "missing" | "inTrash";
/** One note in an undo (schnipsle-import `UndoItem`). */
export interface UndoItem {
  title: string;
  /** Relative to notes/; null if the note is gone. */
  path: string | null;
  action: UndoAction;
  problem: string | null;
  trashKey: string | null;
}
/** An undo planned or done (schnipsle-import `Undo`). */
export interface ImportUndo {
  run: ImportRun;
  items: UndoItem[];
}
/** What `apply_import` returns (schnipsle-import `Outcome`). */
export interface ImportOutcome {
  runId: string | null;
  plan: ImportPlan;
  written: { path: string; action: "added" | "updated" }[];
  /** Names in the trash of the notes moved there. */
  trashed: string[];
}

/** A format `export_formats` offers (schnipsle-export `Format`). */
export type ExportFormat = "html" | "pdf" | "odt" | "docx" | "epub";
/** What `export_choices` returns. */
export interface ExportChoices {
  formats: ExportFormat[];
  /** The version of the Pandoc in use, if any. */
  pandoc: string | null;
  /** Why the Pandoc chosen in Settings does not run (ADR-0039). */
  pandocProblem: string | null;
  /** An export can be shared afterwards (`share_export`): macOS and iOS. */
  share: boolean;
}
/** Where the notes go (schnipsle-export `Layout`). */
export type ExportLayout = "singleFile" | "folder" | "filePerNote" | "zip";
/** The paper of a format with pages (schnipsle-export `Paper`). */
export type ExportPaper = "a4" | "letter";
/** What happened to a link, picture or note (schnipsle-export `ProblemKind`). */
export type ExportProblemKind =
  | "noteNotRead"
  | "pictureMissing"
  | "webPictureAsLink"
  | "pictureNotConverted"
  | "attachmentNotIncluded"
  | "noteNotExported"
  | "noteInOtherFile"
  | "scoreNotEngraved";
/** One thing an export could not carry over (schnipsle-export `Problem`). */
export interface ExportProblem {
  path: string;
  title: string;
  kind: ExportProblemKind;
  target: string;
}
/** What `export_notes` returns (schnipsle-export `Report`). */
export interface ExportReport {
  files: string[];
  /** Those of `files` that got a number, as their name was already taken. */
  numbered: string[];
  /** What went wrong in documents that were still written. */
  warnings: ExportWarning[];
  problems: ExportProblem[];
}

/** A warning about a document of an export (schnipsle-export `Warning`), by kind. */
export type ExportWarning =
  | { kind: "linksMissed"; title: string; count: number }
  | { kind: "linksFailed"; title: string; error: string }
  | { kind: "pageNumbersFailed"; title: string; error: string }
  | { kind: "svgOnly"; title: string; description: string | null }
  | { kind: "converter"; title: string; message: string };

/** The names of the notes in a song (schnipsle-core `chordpro::chord::Notation`). */
export type Notation = "english" | "german";
/** A piece of a lyrics line: its chord, as shown, goes over the start of its text. */
export interface SongSegment {
  chord: string | null;
  text: string;
}
/** A line of a song sheet (schnipsle-core `chordpro::sheet::SheetLine`). */
export type SongLine =
  | { type: "lyrics"; segments: SongSegment[] }
  | { type: "chords"; chords: string[] }
  | { type: "comment"; text: string; italic: boolean }
  | { type: "tab"; text: string }
  | { type: "unknown"; text: string }
  | { type: "empty" };
/** A verse, chorus, bridge or tab, or lines outside any of them. */
export interface SongSection {
  kind: "plain" | "verse" | "chorus" | "bridge" | "tab";
  label: string | null;
  /** A chorus sung again by `{chorus}`. */
  repeat: boolean;
  lines: SongLine[];
}
/** What `render_song` returns (schnipsle-core `chordpro::sheet::Sheet`). */
export interface SongSheet {
  /** Title, subtitle, artist, composer, lyricist, key, capo, tempo and time. */
  meta: { name: string; value: string }[];
  sections: SongSection[];
  /** The names the chords are written in. */
  notation: Notation;
  /** The names they are shown in. */
  shown: Notation;
  /** Semitones of the info string and the viewer. */
  transpose: number;
  lyricsOnly: boolean;
  nosource: boolean;
}

/** A setting of the vault, as `read_settings` returns it (ADR-0039). */
export interface Setting<T> {
  /** The value in effect. */
  value: T;
  /** What applies when the file does not set it. */
  default: T;
  /** The value as `settings.yaml` has it; `null` if it does not set it. */
  written: string | null;
  /** Why `value` is not what the file says, if it is not. */
  deviation: "invalid" | "extended" | null;
}
export type ChordNotation = "english" | "german";
/** The settings of the open vault (schnipsle-core `VaultSettings`). */
export interface VaultSettings {
  historyKeepAllDays: Setting<number>;
  historyKeepDailyDays: Setting<number>;
  chordNotation: Setting<ChordNotation>;
}
/** A change of one setting of the vault; `null` removes it from the file. */
export type SettingChange =
  | { key: "historyKeepAllDays"; value: number | null }
  | { key: "historyKeepDailyDays"; value: number | null }
  | { key: "chordNotation"; value: ChordNotation | null };
/** What thinning the history would remove (`history_to_thin`). */
export interface Thinning {
  versions: number;
  notes: number;
}
/** The settings of this device (`read_device_settings`); none on a phone. */
export interface DeviceSettings {
  /** The Pandoc program picked; `null` to find it. */
  pandocChosen: string | null;
  /** The Pandoc in use, if one runs. */
  pandoc: { path: string; version: string } | null;
  /** Why the picked Pandoc does not run. */
  pandocProblem: string | null;
  checkForUpdates: boolean;
}
/** A note a tag rename changes (schnipsle-core `PlannedNote`). */
export interface PlannedNote {
  path: string;
  title: string;
  hash: string;
  occurrences: number;
}
/** A smart folder a tag rename changes (schnipsle-core `PlannedFolder`). */
export interface PlannedFolder {
  id: string;
  name: string;
  query: string;
  renamed: string;
}
/** A note a tag rename leaves as it is, and why (schnipsle-core `SkippedNote`). */
export interface SkippedNote {
  path: string;
  title: string;
  reason: string;
}
/** What a tag rename would change (`plan_tag_rename`, ADR-0040). */
export interface RetagPlan {
  old: string;
  new: string;
  merge: boolean;
  notes: PlannedNote[];
  folders: PlannedFolder[];
  skipped: SkippedNote[];
}
/** What a tag rename or its undo did (`rename_tag`, `undo_tag_rename`). */
export interface RetagOutcome {
  changed: string[];
  skipped: SkippedNote[];
  folders: number;
  foldersError: string | null;
}
