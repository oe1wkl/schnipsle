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
interface NoteContent {
  path: string;
  text: string;
  hash: string;
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
};

/** The note in the editor. `savedText` is what is on disk (as far as we know). */
let current: { path: string; hash: string; savedText: string } | null = null;
let conflictHash: string | null | undefined; // set while the conflict banner shows
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
  setStatus(`Opened ${content.path}`);
  void showTags();
  void refreshList();
}

async function save(): Promise<void> {
  if (!current || !isDirty() || conflictHash !== undefined) return;
  const note = current;
  const text = ui.text.value;
  try {
    note.hash = await invoke<string>("save_note", {
      path: note.path,
      text,
      expectedHash: note.hash,
    });
    note.savedText = text;
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
      ? invoke<string>("create_note", { path: note.path, text })
      : invoke<string>("save_note", { path: note.path, text, expectedHash: conflictHash });
  write
    .then((hash) => {
      note.hash = hash;
      note.savedText = text;
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
    .then(() => invoke<string>("create_note", { path, text }))
    .then((hash) => {
      load({ path, text, hash });
      showView("editor");
      ui.text.focus();
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
