// SPDX-License-Identifier: Apache-2.0
// Spike UI: open a vault folder, list notes, edit with autosave in the
// CodeMirror editor (ADR-0007), react to changes made elsewhere (`vault-changed`
// events, ADR-0015) and resolve conflicts.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { asError, setStatus, ui } from "./dom";
import { createEditor, type EditorMode, frontMatterLength, isPinned } from "./editor";
import { createImageCache, fileName } from "./images";
import { createSidebar, type ListFilter } from "./sidebar";
import type {
  VaultInfo,
  FolderSetup,
  StrayNotes,
  MovedNotes,
  Opened,
  NoteEntry,
  ConflictVersion,
  NoteContent,
  Tag,
  VaultChanges,
  SavedNote,
  TrashedNote,
  AddedAttachment,
  Version,
  NewNote,
  SearchHit,
  SnippetPart,
  SortOrder,
} from "./types";

const AUTOSAVE_MS = 1000;
/** A typing pause this long stores a version in the history (ADR-0011). */
const HISTORY_PAUSE_MS = 30_000;

/**
 * The note in the editor. `savedText` is what is on disk (as far as we know);
 * `titleAtOpen` decides whether leaving the note renames its file.
 */
let current: { path: string; hash: string; savedText: string; titleAtOpen: string } | null = null;
const images = createImageCache(() => current?.path ?? null);
let listMode: "notes" | "trash" = "notes";
let sortOrder: SortOrder = "modified";
let lastTrashed: TrashedNote | null = null;
let conflictHash: string | null | undefined; // set while the conflict banner shows
let syncConflict: ConflictVersion | undefined; // shown in the sync conflict banner
let entries: NoteEntry[] = [];
let saveTimer: number | undefined;
let saving: Promise<void> = Promise.resolve();
/** Why the open vault is read-only; `null` if it is writable or none is open. */
let readOnly: string | null = null;
/** The notes outside notes/ last shown, and those the user said "Not now" to. */
let straysShown = "";
let straysDismissed = "";

const editor = createEditor(ui.editorHost, {
  onChange: () => {
    setStatus("Editing…");
    scheduleSave();
    scheduleSnapshot();
    showPinned();
    void showTags();
  },
  onBlur: () => void flushSave(),
  onFiles: (files, pos) => void attachFiles(files, pos),
  loadImage: (url) => images.load(url),
});

/**
 * The version selected in the history, read-only and in the same mode as the
 * editor, so Preview renders it like the note (images resolve from the note).
 */
const historyView = createEditor(ui.historyPreview, {
  onChange: () => undefined,
  onBlur: () => undefined,
  onFiles: () => undefined,
  loadImage: (url) => images.load(url),
  placeholderText: "Select a version",
});

/**
 * Stores files as attachments (format.md §6) and inserts links to them at
 * `pos`, or in place of the selection.
 */
async function attachFiles(files: File[], pos: number | null): Promise<void> {
  const note = current;
  if (!note || readOnly !== null) {
    setStatus(note ? "This vault is read-only." : "Open a note first.");
    return;
  }
  const links: string[] = [];
  for (const file of files) {
    const name = fileName(file);
    try {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const added = await invoke<AddedAttachment>("add_attachment", bytes, {
        headers: { "x-note": encodeURIComponent(note.path), "x-name": encodeURIComponent(name) },
      });
      links.push(added.markdown);
    } catch (e) {
      setStatus(`Not attached: ${name}: ${asError(e).message}`);
    }
  }
  if (links.length > 0 && current === note) editor.insert(links.join("\n"), pos);
}

const isDirty = (): boolean => current !== null && editor.text() !== current.savedText;

/** The view shown on the phone; on wider screens list and editor are side by side. */
function showView(view: "list" | "editor" | "tags"): void {
  ui.layout.dataset["view"] = view;
  ui.back.hidden = view === "list";
}

/** Search, smart folders and tags above the note list; picking one filters the list. */
const sidebar = createSidebar(
  () => {
    if (ui.layout.dataset["view"] === "tags") showView("list");
    if (listMode === "trash") setListMode("notes");
    else void refreshList();
  },
  (message) => {
    setStatus(`Smart folders not changed: ${message}`);
  },
);

function vaultOpened(info: VaultInfo): void {
  ui.setup.hidden = true;
  ui.vaultName.textContent = info.name;
  ui.vaultHint.textContent = info.path;
  readOnly = info.readOnly;
  straysDismissed = "";
  ui.strays.hidden = true;
  ui.readOnly.hidden = readOnly === null;
  ui.readOnlyText.textContent = readOnly ?? "";
  ui.newNote.disabled = readOnly !== null;
  ui.empty.hidden = true;
  sidebar.reset();
  sidebar.setEnabled(true);
  void sidebar.refresh();
}

/** Shows a vault that was picked or restored, or asks whether to make the folder one. */
async function folderOpened(opened: Opened): Promise<void> {
  if (opened.kind === "setup") {
    showSetup(opened);
    return;
  }
  closeEditor();
  lastTrashed = null;
  ui.undoDelete.hidden = true;
  vaultOpened(opened);
  showView("list");
  await refreshList();
}

/** Asks before a folder without vault.yaml becomes a vault (format.md §1). */
function showSetup(folder: FolderSetup): void {
  const moves = folder.items.length > 0;
  let text = moves
    ? `“${folder.name}” is not a Schnipsle vault yet. To open it, Schnipsle moves everything ` +
      `in it (${String(folder.notes)} notes in ${String(folder.items.length)} items) into a ` +
      `new folder “notes” inside it. Nothing is changed or deleted, but other apps will find ` +
      `these files in “notes” from now on.`
    : `“${folder.name}” is not a Schnipsle vault yet. Make it one? Schnipsle adds the ` +
      `folders “notes” and “.notesapp” to it.`;
  if (folder.notDownloaded.length > 0) {
    text +=
      ` ${String(folder.notDownloaded.length)} items are still in iCloud only and are being ` +
      `downloaded; try again when they are here.`;
  }
  ui.setupText.textContent = text;
  ui.setupCreate.textContent = moves ? "Move into “notes” and open" : "Make it a vault";
  ui.setupItems.replaceChildren(
    ...[...folder.items, ...folder.notDownloaded.map((n) => `${n} (in iCloud only)`)].map(
      (name) => {
        const item = document.createElement("li");
        item.textContent = name;
        return item;
      },
    ),
  );
  ui.setupDetails.hidden = !moves && folder.notDownloaded.length === 0;
  ui.setup.hidden = false;
  ui.setupCreate.focus();
}

async function refreshList(): Promise<void> {
  if (listMode === "trash") {
    ui.listFilter.hidden = true;
    await showTrashList();
    return;
  }
  const filter = sidebar.filter();
  showFilter(filter);
  let snippets = new Map<string, SnippetPart[]>();
  /** Search hits in the trash (`in:trash`): restored from the Trash view, not opened. */
  let inTrash = new Set<string>();
  try {
    switch (filter.kind) {
      case "all":
        entries = await invoke<NoteEntry[]>("list_notes", { sort: sortOrder });
        break;
      case "tag":
      case "untagged":
        entries = await invoke<NoteEntry[]>("notes_in_tags", {
          keys: filter.kind === "tag" ? filter.keys : [],
          sort: sortOrder,
        });
        break;
      case "smart":
      case "search": {
        // Local days for created:/modified: (getTimezoneOffset is UTC minus local).
        const utcOffsetMinutes = -new Date().getTimezoneOffset();
        const hits =
          filter.kind === "smart"
            ? await invoke<SearchHit[]>("smart_folder_notes", {
                query: filter.folder.query,
                sort: filter.folder.sort,
                utcOffsetMinutes,
              })
            : await invoke<SearchHit[]>("search_notes", { query: filter.query, utcOffsetMinutes });
        entries = hits.map((h) => ({ ...h, available: true }));
        snippets = new Map(hits.map((h) => [h.path, h.snippet]));
        inTrash = new Set(hits.filter((h) => h.inTrash).map((h) => h.path));
        break;
      }
    }
  } catch (e) {
    const err = asError(e);
    entries = [];
    ui.notes.replaceChildren();
    ui.empty.hidden = false;
    ui.empty.textContent =
      err.kind === "noIndex" ? "The index is being built; try again in a moment." : err.message;
    return;
  }
  ui.notes.replaceChildren(
    ...entries.map((note) => {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = note.title;
      if (note.pinned) button.classList.add("pinned");
      if (!note.available) {
        button.classList.add("cloud");
        button.title = "Stored in iCloud only – tap to download";
      }
      if (note.path === current?.path) button.setAttribute("aria-current", "true");
      if (inTrash.has(note.path)) {
        button.classList.add("trashed");
        button.title = "In the trash – restore it from the Trash view";
        button.addEventListener("click", () => {
          setStatus(`“${note.title}” is in the trash. Restore it from the Trash view to open it.`);
        });
      } else {
        button.addEventListener("click", () => void openNote(note));
      }
      item.append(button);
      const snippet = snippets.get(note.path);
      if (snippet && snippet.length > 0) {
        item.append(snippetElement(snippet));
      } else {
        const meta = document.createElement("small");
        meta.textContent = `${note.path} · ${new Date(note.modifiedMs).toLocaleString()}`;
        item.append(meta);
      }
      return item;
    }),
  );
  ui.empty.hidden = entries.length > 0;
  if (entries.length === 0) {
    ui.empty.textContent =
      filter.kind === "search"
        ? "Nothing found."
        : filter.kind === "all"
          ? "No notes in this folder yet. Tap New."
          : "No notes here.";
  }
  await checkStrays();
}

/** The line above the list saying what it shows, with "Show all". */
function showFilter(filter: ListFilter): void {
  // Search results come best first, a smart folder has its own order.
  ui.sort.hidden = filter.kind === "search" || filter.kind === "smart";
  ui.listFilter.hidden = filter.kind === "all";
  ui.listFilterText.textContent =
    filter.kind === "tag"
      ? filter.label
      : filter.kind === "untagged"
        ? "Untagged notes"
        : filter.kind === "search"
          ? `Search: ${filter.query}`
          : filter.kind === "smart"
            ? filter.folder.name
            : "";
}

/** A search snippet with the matching words emphasized. */
function snippetElement(parts: SnippetPart[]): HTMLElement {
  const small = document.createElement("small");
  small.className = "snippet";
  for (const part of parts) {
    if (part.hit) {
      const mark = document.createElement("mark");
      mark.textContent = part.text;
      small.append(mark);
    } else {
      small.append(part.text);
    }
  }
  return small;
}

/**
 * Offers to move notes that are outside notes/ and therefore not listed, e.g.
 * written by an older build on another device (format.md §1).
 */
async function checkStrays(): Promise<void> {
  let strays: StrayNotes;
  try {
    strays = await invoke<StrayNotes>("stray_notes");
  } catch {
    return; // the list itself is shown; this is only a hint
  }
  const all = [...strays.items, ...strays.notDownloaded.map((n) => `${n} (in iCloud only)`)];
  straysShown = all.join("\n");
  if (all.length === 0 || readOnly !== null || straysShown === straysDismissed) {
    ui.strays.hidden = true;
    return;
  }
  const count = strays.notes + strays.notDownloaded.length;
  let text =
    `${count === 1 ? "1 note is" : `${String(count)} notes are`} outside the “notes” folder ` +
    `of this vault, so ${count === 1 ? "it is" : "they are"} not listed. An older version of ` +
    `Schnipsle on another device may have saved them there.`;
  if (strays.notDownloaded.length > 0) {
    text += ` Some are still in iCloud only; they are moved once they are downloaded.`;
  }
  ui.straysText.textContent = text;
  ui.straysItems.replaceChildren(
    ...all.map((name) => {
      const item = document.createElement("li");
      item.textContent = name;
      return item;
    }),
  );
  ui.straysMove.disabled = strays.items.length === 0;
  ui.strays.hidden = false;
}

async function showTrashList(): Promise<void> {
  const trashed = await invoke<TrashedNote[]>("list_trash");
  ui.notes.replaceChildren(
    ...trashed.map((note) => {
      const item = document.createElement("li");
      const title = document.createElement("strong");
      title.textContent = note.title;
      const meta = document.createElement("small");
      const when = note.deleted === "" ? "" : new Date(note.deleted).toLocaleString();
      meta.textContent = `Deleted ${when} · was ${note.originalPath || "?"}`;
      const restore = document.createElement("button");
      restore.type = "button";
      restore.className = "small restore";
      restore.textContent = "Restore";
      restore.disabled = readOnly !== null;
      restore.addEventListener("click", () => void restoreNote(note));
      item.append(title, meta, restore);
      return item;
    }),
  );
  ui.empty.hidden = trashed.length > 0;
  if (trashed.length === 0) ui.empty.textContent = "The trash is empty.";
}

async function restoreNote(note: TrashedNote): Promise<void> {
  try {
    const path = await invoke<string>("restore_note", { key: note.key });
    setStatus(`Restored “${note.title}” to ${path}.`);
    if (lastTrashed?.key === note.key) {
      lastTrashed = null;
      ui.undoDelete.hidden = true;
    }
    await refreshList();
  } catch (e) {
    setStatus(`Not restored: ${asError(e).message}`);
  }
}

/** Shows the notes or the trash in the list. */
function setListMode(mode: "notes" | "trash"): void {
  listMode = mode;
  ui.showTrash.textContent = mode === "trash" ? "Notes" : "Trash";
  ui.showTrash.setAttribute("aria-pressed", String(mode === "trash"));
  const kind = sidebar.filter().kind;
  ui.sort.hidden = mode === "trash" || kind === "search" || kind === "smart";
  void refreshList();
}

/**
 * Saves, then renames the note's file if its title changed while it was open
 * (format.md §3). Runs whenever the user leaves a note, never while typing.
 */
function leaveNote(): Promise<void> {
  saving = saving.then(save).then(async () => {
    const note = current;
    if (!note || readOnly !== null || conflictHash !== undefined || isDirty()) return;
    try {
      const path = await invoke<string>("rename_after_edit", {
        path: note.path,
        titleAtOpen: note.titleAtOpen,
      });
      if (path !== note.path) {
        note.path = path;
        setStatus(`Renamed to ${path}`);
      }
    } catch (e) {
      setStatus(`Not renamed: ${asError(e).message}`);
    }
    await snapshotIfChanged();
  });
  return saving;
}

/** Empties the editor: no note open. */
function closeEditor(): void {
  closeHistory();
  ui.showHistory.disabled = true;
  window.clearTimeout(snapshotTimer);
  changedSinceSnapshot = false;
  current = null;
  images.forget();
  editor.load("");
  editor.setEnabled(false);
  ui.deleteNote.disabled = true;
  ui.pin.disabled = true;
  showPinned();
  ui.addImage.disabled = true;
  hideConflict();
  showSyncConflicts([]);
  ui.tags.textContent = "";
}

async function showTags(): Promise<void> {
  const tags = await invoke<Tag[]>("extract_tags", { markdown: editor.text() });
  ui.tags.textContent = tags.map((t) => `#${t.name}`).join(" ");
}

/** The open note's tags when the tag tree was last loaded. */
let tagsInTree = "";

/** After a save: reloads the tag tree if the note's tags changed. */
function refreshTagTreeIfNeeded(): void {
  const tags = ui.tags.textContent;
  if (tags !== tagsInTree) {
    tagsInTree = tags;
    void sidebar.refresh();
  }
}

async function openNote(note: NoteEntry): Promise<void> {
  await leaveNote();
  if (!note.available) {
    try {
      await invoke("download_note", { path: note.path });
      setStatus(`Downloading “${note.title}” from iCloud…`);
    } catch (e) {
      setStatus(asError(e).message);
    }
    return;
  }
  try {
    const content = await invoke<NoteContent>("read_note", { path: note.path });
    load(content);
    showView("editor");
  } catch (e) {
    const err = asError(e);
    setStatus(
      err.kind === "notDownloaded" ? "Not downloaded yet – try again shortly." : err.message,
    );
  }
}

function load(content: NoteContent): void {
  closeHistory();
  ui.showHistory.disabled = false;
  window.clearTimeout(snapshotTimer);
  changedSinceSnapshot = false;
  images.forget();
  current = {
    path: content.path,
    hash: content.hash,
    savedText: content.text,
    titleAtOpen: content.title,
  };
  editor.load(content.text);
  editor.setEnabled(readOnly === null);
  ui.deleteNote.disabled = readOnly !== null;
  ui.pin.disabled = readOnly !== null;
  ui.addImage.disabled = readOnly !== null;
  showPinned();
  hideConflict();
  showSyncConflicts(content.conflicts);
  setStatus(
    content.conflictsError === null
      ? `Opened ${content.path}`
      : `Opened ${content.path}, but could not check for sync conflicts: ${content.conflictsError}`,
  );
  void showTags();
  void refreshList();
}

/**
 * Takes in what a save wrote. The core adds or updates front matter only, so
 * the editor swaps in the new block, outside the undo history, as long as the
 * user has not edited the block meanwhile. Typing since the save started is
 * kept; the cursor moves with the text. If anything but the front matter
 * differs, the editor is left alone and the next save carries its text.
 */
function applySaved(
  note: { hash: string; savedText: string },
  sent: string,
  saved: SavedNote,
): void {
  note.hash = saved.hash;
  note.savedText = saved.text;
  if (note !== current || saved.text === sent) return;
  const oldBlock = sent.slice(0, frontMatterLength(sent));
  const newBlock = saved.text.slice(0, frontMatterLength(saved.text));
  if (saved.text.slice(newBlock.length) !== sent.slice(oldBlock.length)) return;
  editor.replaceFrontMatter(oldBlock, newBlock);
  showPinned();
}

/** The Pin button shows whether the open note is pinned, as its text says. */
function showPinned(): void {
  const pinned = current !== null && isPinned(editor.text());
  ui.pin.setAttribute("aria-pressed", String(pinned));
  ui.pin.textContent = pinned ? "Unpin" : "Pin";
}

/**
 * Pins or unpins the open note after saving it. The core changes only the
 * front matter, which the editor takes in like after a save.
 */
async function togglePin(): Promise<void> {
  const note = current;
  if (!note || readOnly !== null || conflictHash !== undefined || isDirty()) return;
  const sent = note.savedText;
  const pinned = !isPinned(sent);
  try {
    const saved = await invoke<SavedNote>("set_pinned", {
      path: note.path,
      pinned,
      expectedHash: note.hash,
    });
    applySaved(note, sent, saved);
    setStatus(pinned ? "Pinned – listed first." : "Unpinned.");
    void refreshList();
  } catch (e) {
    const err = asError(e);
    if (err.kind === "conflict") {
      showConflict(err.currentHash);
    } else {
      setStatus(`Not ${pinned ? "pinned" : "unpinned"}: ${err.message}`);
    }
  }
}

async function save(): Promise<void> {
  if (!current || !isDirty() || conflictHash !== undefined) return;
  const note = current;
  const text = editor.text();
  try {
    const saved = await invoke<SavedNote>("save_note", {
      path: note.path,
      text,
      expectedHash: note.hash,
    });
    applySaved(note, text, saved);
    setStatus(`Saved ${new Date().toLocaleTimeString()}`);
    refreshTagTreeIfNeeded();
  } catch (e) {
    const err = asError(e);
    if (err.kind === "conflict") {
      showConflict(err.currentHash);
    } else {
      setStatus(`Not saved: ${err.message}`);
    }
  }
}

function scheduleSave(): void {
  window.clearTimeout(saveTimer);
  saveTimer = window.setTimeout(() => {
    saving = saving.then(save);
  }, AUTOSAVE_MS);
}

/** Set by edits since the open note's last snapshot. */
let changedSinceSnapshot = false;
let snapshotTimer: number | undefined;

/**
 * Stores a version of the open note in its history (ADR-0011): `text` if
 * given (a version about to be replaced), else the note as saved. The core
 * skips a version the history already ends with.
 */
async function snapshot(text: string | null = null): Promise<void> {
  const note = current;
  if (!note || readOnly !== null) return;
  try {
    await invoke("snapshot_note", { path: note.path, text });
  } catch (e) {
    setStatus(`Version not stored in the history: ${asError(e).message}`);
  }
}

/** After a typing pause: saves, then stores the note in its history. */
function scheduleSnapshot(): void {
  changedSinceSnapshot = true;
  window.clearTimeout(snapshotTimer);
  snapshotTimer = window.setTimeout(() => {
    saving = saving.then(save).then(snapshotIfChanged);
  }, HISTORY_PAUSE_MS);
}

/** Stores the saved note in its history if it was edited since the last time. */
async function snapshotIfChanged(): Promise<void> {
  window.clearTimeout(snapshotTimer);
  if (!changedSinceSnapshot || isDirty() || conflictHash !== undefined) return;
  changedSinceSnapshot = false;
  await snapshot();
}

async function flushSave(): Promise<void> {
  window.clearTimeout(saveTimer);
  saving = saving.then(save);
  await saving;
}

function showConflict(hashOnDisk: string | null): void {
  conflictHash = hashOnDisk;
  ui.conflict.hidden = false;
  setStatus(
    hashOnDisk === null ? "The note was deleted elsewhere." : "Conflict – choose a version.",
  );
}

function hideConflict(): void {
  conflictHash = undefined;
  ui.conflict.hidden = true;
}

/** Shows the first conflict version a sync service kept for the open note, if any. */
function showSyncConflicts(conflicts: ConflictVersion[]): void {
  syncConflict = conflicts[0];
  ui.syncConflict.hidden = syncConflict === undefined;
  if (!syncConflict) return;
  const when = new Date(syncConflict.modifiedMs).toLocaleString();
  const more = conflicts.length > 1 ? ` (1 of ${String(conflicts.length)})` : "";
  ui.syncConflictText.textContent =
    syncConflict.copyPath === null
      ? `${syncConflict.device ?? "Another device"} saved a different version at ${when}${more}.`
      : `“${syncConflict.copyPath}” is a copy of this note with different text (saved ${when})${more}.`;
  ui.syncKeepBoth.hidden = syncConflict.copyPath === null;
  ui.syncOther.value = syncConflict.text;
}

/** Keeps `text` as the note; every other version goes into the note's history. */
function resolveSyncConflict(pick: "mine" | "other"): void {
  saving = saving.then(save).then(async () => {
    if (!current || !syncConflict) return;
    const text = pick === "mine" ? editor.text() : syncConflict.text;
    try {
      const content = await invoke<NoteContent>("resolve_conflict", {
        path: current.path,
        text,
        expectedHash: current.hash,
      });
      load(content);
      setStatus("Conflict resolved – the other version is kept in the note's history.");
      await refreshList();
    } catch (e) {
      const err = asError(e);
      setStatus(`Not resolved: ${err.message}`);
      if (err.kind === "conflict") queueCheck();
    }
  });
}

ui.syncKeepMine.addEventListener("click", () => {
  resolveSyncConflict("mine");
});
ui.syncUseOther.addEventListener("click", () => {
  resolveSyncConflict("other");
});
ui.syncKeepBoth.addEventListener("click", () => {
  saving = saving.then(save).then(async () => {
    const copyPath = syncConflict?.copyPath;
    if (!current || !copyPath) return;
    try {
      load(await invoke<NoteContent>("keep_both", { path: current.path, copyPath }));
      setStatus(`Kept both: “${copyPath}” is now a separate note.`);
      await refreshList();
    } catch (e) {
      setStatus(`Not changed: ${asError(e).message}`);
    }
  });
});

ui.useTheirs.addEventListener("click", () => {
  if (!current) return;
  const path = current.path;
  // The text in the editor goes into the history, never just away (ADR-0011).
  snapshot(editor.text())
    .then(() => invoke<NoteContent>("read_note", { path }))
    .then(load)
    .catch((e: unknown) => {
      setStatus(asError(e).message);
    });
});

ui.keepMine.addEventListener("click", () => {
  if (!current) return;
  const note = current;
  const text = editor.text();
  const expectedHash = conflictHash;
  // The other version on disk goes into the history before it is replaced.
  const write =
    expectedHash === null || expectedHash === undefined
      ? invoke<SavedNote>("create_note", { path: note.path, text })
      : snapshot().then(() =>
          invoke<SavedNote>("save_note", { path: note.path, text, expectedHash }),
        );
  write
    .then((saved) => {
      applySaved(note, text, saved);
      hideConflict();
      setStatus("Saved your version.");
    })
    .catch((e: unknown) => {
      setStatus(asError(e).message);
    });
});

/**
 * Picks up changes made on other devices or in other editors. `changes` comes
 * from a `vault-changed` event; without it (the app became visible again, and
 * events may have been missed) the open note is always re-checked.
 */
async function checkForChanges(changes?: VaultChanges): Promise<void> {
  if (ui.newNote.disabled) return; // no vault open
  try {
    await refreshList();
    if (!changes || changes.rescan || changes.paths.length > 0 || changes.smartFolders) {
      void sidebar.refresh();
    }
    if (!current || conflictHash !== undefined) return;
    const path = current.path;
    if (changes && !changes.rescan && !changes.paths.includes(path)) return;
    const onDisk = entries.find((n) => n.path === path);
    if (sidebar.filter().kind === "all") {
      // Only the full list tells whether the note is gone; a filtered one may just not show it.
      if (!onDisk) {
        setStatus("This note was deleted or moved elsewhere.");
        return;
      }
      if (!onDisk.available) return;
    }
    const content = await invoke<NoteContent>("read_note", { path });
    showSyncConflicts(content.conflicts);
    if (content.hash === current.hash) return;
    if (content.text === editor.text()) {
      // Same text as in the editor: the echo of our own save, or an identical edit.
      current.hash = content.hash;
      current.savedText = content.text;
      return;
    }
    if (isDirty()) {
      showConflict(content.hash);
    } else {
      await snapshot(current.savedText); // the version being replaced (ADR-0011)
      load(content);
      setStatus("Updated with changes from elsewhere.");
    }
  } catch (e) {
    setStatus(asError(e).message);
  }
}

ui.back.addEventListener("click", () => {
  if (ui.layout.dataset["view"] === "tags") {
    showView("list");
    return;
  }
  void leaveNote().then(async () => {
    showView("list");
    await refreshList();
  });
});

ui.sort.addEventListener("change", () => {
  sortOrder = ui.sort.value === "title" ? "title" : "modified";
  try {
    localStorage.setItem("schnipsle.sort", sortOrder);
  } catch {
    // storage unavailable: the choice lasts until the app closes
  }
  void refreshList();
});
try {
  if (localStorage.getItem("schnipsle.sort") === "title") sortOrder = "title";
} catch {
  // storage unavailable
}
ui.sort.value = sortOrder;

ui.showTrash.addEventListener("click", () => {
  setListMode(listMode === "trash" ? "notes" : "trash");
});

ui.pin.addEventListener("click", () => {
  window.clearTimeout(saveTimer);
  saving = saving.then(save).then(togglePin);
});

ui.deleteNote.addEventListener("click", () => {
  saving = saving.then(save).then(async () => {
    const note = current;
    if (!note) return;
    if (conflictHash !== undefined || isDirty()) {
      setStatus("Not deleted: resolve the conflict first, so no text is lost.");
      return;
    }
    try {
      const trashed = await invoke<TrashedNote>("trash_note", { path: note.path });
      closeEditor();
      lastTrashed = trashed;
      ui.undoDelete.hidden = false;
      setStatus(`Moved “${trashed.title}” to the trash.`);
      showView("list");
      await refreshList();
    } catch (e) {
      setStatus(`Not deleted: ${asError(e).message}`);
    }
  });
});

// --- History of the open note (ADR-0011) ---

let selectedVersion: Version | null = null;

const versionLabel = (v: Version): string =>
  v.takenMs === null ? v.name : new Date(v.takenMs).toLocaleString();

function closeHistory(): void {
  selectedVersion = null;
  ui.history.hidden = true;
  ui.editorHost.hidden = false;
  ui.showHistory.setAttribute("aria-pressed", "false");
}

/** Shows the open note's versions in place of the editor, newest first. */
async function openHistory(): Promise<void> {
  await flushSave();
  const note = current;
  if (!note) return;
  let versions: Version[];
  try {
    versions = await invoke<Version[]>("list_versions", { path: note.path });
  } catch (e) {
    setStatus(`History not available: ${asError(e).message}`);
    return;
  }
  if (current !== note) return;
  ui.editorHost.hidden = true;
  ui.history.hidden = false;
  ui.showHistory.setAttribute("aria-pressed", "true");
  ui.historyHint.textContent =
    versions.length === 0
      ? "No earlier versions yet. One is stored after a pause in typing and when you leave the note."
      : `${String(versions.length)} earlier ${versions.length === 1 ? "version" : "versions"}`;
  historyView.load("");
  ui.historyRestore.disabled = true;
  ui.historyCopy.disabled = true;
  const buttons = versions.map((version) => {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = versionLabel(version);
    button.addEventListener("click", () => void selectVersion(version, button));
    return button;
  });
  ui.historyList.replaceChildren(
    ...buttons.map((button) => {
      const item = document.createElement("li");
      item.append(button);
      return item;
    }),
  );
  const [newest] = versions;
  const [first] = buttons;
  if (newest && first) await selectVersion(newest, first);
}

async function selectVersion(version: Version, button: HTMLButtonElement): Promise<void> {
  const note = current;
  if (!note) return;
  for (const other of ui.historyList.querySelectorAll("button")) {
    other.removeAttribute("aria-current");
  }
  button.setAttribute("aria-current", "true");
  try {
    const text = await invoke<string>("read_version", { path: note.path, name: version.name });
    if (current !== note) return;
    selectedVersion = version;
    historyView.load(text);
    ui.historyRestore.disabled = readOnly !== null;
    ui.historyCopy.disabled = readOnly !== null;
  } catch (e) {
    historyView.load(`This version cannot be read: ${asError(e).message}`);
    selectedVersion = null;
    ui.historyRestore.disabled = true;
    ui.historyCopy.disabled = true;
  }
}

ui.showHistory.addEventListener("click", () => {
  if (ui.history.hidden) void openHistory();
  else closeHistory();
});
ui.historyClose.addEventListener("click", closeHistory);

ui.historyRestore.addEventListener("click", () => {
  const note = current;
  const version = selectedVersion;
  if (!note || !version) return;
  saving = saving.then(save).then(async () => {
    if (isDirty() || conflictHash !== undefined) {
      setStatus("Not restored: the note has unsaved changes or a conflict.");
      return;
    }
    try {
      await invoke<SavedNote>("restore_version", {
        path: note.path,
        name: version.name,
        expectedHash: note.hash,
      });
      const content = await invoke<NoteContent>("read_note", { path: note.path });
      load(content);
      setStatus(
        `Restored the version of ${versionLabel(version)}; the text before is in the history.`,
      );
    } catch (e) {
      setStatus(`Not restored: ${asError(e).message}`);
    }
  });
});

ui.historyCopy.addEventListener("click", () => {
  const note = current;
  const version = selectedVersion;
  if (!note || !version) return;
  void leaveNote()
    .then(() => invoke<NewNote>("copy_version", { path: note.path, name: version.name }))
    .then(async (created) => {
      const content = await invoke<NoteContent>("read_note", { path: created.path });
      if (listMode === "trash") setListMode("notes");
      load(content);
      setStatus(`Copied the version of ${versionLabel(version)} as a new note.`);
      await refreshList();
    })
    .catch((e: unknown) => {
      setStatus(`Not copied: ${asError(e).message}`);
    });
});

ui.addImage.addEventListener("click", () => {
  ui.imageFile.click();
});
ui.imageFile.addEventListener("change", () => {
  const files = Array.from(ui.imageFile.files ?? []);
  ui.imageFile.value = "";
  if (files.length > 0) void attachFiles(files, null);
});

ui.undoDelete.addEventListener("click", () => {
  const trashed = lastTrashed;
  if (!trashed) return;
  void restoreNote(trashed);
});

ui.pick.addEventListener("click", () => {
  void leaveNote()
    .then(() => invoke<Opened | null>("pick_vault"))
    .then(async (opened) => {
      if (opened) await folderOpened(opened);
    })
    .catch((e: unknown) => {
      setStatus(asError(e).message);
    });
});

ui.setupCreate.addEventListener("click", () => {
  ui.setupCreate.disabled = true;
  void leaveNote()
    .then(() => invoke<VaultInfo>("create_vault"))
    .then(async (info) => {
      await folderOpened({ kind: "vault", ...info });
    })
    .catch((e: unknown) => {
      setStatus(`Not opened: ${asError(e).message}`);
    })
    .finally(() => {
      ui.setupCreate.disabled = false;
    });
});

ui.straysMove.addEventListener("click", () => {
  ui.straysMove.disabled = true;
  invoke<MovedNotes>("move_stray_notes")
    .then(async (result) => {
      const n = result.moved.length;
      let message = `Moved ${n === 1 ? "1 item" : `${String(n)} items`} into “notes”.`;
      if (result.relinked.length > 0) {
        message += ` Updated the links in ${String(result.relinked.length)} of them.`;
      }
      if (result.skipped.length > 0) {
        message += ` Not moved: ${result.skipped.join(", ")} (name taken or not downloaded yet).`;
      }
      setStatus(message);
      await refreshList();
    })
    .catch((e: unknown) => {
      setStatus(`Not moved: ${asError(e).message}`);
      ui.straysMove.disabled = false;
    });
});

ui.straysLater.addEventListener("click", () => {
  straysDismissed = straysShown;
  ui.strays.hidden = true;
});

ui.setupCancel.addEventListener("click", () => {
  ui.setup.hidden = true;
  setStatus("Folder not opened; nothing was changed.");
});

ui.newNote.addEventListener("click", () => {
  const text = "# New note\n\n";
  void leaveNote()
    .then(() => invoke<NewNote>("new_note", { text }))
    .then((created) => {
      if (listMode === "trash") setListMode("notes");
      load({
        path: created.path,
        title: "New note",
        ...created.saved,
        conflicts: [],
        conflictsError: null,
      });
      showView("editor");
      editor.focus();
      editor.cursorToEnd();
    })
    .catch((e: unknown) => {
      setStatus(asError(e).message);
    });
});

// Live preview or plain Markdown source (ADR-0007); remembered on this device.
const MODE_KEY = "schnipsle.editorMode";
function setMode(mode: EditorMode): void {
  editor.setMode(mode);
  historyView.setMode(mode);
  ui.mode.textContent = mode === "live" ? "Source" : "Preview";
  ui.mode.setAttribute("aria-pressed", String(mode === "source"));
  try {
    localStorage.setItem(MODE_KEY, mode);
  } catch {
    // storage unavailable: the choice lasts until the app closes
  }
}
ui.mode.addEventListener("click", () => {
  setMode(ui.mode.getAttribute("aria-pressed") === "true" ? "live" : "source");
});
let savedMode: string | null = null;
try {
  savedMode = localStorage.getItem(MODE_KEY);
} catch {
  // storage unavailable
}
setMode(savedMode === "source" ? "source" : "live");

// File menu (desktop): same actions as the buttons.
void listen<string>("menu", (event) => {
  if (event.payload === "open-vault") ui.pick.click();
  else if (event.payload === "new-note" && !ui.newNote.disabled) ui.newNote.click();
});

/** Runs a check after any save in progress, so a save's own echo is not mistaken for a change. */
function queueCheck(changes?: VaultChanges): void {
  saving = saving.then(() => checkForChanges(changes));
}

void listen<VaultChanges>("vault-changed", (event) => {
  queueCheck(event.payload);
});

// The index was (re)built: tags and filtered lists can be shown now.
void listen("index-synced", () => {
  void sidebar.refresh();
  if (sidebar.filter().kind !== "all") void refreshList();
});

ui.showTags.addEventListener("click", () => {
  sidebar.openTags();
  showView("tags");
});

ui.clearFilter.addEventListener("click", () => {
  sidebar.showAll();
});

document.addEventListener("visibilitychange", () => {
  if (document.visibilityState === "visible") queueCheck();
  else void leaveNote();
});

invoke<Opened | null>("restore_vault")
  .then(async (opened) => {
    if (opened) await folderOpened(opened);
  })
  .catch((e: unknown) => {
    setStatus(asError(e).message);
  });
