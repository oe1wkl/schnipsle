// SPDX-License-Identifier: Apache-2.0
// iOS spike UI: open a vault folder, list notes, edit with autosave, react to
// changes made elsewhere (`vault-changed` events, ADR-0015) and resolve conflicts.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

interface VaultInfo {
  name: string;
  path: string;
}
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
}
interface NoteContent {
  path: string;
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
  text: el("text") as HTMLTextAreaElement,
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
};

/** The note in the editor. `savedText` is what is on disk (as far as we know). */
let current: { path: string; hash: string; savedText: string } | null = null;
let conflictHash: string | null | undefined; // set while the conflict banner shows
let syncConflict: ConflictVersion | undefined; // shown in the sync conflict banner
let entries: NoteEntry[] = [];
let saveTimer: number | undefined;
let saving: Promise<void> = Promise.resolve();

const isDirty = (): boolean => current !== null && ui.text.value !== current.savedText;

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
  ui.vaultName.textContent = info.name;
  ui.vaultHint.textContent = info.path;
  ui.newNote.disabled = false;
  ui.empty.hidden = true;
}

async function refreshList(): Promise<void> {
  entries = await invoke<NoteEntry[]>("list_notes");
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
}

async function showTags(): Promise<void> {
  const tags = await invoke<Tag[]>("extract_tags", { markdown: ui.text.value });
  ui.tags.textContent = tags.map((t) => `#${t.name}`).join(" ");
}

async function openNote(note: NoteEntry): Promise<void> {
  await flushSave();
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
  current = { path: content.path, hash: content.hash, savedText: content.text };
  ui.text.value = content.text;
  ui.text.disabled = false;
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

/** Length of the front matter block at the start of `text`, 0 if there is none. */
function frontMatterLength(text: string): number {
  const match = /^\uFEFF?---\r?\n(?:.*\r?\n)*?---[ \t]*(?:\r?\n|$)/.exec(text);
  return match ? match[0].length : 0;
}

/**
 * Takes in what a save wrote. The core adds or updates front matter, so the
 * text on disk can differ from what was sent. The editor shows it only if
 * nothing was typed since the save started, so no keystroke is lost; the
 * cursor moves with the front matter. Otherwise the next save carries the new
 * typing and the core keeps the note's id and keys.
 */
function applySaved(
  note: { hash: string; savedText: string },
  sent: string,
  saved: SavedNote,
): void {
  note.hash = saved.hash;
  note.savedText = saved.text;
  if (note !== current || ui.text.value !== sent || saved.text === sent) return;
  const shift = frontMatterLength(saved.text) - frontMatterLength(sent);
  const moved = (at: number): number => (at >= frontMatterLength(sent) ? at + shift : at);
  const { selectionStart, selectionEnd } = ui.text;
  ui.text.value = saved.text;
  ui.text.setSelectionRange(moved(selectionStart), moved(selectionEnd));
}

async function save(): Promise<void> {
  if (!current || !isDirty() || conflictHash !== undefined) return;
  const note = current;
  const text = ui.text.value;
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
  const who = syncConflict.device ?? "Another device";
  const when = new Date(syncConflict.modifiedMs).toLocaleString();
  const more = conflicts.length > 1 ? ` (1 of ${String(conflicts.length)})` : "";
  ui.syncConflictText.textContent = `${who} saved a different version at ${when}${more}.`;
  ui.syncOther.value = syncConflict.text;
}

/** Keeps `text` as the note; every other version goes into the note's history. */
function resolveSyncConflict(pick: "mine" | "other"): void {
  saving = saving.then(save).then(async () => {
    if (!current || !syncConflict) return;
    const text = pick === "mine" ? ui.text.value : syncConflict.text;
    try {
      const content = await invoke<NoteContent>("resolve_conflict", {
        path: current.path,
        text,
        expectedHash: current.hash,
      });
      load(content);
      setStatus("Conflict resolved – the other version is kept in the note's history.");
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
  const text = ui.text.value;
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
    if (content.text === ui.text.value) {
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

ui.text.addEventListener("input", () => {
  setStatus("Editing…");
  scheduleSave();
  void showTags();
});
ui.text.addEventListener("blur", () => void flushSave());

ui.back.addEventListener("click", () => {
  void flushSave().then(() => {
    showView("list");
  });
});

ui.pick.addEventListener("click", () => {
  void flushSave()
    .then(() => invoke<VaultInfo | null>("pick_vault"))
    .then(async (info) => {
      if (!info) return;
      current = null;
      ui.text.value = "";
      ui.text.disabled = true;
      vaultOpened(info);
      showView("list");
      await refreshList();
    })
    .catch((e: unknown) => {
      setStatus(asError(e).message);
    });
});

ui.newNote.addEventListener("click", () => {
  const stamp = new Date().toISOString().slice(0, 19).replace("T", " ").replaceAll(":", "");
  const path = `Note ${stamp}.md`;
  const text = "# New note\n\n";
  void flushSave()
    .then(() => invoke<SavedNote>("create_note", { path, text }))
    .then((saved) => {
      load({ path, ...saved, conflicts: [], conflictsError: null });
      showView("editor");
      ui.text.focus();
      ui.text.setSelectionRange(saved.text.length, saved.text.length);
    })
    .catch((e: unknown) => {
      setStatus(asError(e).message);
    });
});

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
  else void flushSave();
});

invoke<VaultInfo | null>("restore_vault")
  .then(async (info) => {
    if (!info) return;
    vaultOpened(info);
    await refreshList();
  })
  .catch((e: unknown) => {
    setStatus(asError(e).message);
  });
