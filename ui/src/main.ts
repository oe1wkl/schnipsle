// SPDX-License-Identifier: Apache-2.0
// Spike UI: open a vault folder, list notes, edit with autosave in the
// CodeMirror editor (ADR-0007), react to changes made elsewhere (`vault-changed`
// events, ADR-0015) and resolve conflicts.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { measured, noStart, started } from "./diagnostics";
import { asError, setStatus, ui } from "./dom";
import { createEditor, type EditorMode, frontMatterLength, isPinned } from "./editor";
import { createExport } from "./export";
import { createHistory, versionLabel } from "./history";
import { createImageCache, fileName } from "./images";
import { createImport } from "./import";
import { createImports } from "./imports";
import { createSettings } from "./settings";
import { createRetag } from "./retag";
import { createSidebar, type ListFilter } from "./sidebar";
import { roving } from "./keys";
import { musicMenu } from "./snippets";
import { t } from "./strings";
import type { SongHost } from "./song";
import type {
  FirstStart,
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
  NewNote,
  OtherUsed,
  SearchHit,
  SnippetPart,
  SortOrder,
  SongSheet,
  Notation,
} from "./types";

const AUTOSAVE_MS = 1000;
/** A typing pause this long stores a version in the history (ADR-0011). */
const HISTORY_PAUSE_MS = 30_000;

/**
 * The note in the editor. `savedText` is what is on disk (as far as we know);
 * `titleAtOpen` decides whether leaving the note renames its file.
 */
let current: { path: string; hash: string; savedText: string; titleAtOpen: string } | null = null;
/**
 * The note from the trash shown read-only in the editor (no note is open
 * then), with the path it had, so its images resolve as they did there.
 */
let trashShown: { key: string; path: string } | null = null;
const images = createImageCache(() => current?.path ?? trashShown?.path ?? null);
let listMode: "notes" | "trash" = "notes";
/** The list's search hits in the trash, by path; never exported (ADR-0021). */
let hitsInTrash = new Set<string>();
let sortOrder: SortOrder = "modified";
let lastTrashed: TrashedNote | null = null;
let conflictHash: string | null | undefined; // set while the conflict banner shows
/** Why the last save failed, other than a conflict; `null` after a save that worked. */
let saveProblem: string | null = null;
let syncConflict: ConflictVersion | undefined; // shown in the sync conflict banner
let entries: NoteEntry[] = [];
/** Counts list drawings; rows still to come of an older one are dropped. */
let listDrawing = 0;
/**
 * Where the note list was when the phone hid it for a note or the tags
 * (`showView`). WebKit keeps no layout for hidden rows, so the place is not
 * read from them while the list is hidden.
 */
let hiddenPlace: { rows: number; anchor: RowInView | null } | null = null;
/** The list is to be shown once its new rows are in (`showList`). */
let showWhenDrawn = false;
/** Rows of a list drawn at once, and per frame after that (ADR-0046). */
const FIRST_ROWS = 100;
const ROWS_PER_FRAME = 500;
/** The format of `toLocaleString()`, made once instead of for each row. */
const dateTime = new Intl.DateTimeFormat(undefined, {
  year: "numeric",
  month: "numeric",
  day: "numeric",
  hour: "numeric",
  minute: "numeric",
  second: "numeric",
});
let saveTimer: number | undefined;
let saving: Promise<void> = Promise.resolve();
/** Why the open vault is read-only; `null` if it is writable or none is open. */
let readOnly: string | null = null;
let vaultIsOpen = false;
/** The notes outside notes/ last shown, and those the user said "Not now" to. */
let straysShown = "";
let straysDismissed = "";

/** Song sheets for both editors: the core renders them (ADR-0030). */
const songs: SongHost = {
  render: (source, info, view) =>
    invoke<SongSheet>("render_song", {
      source,
      info,
      transpose: view.transpose,
      lyricsOnly: view.lyricsOnly,
      shown: view.shown,
    }),
  applyTransposition: (source, semitones, notation) =>
    applyTransposition(source, semitones, notation),
  report: setStatus,
};

const editor = createEditor(ui.editorHost, {
  onChange: () => {
    setStatus(t.editor.editing);
    scheduleSave();
    scheduleSnapshot();
    showPinned();
    void showTags();
  },
  onBlur: () => void flushSave(),
  onFiles: (files, pos) => void attachFiles(files, pos),
  loadImage: (url) => images.load(url),
  openLink: (link) => void openLink(link),
  songs,
});

// The note list is one Tab stop, with the arrow keys inside (ADR-0041 rule 7).
roving(ui.notes, { items: "li > button:first-child", row: "li" });

/**
 * The version selected in the history, read-only and in the same mode as the
 * editor, so Preview renders it like the note (images resolve from the note).
 */
const historyView = createEditor(ui.historyPreview, {
  onChange: () => undefined,
  onBlur: () => undefined,
  onFiles: () => undefined,
  loadImage: (url) => images.load(url),
  placeholderText: t.history.selectVersion,
  songs,
});

// --- History of the open note (ADR-0011, ADR-0038) ---

const noteHistory = createHistory({
  note: () => current?.path ?? null,
  flush: flushSave,
  readOnly: () => readOnly !== null,
  preview: historyView,
  status: setStatus,
  restore: (version) => {
    const note = current;
    if (!note) return;
    saving = saving.then(save).then(async () => {
      if (isDirty() || conflictHash !== undefined) {
        setStatus(t.history.notRestoredDirty);
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
        setStatus(t.history.restored(versionLabel(version)));
      } catch (e) {
        setStatus(t.history.notRestored(asError(e).message));
      }
    });
  },
  copy: (version) => {
    const note = current;
    if (!note) return;
    void leaveNote()
      .then(async (left) => {
        if (!left) return;
        const created = await invoke<NewNote>("copy_version", {
          path: note.path,
          name: version.name,
        });
        const content = await invoke<NoteContent>("read_note", { path: created.path });
        if (listMode === "trash") setListMode("notes");
        load(content);
        setStatus(t.history.copied(versionLabel(version)));
        await refreshList();
      })
      .catch((e: unknown) => {
        setStatus(t.history.notCopied(asError(e).message));
      });
  },
});

/**
 * Stores files as attachments (format.md §6) and inserts links to them at
 * `pos`, or in place of the selection.
 */
async function attachFiles(files: File[], pos: number | null): Promise<void> {
  const note = current;
  if (!note || readOnly !== null) {
    setStatus(note ? t.common.readOnly : t.note.openFirst);
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
      setStatus(t.note.notAttached(name, asError(e).message));
    }
  }
  if (links.length > 0 && current === note) editor.insert(links.join("\n"), pos);
}

const isDirty = (): boolean => current !== null && editor.text() !== current.savedText;

/** The view shown on the phone; on wider screens list and editor are side by side. */
function showView(view: "list" | "editor" | "tags"): void {
  // A view chosen since wins over a list still to be shown.
  showWhenDrawn = false;
  if (ui.layout.dataset["view"] === "list" && view !== "list" && ui.notes.offsetParent !== null) {
    hiddenPlace = { rows: rowsAtOnce(), anchor: rowInView() };
  }
  ui.layout.dataset["view"] = view;
  ui.back.hidden = view === "list";
}

/**
 * Shows the note list, drawn again. A list the phone hid for a note or the
 * tags is drawn while still hidden and shown once its new rows are in:
 * showing it first would make WebKit lay out every old row only to replace
 * them, about 100 ms for 10,000 rows on an iPhone 18 Pro.
 */
async function showList(): Promise<void> {
  if (ui.layout.dataset["view"] !== "list") showWhenDrawn = true;
  await refreshList();
}

/** Shows the list if `showList` asked for it. */
function revealList(): void {
  if (!showWhenDrawn) return;
  hiddenPlace = null;
  showView("list");
}

/** Search, smart folders and tags above the note list; picking one filters the list. */
const sidebar = createSidebar(
  () => {
    // Another list: the place in the one before does not apply.
    hiddenPlace = null;
    const fromTags = ui.layout.dataset["view"] === "tags";
    if (listMode === "trash") {
      if (fromTags) showView("list");
      setListMode("notes");
    } else if (fromTags) void showList();
    else void refreshList();
  },
  (message) => {
    setStatus(t.smart.notChanged(message));
  },
  {
    can: () => vaultIsOpen && readOnly === null,
    open: (name) => {
      retag.open(name);
    },
  },
);

/** Renaming and merging tags (F-11, ADR-0040). */
const retag = createRetag({
  // Saved first, as the rename reads the files.
  beforeChange: flushSave,
  changed: (from, to) => {
    sidebar.retagged(from, to);
    queueCheck();
  },
  status: setStatus,
});

function vaultOpened(info: VaultInfo): void {
  ui.setup.hidden = true;
  ui.firstStart.hidden = true;
  ui.vaultName.textContent = info.name;
  ui.vaultHint.textContent = info.path;
  readOnly = info.readOnly;
  vaultIsOpen = true;
  straysDismissed = "";
  ui.strays.hidden = true;
  ui.readOnly.hidden = readOnly === null;
  ui.readOnlyText.textContent = readOnly ?? "";
  ui.newNote.disabled = readOnly !== null;
  // The previous vault's notes go at once: listing a vault that is still
  // downloading can take a while, and their rows must not open in this one.
  listDrawing++;
  hiddenPlace = null;
  entries = [];
  hitsInTrash = new Set();
  ui.notes.style.paddingBottom = "";
  ui.notes.replaceChildren();
  ui.empty.textContent = t.list.reading;
  ui.empty.hidden = false;
  sidebar.reset();
  retag.reset();
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
  // Shown at once with "reading": its rows are gone, a slow listing should not look stuck.
  showView("list");
  await refreshList();
}

/** Asks before a folder without vault.yaml becomes a vault (format.md §1). */
function showSetup(folder: FolderSetup): void {
  const moves = folder.items.length > 0;
  let text = moves
    ? t.setup.moves(folder.name, folder.notes, folder.items.length)
    : t.setup.adds(folder.name);
  if (folder.notDownloaded.length > 0) text += t.setup.downloading(folder.notDownloaded.length);
  ui.setupText.textContent = text;
  ui.setupCreate.textContent = moves ? t.setup.createAndMove : t.setup.create;
  ui.setupItems.replaceChildren(
    ...[...folder.items, ...folder.notDownloaded.map(t.common.inICloudOnly)].map((name) => {
      const item = document.createElement("li");
      item.textContent = name;
      return item;
    }),
  );
  ui.setupDetails.hidden = !moves && folder.notDownloaded.length === 0;
  ui.setup.hidden = false;
  ui.setupCreate.focus();
}

/**
 * Rows of the note list to draw at once: a first screenful, or, when the list
 * is scrolled, all rows down to the bottom of the window, so that drawing it
 * again keeps its scroll position.
 */
function rowsAtOnce(): number {
  if (ui.notes.offsetParent === null) return Math.max(FIRST_ROWS, hiddenPlace?.rows ?? 0);
  const rows = ui.notes.children;
  const bottom = window.innerHeight;
  let low = 0;
  let high = rows.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if ((rows.item(middle)?.getBoundingClientRect().top ?? bottom) < bottom) low = middle + 1;
    else high = middle;
  }
  return Math.max(FIRST_ROWS, low);
}

/** A row of the note list and how far below the top of the list's view it is. */
interface RowInView {
  path: string;
  offset: number;
}

/**
 * The first row in view of a scrolled note list, other than the open note's,
 * which moves to the top when it is saved.
 */
function rowInView(): RowInView | null {
  if (ui.notes.offsetParent === null) return hiddenPlace?.anchor ?? null;
  const view = ui.notes.closest<HTMLElement>(".list");
  if (view === null || view.scrollTop === 0) return null;
  const top = view.getBoundingClientRect().top;
  const rows = ui.notes.children;
  let low = 0;
  let high = rows.length;
  while (low < high) {
    const middle = Math.floor((low + high) / 2);
    if ((rows.item(middle)?.getBoundingClientRect().bottom ?? top) <= top) low = middle + 1;
    else high = middle;
  }
  for (let i = low; i < Math.min(rows.length, low + 3); i++) {
    const row = rows.item(i);
    const path = row?.querySelector<HTMLElement>("button")?.dataset["path"];
    if (row && path !== undefined && path !== current?.path) {
      return { path, offset: row.getBoundingClientRect().top - top };
    }
  }
  return null;
}

/** Scrolls the note list so that `anchor` is where it was before drawing. */
function keepInView(anchor: RowInView | null): void {
  const view = ui.notes.closest<HTMLElement>(".list");
  if (anchor === null || view === null || ui.notes.offsetParent === null) return;
  const row = [...ui.notes.querySelectorAll<HTMLElement>("li > button:first-child")]
    .find((button) => button.dataset["path"] === anchor.path)
    ?.closest("li");
  if (!row) return;
  view.scrollTop +=
    row.getBoundingClientRect().top - view.getBoundingClientRect().top - anchor.offset;
}

async function refreshList(): Promise<void> {
  const drawing = ++listDrawing;
  if (listMode === "trash") {
    ui.listFilter.hidden = true;
    try {
      await showTrashList();
    } finally {
      revealList();
    }
    return;
  }
  const filter = sidebar.filter();
  showFilter(filter);
  let found: NoteEntry[] = [];
  let snippets = new Map<string, SnippetPart[]>();
  /** Search hits in the trash (`in:trash`): restored from the Trash view, not opened. */
  let inTrash = new Set<string>();
  // Timed until drawn (ADR-0045).
  const since = performance.now();
  const kinds = {
    all: "allNotesUi",
    tag: "notesInTagsUi",
    untagged: "notesInTagsUi",
    smart: "smartFolderUi",
    search: "searchUi",
  } as const;
  try {
    switch (filter.kind) {
      case "all":
        found = await invoke<NoteEntry[]>("list_notes", { sort: sortOrder });
        break;
      case "tag":
      case "untagged":
        found = await invoke<NoteEntry[]>("notes_in_tags", {
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
        found = hits.map((h) => ({ ...h, available: true }));
        snippets = new Map(hits.map((h) => [h.path, h.snippet]));
        inTrash = new Set(hits.filter((h) => h.inTrash).map((h) => h.path));
        break;
      }
    }
  } catch (e) {
    if (drawing !== listDrawing) return; // a newer drawing has started
    const err = asError(e);
    entries = [];
    hitsInTrash = inTrash;
    ui.notes.style.paddingBottom = "";
    ui.notes.replaceChildren();
    ui.empty.hidden = false;
    ui.empty.textContent = err.kind === "noIndex" ? t.list.indexing : err.message;
    revealList();
    return;
  }
  // Only the newest drawing is shown: an older one finishing later would
  // replace it with its own rows and then stop.
  if (drawing !== listDrawing) return;
  entries = found;
  hitsInTrash = inTrash;
  const row = (note: NoteEntry): HTMLLIElement => {
    const item = document.createElement("li");
    const button = document.createElement("button");
    button.type = "button";
    button.dataset["path"] = note.path;
    button.textContent = note.title;
    if (note.pinned) button.classList.add("pinned");
    if (!note.available) {
      button.classList.add("cloud");
      button.title = t.list.inICloud;
    }
    if (note.path === current?.path) button.setAttribute("aria-current", "true");
    if (inTrash.has(note.path)) {
      button.classList.add("trashed");
      button.title = t.list.inTrash;
      button.addEventListener("click", () => {
        setStatus(t.list.inTrashStatus(note.title));
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
      meta.textContent = `${note.path} · ${dateTime.format(note.modifiedMs)}`;
      item.append(meta);
    }
    return item;
  };
  // A long list shows its first rows at once and the rest frame by frame. The
  // rows still to come are kept as space below the list, so its height and
  // the scroll bar stay as they will be.
  const shown = entries;
  const first = rowsAtOnce();
  const anchor = rowInView();
  ui.notes.style.paddingBottom = "";
  ui.notes.replaceChildren(...shown.slice(0, first).map(row));
  // Only now, with the old rows gone, is a hidden list shown.
  revealList();
  const drawn = Math.min(first, shown.length);
  const perRow = drawn === 0 ? 0 : ui.notes.getBoundingClientRect().height / drawn;
  const reserve = (from: number): void => {
    const left = shown.length - from;
    ui.notes.style.paddingBottom = left > 0 ? `${String(Math.round(left * perRow))}px` : "";
  };
  reserve(first);
  keepInView(anchor);
  const more = (from: number): void => {
    if (drawing !== listDrawing || from >= shown.length) return;
    ui.notes.append(...shown.slice(from, from + ROWS_PER_FRAME).map(row));
    reserve(from + ROWS_PER_FRAME);
    requestAnimationFrame(() => {
      more(from + ROWS_PER_FRAME);
    });
  };
  requestAnimationFrame(() => {
    more(first);
  });
  ui.empty.hidden = entries.length > 0;
  if (entries.length === 0) {
    ui.empty.textContent =
      filter.kind === "search"
        ? t.list.nothingFound
        : filter.kind === "all"
          ? t.list.noNotesYet
          : t.list.noNotesHere;
  }
  measured(kinds[filter.kind], since, { notes: entries.length });
  started();
  await checkStrays();
}

/** The line above the list saying what it shows, with "Show all". */
function showFilter(filter: ListFilter): void {
  // Search results come best first, a smart folder has its own order.
  ui.sort.hidden = filter.kind === "search" || filter.kind === "smart";
  ui.listFilter.hidden = filter.kind === "all";
  ui.listFilterText.textContent = listLabel(filter);
}

/** What the list shows, in words; empty for all notes. */
function listLabel(filter: ListFilter): string {
  switch (filter.kind) {
    case "tag":
      return filter.label;
    case "untagged":
      return t.list.untagged;
    case "search":
      return t.list.searchFor(filter.query);
    case "smart":
      return filter.folder.name;
    case "all":
      return "";
  }
}

/** The title of one document made of the list's notes (ADR-0021 rule 3). */
function listName(filter: ListFilter): string | null {
  switch (filter.kind) {
    case "search":
      return filter.query;
    case "all":
      return null;
    default:
      return listLabel(filter);
  }
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
  const all = [...strays.items, ...strays.notDownloaded.map(t.common.inICloudOnly)];
  straysShown = all.join("\n");
  if (all.length === 0 || readOnly !== null || straysShown === straysDismissed) {
    ui.strays.hidden = true;
    return;
  }
  const count = strays.notes + strays.notDownloaded.length;
  let text = t.strays.text(count);
  if (strays.notDownloaded.length > 0) text += t.strays.someInICloud;
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
  ui.notes.style.paddingBottom = "";
  ui.notes.replaceChildren(
    ...trashed.map((note) => {
      const item = document.createElement("li");
      const title = document.createElement("button");
      title.type = "button";
      title.textContent = note.title;
      title.title = t.trash.readIt;
      if (note.key === trashShown?.key) title.setAttribute("aria-current", "true");
      title.addEventListener("click", () => void showTrashed(note));
      const meta = document.createElement("small");
      const when = note.deleted === "" ? "" : new Date(note.deleted).toLocaleString();
      meta.textContent = t.trash.deleted(when, note.originalPath || "?");
      const restore = document.createElement("button");
      restore.type = "button";
      restore.className = "small restore";
      restore.textContent = t.common.restore;
      restore.disabled = readOnly !== null;
      restore.addEventListener("click", () => void restoreNote(note));
      item.append(title, meta, restore);
      return item;
    }),
  );
  ui.empty.hidden = trashed.length > 0;
  if (trashed.length === 0) ui.empty.textContent = t.trash.empty;
}

/** Shows a note from the trash read-only in the editor, so it can be read before restoring it. */
async function showTrashed(note: TrashedNote): Promise<void> {
  if (!(await leaveNote())) return;
  let text: string;
  try {
    text = await invoke<string>("read_trashed_note", { key: note.key });
  } catch (e) {
    setStatus(t.trash.cannotRead(note.title, asError(e).message));
    return;
  }
  closeEditor();
  trashShown = { key: note.key, path: note.originalPath.replace(/^notes\//, "") };
  editor.load(text);
  setStatus(t.trash.shown(note.title));
  showView("editor");
  void refreshList();
}

async function restoreNote(note: TrashedNote): Promise<void> {
  try {
    const path = await invoke<string>("restore_note", { key: note.key });
    setStatus(t.trash.restored(note.title, path));
    if (trashShown?.key === note.key) {
      // The note being read is back: open it for editing.
      await openNote({ path, title: note.title, modifiedMs: 0, available: true, pinned: false });
    }
    if (lastTrashed?.key === note.key) {
      lastTrashed = null;
      ui.undoDelete.hidden = true;
    }
    await refreshList();
  } catch (e) {
    setStatus(t.trash.notRestored(asError(e).message));
  }
}

/** Shows the notes or the trash in the list. */
function setListMode(mode: "notes" | "trash"): void {
  listMode = mode;
  hiddenPlace = null;
  if (mode === "notes" && trashShown !== null) closeEditor();
  ui.showTrash.textContent = mode === "trash" ? t.list.notes : t.list.trash;
  ui.showTrash.setAttribute("aria-pressed", String(mode === "trash"));
  const kind = sidebar.filter().kind;
  ui.sort.hidden = mode === "trash" || kind === "search" || kind === "smart";
  void refreshList();
}

/**
 * Saves, then renames the note's file if its title changed while it was open
 * (format.md §3). Runs whenever the user leaves a note, never while typing.
 * Resolves to `false`, and says why, if the note still has text that is not
 * saved (a failed save or a conflict): the caller stays on the note then, so
 * the text is not dropped (ADR-0042).
 */
function leaveNote(): Promise<boolean> {
  const left = saving.then(save).then(async () => {
    const note = current;
    if (!note || readOnly !== null) return true;
    if (conflictHash !== undefined || isDirty()) {
      setStatus(t.note.staysOpen(unsavedWhy()));
      return false;
    }
    try {
      const path = await invoke<string>("rename_after_edit", {
        path: note.path,
        titleAtOpen: note.titleAtOpen,
      });
      if (path !== note.path) {
        note.path = path;
        setStatus(t.note.renamed(path));
      }
    } catch (e) {
      setStatus(t.note.notRenamed(asError(e).message));
    }
    await snapshotIfChanged();
    return true;
  });
  saving = left.then(() => undefined);
  return left;
}

/** Why the open note has text that is not saved, for a message. */
function unsavedWhy(): string {
  if (conflictHash !== undefined) return t.conflict.unresolved;
  return saveProblem ?? t.note.notSavedYet;
}

/** Empties the editor: no note open. */
function closeEditor(): void {
  trashShown = null;
  noteHistory.close();
  ui.showHistory.disabled = true;
  ui.exportNote.disabled = true;
  window.clearTimeout(snapshotTimer);
  changedSinceSnapshot = false;
  current = null;
  saveProblem = null;
  images.forget();
  editor.load("");
  editor.setEnabled(false);
  ui.deleteNote.disabled = true;
  ui.pin.disabled = true;
  showPinned();
  ui.addImage.disabled = true;
  ui.addMusic.disabled = true;
  hideConflict();
  showSyncConflicts([]);
  ui.tags.textContent = "";
}

async function showTags(): Promise<void> {
  const tags = await invoke<Tag[]>("extract_tags", { markdown: editor.text() });
  ui.tags.textContent = tags.map((tag) => `#${tag.name}`).join(" ");
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
  if (!(await leaveNote())) return;
  if (!note.available) {
    try {
      await invoke("download_note", { path: note.path });
      setStatus(t.note.downloading(note.title));
    } catch (e) {
      setStatus(asError(e).message);
    }
    return;
  }
  await openPath(note.path);
}

/** Opens the note at `path`, relative to notes/. */
async function openPath(path: string): Promise<void> {
  try {
    const since = performance.now();
    const content = await invoke<NoteContent>("read_note", { path });
    load(content);
    showView("editor");
    measured("readNoteUi", since, { kilobytes: Math.ceil(content.text.length / 1024) });
  } catch (e) {
    const err = asError(e);
    setStatus(err.kind === "notDownloaded" ? t.common.notDownloaded : err.message);
  }
}

/**
 * Opens what a link in the open note points to: another note here, a web page
 * or a file in the vault with the system's default app.
 */
async function openLink(link: string): Promise<void> {
  const note = current;
  if (!note) return;
  const written = /^www\./i.test(link) ? `https://${link}` : link;
  try {
    const path = await invoke<string | null>("open_link", { notePath: note.path, link: written });
    if (path === null) return;
    if (!(await leaveNote())) return;
    await openPath(path);
  } catch (e) {
    const err = asError(e);
    setStatus(
      err.kind === "invalidPath"
        ? t.note.linkInvalid(link)
        : err.kind === "notDownloaded"
          ? t.common.notDownloaded
          : t.note.linkFailed(link, err.message),
    );
  }
}

function load(content: NoteContent): void {
  trashShown = null;
  noteHistory.close();
  ui.showHistory.disabled = false;
  ui.exportNote.disabled = false;
  window.clearTimeout(snapshotTimer);
  changedSinceSnapshot = false;
  images.forget();
  current = {
    path: content.path,
    hash: content.hash,
    savedText: content.text,
    titleAtOpen: content.title,
  };
  saveProblem = null;
  editor.load(content.text);
  editor.setEnabled(readOnly === null);
  ui.deleteNote.disabled = readOnly !== null;
  ui.pin.disabled = readOnly !== null;
  ui.addImage.disabled = readOnly !== null;
  ui.addMusic.disabled = readOnly !== null;
  showPinned();
  hideConflict();
  showSyncConflicts(content.conflicts);
  setStatus(
    content.conflictsError === null
      ? t.note.opened(content.path)
      : t.note.openedNoConflictCheck(content.path, content.conflictsError),
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
  ui.pin.textContent = pinned ? t.note.unpin : t.note.pin;
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
    setStatus(pinned ? t.note.pinnedFirst : t.note.unpinned);
    void refreshList();
  } catch (e) {
    const err = asError(e);
    if (err.kind === "conflict") {
      showConflict(err.currentHash);
    } else {
      setStatus((pinned ? t.note.notPinned : t.note.notUnpinned)(err.message));
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
    saveProblem = null;
    setStatus(t.note.saved(new Date().toLocaleTimeString()));
    refreshTagTreeIfNeeded();
  } catch (e) {
    const err = asError(e);
    if (err.kind === "conflict") {
      showConflict(err.currentHash);
    } else {
      saveProblem = err.message;
      setStatus(t.note.notSaved(err.message));
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
    setStatus(t.note.snapshotFailed(asError(e).message));
  }
}

/**
 * "Apply transposition" (format.md §5.7 rule 5): saves the note and stores
 * it in its history, then asks the core for the block with its chords moved.
 * Returns `null`, and says why, if the version before could not be kept.
 */
async function applyTransposition(
  source: string,
  semitones: number,
  notation: Notation,
): Promise<string | null> {
  const note = current;
  if (!note || readOnly !== null) return null;
  try {
    await flushSave();
    if (current !== note || isDirty() || conflictHash !== undefined) {
      setStatus(t.note.chordsUnsaved);
      return null;
    }
    await invoke("snapshot_note", { path: note.path, text: null });
    return await invoke<string>("apply_transposition", { source, semitones, notation });
  } catch (e) {
    setStatus(t.note.chordsFailed(asError(e).message));
    return null;
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
  setStatus(hashOnDisk === null ? t.conflict.deletedElsewhere : t.conflict.choose);
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
  const more = conflicts.length > 1 ? t.syncConflict.oneOf(conflicts.length) : "";
  ui.syncConflictText.textContent =
    syncConflict.copyPath === null
      ? t.syncConflict.savedOn(syncConflict.device ?? t.syncConflict.anotherDevice, when, more)
      : t.syncConflict.copy(syncConflict.copyPath, when, more);
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
      setStatus(t.syncConflict.resolved);
      await refreshList();
    } catch (e) {
      const err = asError(e);
      setStatus(t.syncConflict.notResolved(err.message));
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
      setStatus(t.syncConflict.keptBoth(copyPath));
      await refreshList();
    } catch (e) {
      setStatus(t.syncConflict.notChanged(asError(e).message));
    }
  });
});

// The conflict banner (ADR-0042): the version not chosen is stored first, in
// the note's history or the trash; if it cannot be, nothing changes.
ui.useTheirs.addEventListener("click", () => {
  saving = saving.then(async () => {
    const note = current;
    if (!note || conflictHash === undefined) return;
    try {
      const used = await invoke<OtherUsed>("use_other", { path: note.path, text: editor.text() });
      if (used.note) {
        load(used.note);
        setStatus(t.conflict.usedOther);
      } else {
        closeEditor();
        if (used.trashed) {
          lastTrashed = used.trashed;
          ui.undoDelete.hidden = false;
          setStatus(t.conflict.mineInTrash(used.trashed.title));
        }
        await showList();
      }
    } catch (e) {
      const err = asError(e);
      if (err.kind === "conflict") showConflict(err.currentHash);
      setStatus(t.conflict.notUsed(err.message));
    }
  });
});

ui.keepMine.addEventListener("click", () => {
  saving = saving.then(async () => {
    const note = current;
    if (!note || conflictHash === undefined) return;
    const text = editor.text();
    try {
      const saved = await invoke<SavedNote>("keep_mine", {
        path: note.path,
        text,
        otherHash: conflictHash,
      });
      applySaved(note, text, saved);
      hideConflict();
      saveProblem = null;
      setStatus(t.conflict.keptMine);
      void refreshList();
    } catch (e) {
      const err = asError(e);
      if (err.kind === "conflict") {
        showConflict(err.currentHash);
      } else {
        setStatus(t.conflict.notKept(err.message));
      }
    }
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
        setStatus(t.note.deletedElsewhere);
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
      setStatus(t.note.updated);
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
  void leaveNote().then(async (left) => {
    if (!left) return;
    await showList();
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
      setStatus(t.trash.notDeletedConflict);
      return;
    }
    try {
      const trashed = await invoke<TrashedNote>("trash_note", { path: note.path });
      closeEditor();
      lastTrashed = trashed;
      ui.undoDelete.hidden = false;
      setStatus(t.trash.moved(trashed.title));
      await showList();
    } catch (e) {
      setStatus(t.trash.notDeleted(asError(e).message));
    }
  });
});

musicMenu(ui.addMusic, {
  state: () => editor.state(),
  edit: ({ spec, focus }) => {
    editor.edit(spec, focus);
  },
  openLink: (url) => void openLink(url),
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

function pickVault(): void {
  firstStartAnswered = true;
  void leaveNote()
    .then(async (left) => {
      if (!left) return;
      const opened = await invoke<Opened | null>("pick_vault");
      if (opened) await folderOpened(opened);
    })
    .catch((e: unknown) => {
      setStatus(asError(e).message);
    });
}
ui.pick.addEventListener("click", pickVault);

// The first start (ADR-0048, ADR-0050): with no vault remembered, the app
// makes or opens the default vault. On the iPhone offline it asks instead and
// keeps waiting for iCloud.

/** The user chose a vault, so a late answer from iCloud opens nothing by itself. */
let firstStartAnswered = false;

async function firstStartDone(first: FirstStart): Promise<void> {
  switch (first.kind) {
    case "notFirst":
      ui.empty.textContent = t.list.noVault;
      return;
    case "offline":
      ui.empty.textContent = t.list.noVault;
      ui.firstStartText.textContent = t.firstStart.offline;
      ui.firstStartLocal.hidden = false;
      ui.firstStartPick.hidden = false;
      ui.firstStartICloud.hidden = true;
      ui.firstStart.hidden = false;
      void waitForICloud();
      return;
    case "opened": {
      await folderOpened({ kind: "vault", ...first.vault });
      const name = first.vault.name;
      const done = first.made ? t.firstStart.made(name) : t.firstStart.opened(name);
      setStatus(done + (first.local === null ? t.firstStart.inICloud : t.firstStart[first.local]));
      return;
    }
  }
}

function openDefault(local: boolean): void {
  firstStartAnswered = true;
  ui.firstStart.hidden = true;
  void leaveNote()
    .then(async (left) => {
      if (!left) return;
      if (!vaultIsOpen) ui.empty.textContent = t.firstStart.waiting;
      await firstStartDone(await invoke<FirstStart>("open_default_vault", { local }));
    })
    .catch((e: unknown) => {
      if (!vaultIsOpen) ui.empty.textContent = t.list.noVault;
      setStatus(t.firstStart.notMade(asError(e).message));
    });
}

/** Offline at the first start: once iCloud answers, opens the default vault,
 * or, if the user chose another meanwhile, offers the one iCloud has. */
async function waitForICloud(): Promise<void> {
  try {
    const vault = await invoke<string | null>("wait_for_icloud");
    if (!firstStartAnswered) {
      openDefault(false);
    } else if (vault !== null) {
      ui.firstStartText.textContent = t.firstStart.arrived(vault);
      ui.firstStartLocal.hidden = true;
      ui.firstStartPick.hidden = true;
      ui.firstStartICloud.hidden = false;
      ui.firstStart.hidden = false;
    }
  } catch (e: unknown) {
    setStatus(t.firstStart.notMade(asError(e).message));
  }
}

ui.firstStartLocal.addEventListener("click", () => {
  openDefault(true);
});
ui.firstStartPick.addEventListener("click", () => {
  ui.firstStart.hidden = true;
  pickVault();
});
ui.firstStartICloud.addEventListener("click", () => {
  openDefault(false);
});

ui.setupCreate.addEventListener("click", () => {
  ui.setupCreate.disabled = true;
  void leaveNote()
    .then(async (left) => {
      if (!left) return;
      const info = await invoke<VaultInfo>("create_vault");
      await folderOpened({ kind: "vault", ...info });
    })
    .catch((e: unknown) => {
      setStatus(t.setup.notOpened(asError(e).message));
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
      let message = t.strays.moved(n);
      if (result.relinked.length > 0) message += t.strays.relinked(result.relinked.length);
      if (result.skipped.length > 0) message += t.strays.skipped(result.skipped.join(", "));
      setStatus(message);
      await refreshList();
    })
    .catch((e: unknown) => {
      setStatus(t.strays.notMoved(asError(e).message));
      ui.straysMove.disabled = false;
    });
});

ui.straysLater.addEventListener("click", () => {
  straysDismissed = straysShown;
  ui.strays.hidden = true;
});

ui.setupCancel.addEventListener("click", () => {
  ui.setup.hidden = true;
  setStatus(t.setup.cancelled);
});

ui.newNote.addEventListener("click", () => {
  const text = `# ${t.editor.newNoteTitle}\n\n`;
  void leaveNote()
    .then(async (left) => {
      if (!left) return;
      const created = await invoke<NewNote>("new_note", { text });
      if (listMode === "trash") setListMode("notes");
      load({
        path: created.path,
        title: t.editor.newNoteTitle,
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
  ui.mode.textContent = mode === "live" ? t.note.source : t.note.preview;
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

const importer = createImport({
  vaultState: () => (vaultIsOpen ? { open: true, readOnly } : { open: false }),
  openNote: (path, title) => {
    showView("list");
    void openNote({ path, title, modifiedMs: 0, available: true, pinned: false });
  },
  imported: () => {
    void refreshList();
    void sidebar.refresh();
  },
});

const imports = createImports({
  vaultState: () => (vaultIsOpen ? { open: true, readOnly } : { open: false }),
  openNote: (path, title) => {
    showView("list");
    void openNote({ path, title, modifiedMs: 0, available: true, pinned: false });
  },
  changed: () => {
    void refreshList();
    void sidebar.refresh();
  },
});

const exporter = createExport({
  vaultOpen: () => vaultIsOpen,
  // Saved first, as the export reads the files; saving can also rename the note.
  selection: async () => {
    await leaveNote();
    await refreshList();
    const note = current;
    const filter = sidebar.filter();
    return {
      note: note && {
        path: note.path,
        title: entries.find((e) => e.path === note.path)?.title ?? note.titleAtOpen,
        unsaved: isDirty() || conflictHash !== undefined,
      },
      list:
        listMode === "trash"
          ? null
          : {
              notes: entries.filter((e) => !hitsInTrash.has(e.path)).map((e) => e.path),
              inTrash: hitsInTrash.size,
              name: listName(filter),
              label: listLabel(filter),
            },
    };
  },
  openNote: (path, title) => {
    showView("list");
    void openNote({ path, title, modifiedMs: 0, available: true, pinned: false });
  },
});

// On iOS, with no File menu, Export has buttons: in the note's bar for the note,
// above the list for the notes it shows.
void invoke<boolean>("exports_are_shared").then((shared) => {
  ui.exportNote.hidden = !shared;
  ui.exportList.hidden = !shared;
});
ui.exportNote.addEventListener("click", () => void exporter.open("note"));
ui.exportList.addEventListener("click", () => void exporter.open("list"));

const settingsView = createSettings({
  vaultState: () => (vaultIsOpen ? { open: true, readOnly } : { open: false }),
  chordNamesChanged: () => {
    // Read the open note again, so its song sheets show the new names.
    void leaveNote().then(async (left) => {
      const note = current;
      if (!left || !note) return;
      try {
        load(await invoke<NoteContent>("read_note", { path: note.path }));
      } catch (e) {
        setStatus(t.note.chordNamesLater(asError(e).message));
      }
    });
  },
});
// The iPhone has no menu: Settings sits in the bar there.
void invoke<boolean>("exports_are_shared").then((mobile) => {
  ui.showSettings.hidden = !mobile;
});
ui.showSettings.addEventListener("click", () => void settingsView.open());

// File menu (desktop): same actions as the buttons, plus Import…, Imports… and Export….
void listen<string>("menu", (event) => {
  if (event.payload === "open-vault") ui.pick.click();
  else if (event.payload === "new-note" && !ui.newNote.disabled) ui.newNote.click();
  else if (event.payload === "import") importer.open();
  else if (event.payload === "imports") imports.open();
  else if (event.payload === "export") void exporter.open();
  else if (event.payload === "settings") void settingsView.open();
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

// Quit and Close wait for the open note to be saved (ADR-0042, desktop only).
let quitting = false;
void listen("quit-requested", () => {
  void invoke("quit_heard");
  if (quitting) return;
  if (ui.quitDialog.open) {
    ui.quitCancel.focus();
    return;
  }
  quitting = true;
  window.clearTimeout(saveTimer);
  void leaveNote().then(async (left) => {
    quitting = false;
    if (left) {
      await invoke("quit_app");
      return;
    }
    const note = current;
    const title =
      (note && entries.find((e) => e.path === note.path)?.title) ?? note?.titleAtOpen ?? "";
    ui.quitText.textContent = t.quit.text(title, unsavedWhy());
    ui.quitDialog.showModal();
    ui.quitCancel.focus();
  });
});
let quitChosen = false;
ui.quitAnyway.addEventListener("click", () => {
  quitChosen = true;
  ui.quitDialog.close();
  void invoke("quit_app");
});
// Cancel, Return or Esc: a request to quit from the Dock or a logout waits
// for this answer (macOS).
ui.quitDialog.addEventListener("close", () => {
  if (quitChosen) return;
  void invoke("quit_cancelled");
});

async function startUp(): Promise<void> {
  const opened = await invoke<Opened | null>("restore_vault");
  if (opened?.kind !== "vault") noStart();
  if (opened) {
    await folderOpened(opened);
    return;
  }
  ui.empty.textContent = t.firstStart.waiting;
  try {
    await firstStartDone(await invoke<FirstStart>("first_start"));
  } catch (e: unknown) {
    ui.empty.textContent = t.list.noVault;
    setStatus(t.firstStart.notMade(asError(e).message));
  }
}
startUp().catch((e: unknown) => {
  noStart();
  setStatus(asError(e).message);
});
