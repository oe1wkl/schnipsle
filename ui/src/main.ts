// SPDX-License-Identifier: Apache-2.0
// Spike UI: open a vault folder, list notes, edit with autosave in the
// CodeMirror editor (ADR-0007), react to changes made elsewhere (`vault-changed`
// events, ADR-0015) and resolve conflicts.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { createEditor, type EditorMode, frontMatterLength } from "./editor";

interface VaultInfo {
  name: string;
  path: string;
  /** Why the vault is read-only (format.md §2), if it is. */
  readOnly: string | null;
}
/** A picked folder that is not a vault yet (src-tauri `Opened::Setup`). */
interface FolderSetup {
  name: string;
  path: string;
  /** Visible items at its top level that would move into notes/. */
  items: string[];
  notes: number;
  /** Items only in iCloud; their download was requested. */
  notDownloaded: string[];
}
/** Notes outside notes/, e.g. from an older build (schnipsle-core `StrayNotes`). */
interface StrayNotes {
  items: string[];
  notes: number;
  notDownloaded: string[];
}
/** What moving them did (schnipsle-core `MovedNotes`). */
interface MovedNotes {
  moved: string[];
  skipped: string[];
  /** Notes whose links were rewritten to keep working (ADR-0016). */
  relinked: string[];
}
/** Result of picking or restoring a folder (src-tauri `Opened`). */
type Opened = ({ kind: "vault" } & VaultInfo) | ({ kind: "setup" } & FolderSetup);
interface NoteEntry {
  path: string;
  title: string;
  modifiedMs: number;
  available: boolean;
}
/** A version a sync service kept because it conflicts (schnipsle-core `ConflictVersion`). */
interface ConflictVersion {
  text: string;
  hash: string;
  modifiedMs: number;
  device: string | null;
  /** Set for another file with the same id, e.g. iCloud's copy `Name 2.md`. */
  copyPath: string | null;
}
interface NoteContent {
  path: string;
  title: string;
  text: string;
  hash: string;
  conflicts: ConflictVersion[];
  conflictsError: string | null;
}
interface Tag {
  name: string;
  key: string;
}
/** Payload of the `vault-changed` event (schnipsle-core `VaultChanges`). */
interface VaultChanges {
  paths: string[];
  rescan: boolean;
}
/** What a save wrote: the text on disk, with the front matter the core maintains. */
interface SavedNote {
  text: string;
  hash: string;
}
/** A note in the trash (schnipsle-core `TrashedNote`). */
interface TrashedNote {
  key: string;
  title: string;
  originalPath: string;
  deleted: string;
}
/** A stored attachment (schnipsle-core `AddedAttachment`). */
interface AddedAttachment {
  path: string;
  link: string;
  markdown: string;
}
interface NewNote {
  path: string;
  saved: SavedNote;
}
type SortOrder = "modified" | "title";
interface CommandError {
  kind: string;
  message: string;
  currentHash: string | null;
}

const AUTOSAVE_MS = 1000;

function el(id: string): HTMLElement {
  const found = document.getElementById(id);
  if (!found) throw new Error(`missing #${id}`);
  return found;
}

const ui = {
  layout: document.querySelector<HTMLElement>(".layout") ?? document.body,
  back: el("back") as HTMLButtonElement,
  vaultName: el("vault-name"),
  vaultHint: el("vault-hint"),
  pick: el("pick-vault") as HTMLButtonElement,
  newNote: el("new-note") as HTMLButtonElement,
  empty: el("empty"),
  notes: el("notes") as HTMLUListElement,
  editorHost: el("editor-host"),
  mode: el("mode") as HTMLButtonElement,
  sort: el("sort") as HTMLSelectElement,
  showTrash: el("show-trash") as HTMLButtonElement,
  deleteNote: el("delete-note") as HTMLButtonElement,
  addImage: el("add-image") as HTMLButtonElement,
  imageFile: el("image-file") as HTMLInputElement,
  undoDelete: el("undo-delete") as HTMLButtonElement,
  status: el("status"),
  tags: el("tags"),
  conflict: el("conflict"),
  useTheirs: el("use-theirs") as HTMLButtonElement,
  keepMine: el("keep-mine") as HTMLButtonElement,
  syncConflict: el("sync-conflict"),
  syncConflictText: el("sync-conflict-text"),
  syncOther: el("sync-other") as HTMLTextAreaElement,
  syncKeepMine: el("sync-keep-mine") as HTMLButtonElement,
  syncUseOther: el("sync-use-other") as HTMLButtonElement,
  syncKeepBoth: el("sync-keep-both") as HTMLButtonElement,
  setup: el("setup"),
  setupText: el("setup-text"),
  setupCreate: el("setup-create") as HTMLButtonElement,
  setupCancel: el("setup-cancel") as HTMLButtonElement,
  setupDetails: el("setup-details") as HTMLDetailsElement,
  setupItems: el("setup-items") as HTMLUListElement,
  strays: el("strays"),
  straysText: el("strays-text"),
  straysMove: el("strays-move") as HTMLButtonElement,
  straysLater: el("strays-later") as HTMLButtonElement,
  straysItems: el("strays-items") as HTMLUListElement,
  readOnly: el("read-only"),
  readOnlyText: el("read-only-text"),
};

/**
 * The note in the editor. `savedText` is what is on disk (as far as we know);
 * `titleAtOpen` decides whether leaving the note renames its file.
 */
let current: { path: string; hash: string; savedText: string; titleAtOpen: string } | null = null;
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
    void showTags();
  },
  onBlur: () => void flushSave(),
  onFiles: (files, pos) => void attachFiles(files, pos),
  loadImage,
});

/** Object URLs of the images shown for the open note, by link. */
let images = new Map<string, Promise<string>>();

const IMAGE_TYPES: Record<string, string> = {
  avif: "image/avif",
  gif: "image/gif",
  heic: "image/heic",
  jpeg: "image/jpeg",
  jpg: "image/jpeg",
  png: "image/png",
  svg: "image/svg+xml",
  webp: "image/webp",
};

/** Loads an image the open note links to (format.md §5.5) for the live preview. */
function loadImage(url: string): Promise<string> {
  const note = current;
  if (!note) return Promise.reject(new Error("no note open"));
  let image = images.get(url);
  if (!image) {
    const ext = /\.([a-z0-9]+)(?:[?#]|$)/i.exec(url)?.[1]?.toLowerCase() ?? "";
    image = invoke<ArrayBuffer>("read_linked_file", { notePath: note.path, link: url }).then(
      (bytes) => URL.createObjectURL(new Blob([bytes], { type: IMAGE_TYPES[ext] ?? "" })),
    );
    images.set(url, image);
  }
  return image;
}

/** Releases the images of the note that was open. */
function forgetImages(): void {
  for (const image of images.values()) {
    image.then(
      (url) => {
        URL.revokeObjectURL(url);
      },
      () => undefined,
    );
  }
  images = new Map();
}

const EXTENSION_OF_TYPE: Record<string, string> = {
  "image/gif": "gif",
  "image/heic": "heic",
  "image/jpeg": "jpg",
  "image/png": "png",
  "image/svg+xml": "svg",
  "image/webp": "webp",
};

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
    const name = file.name || `image.${EXTENSION_OF_TYPE[file.type] ?? "bin"}`;
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

function asError(e: unknown): CommandError {
  if (typeof e === "object" && e !== null && "kind" in e) return e as CommandError;
  return { kind: "other", message: String(e), currentHash: null };
}

function setStatus(message: string): void {
  ui.status.textContent = message;
}

function showView(view: "list" | "editor"): void {
  ui.layout.dataset["view"] = view;
  ui.back.hidden = view === "list";
}

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
    await showTrashList();
    return;
  }
  entries = await invoke<NoteEntry[]>("list_notes", { sort: sortOrder });
  ui.notes.replaceChildren(
    ...entries.map((note) => {
      const item = document.createElement("li");
      const button = document.createElement("button");
      button.type = "button";
      button.textContent = note.title;
      if (!note.available) {
        button.classList.add("cloud");
        button.title = "Stored in iCloud only – tap to download";
      }
      if (note.path === current?.path) button.setAttribute("aria-current", "true");
      button.addEventListener("click", () => void openNote(note));
      const meta = document.createElement("small");
      meta.textContent = `${note.path} · ${new Date(note.modifiedMs).toLocaleString()}`;
      item.append(button, meta);
      return item;
    }),
  );
  ui.empty.hidden = entries.length > 0;
  if (entries.length === 0) ui.empty.textContent = "No notes in this folder yet. Tap New.";
  await checkStrays();
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
  ui.sort.hidden = mode === "trash";
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
  });
  return saving;
}

/** Empties the editor: no note open. */
function closeEditor(): void {
  current = null;
  forgetImages();
  editor.load("");
  editor.setEnabled(false);
  ui.deleteNote.disabled = true;
  ui.addImage.disabled = true;
  hideConflict();
  showSyncConflicts([]);
  ui.tags.textContent = "";
}

async function showTags(): Promise<void> {
  const tags = await invoke<Tag[]>("extract_tags", { markdown: editor.text() });
  ui.tags.textContent = tags.map((t) => `#${t.name}`).join(" ");
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
  forgetImages();
  current = {
    path: content.path,
    hash: content.hash,
    savedText: content.text,
    titleAtOpen: content.title,
  };
  editor.load(content.text);
  editor.setEnabled(readOnly === null);
  ui.deleteNote.disabled = readOnly !== null;
  ui.addImage.disabled = readOnly !== null;
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
  invoke<NoteContent>("read_note", { path })
    .then(load)
    .catch((e: unknown) => {
      setStatus(asError(e).message);
    });
});

ui.keepMine.addEventListener("click", () => {
  if (!current) return;
  const note = current;
  const text = editor.text();
  const write =
    conflictHash === null || conflictHash === undefined
      ? invoke<SavedNote>("create_note", { path: note.path, text })
      : invoke<SavedNote>("save_note", { path: note.path, text, expectedHash: conflictHash });
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
    if (!current || conflictHash !== undefined) return;
    const path = current.path;
    if (changes && !changes.rescan && !changes.paths.includes(path)) return;
    const onDisk = entries.find((n) => n.path === path);
    if (!onDisk) {
      setStatus("This note was deleted or moved elsewhere.");
      return;
    }
    if (!onDisk.available) return;
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
      load(content);
      setStatus("Updated with changes from elsewhere.");
    }
  } catch (e) {
    setStatus(asError(e).message);
  }
}

ui.back.addEventListener("click", () => {
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
