// SPDX-License-Identifier: Apache-2.0
// Imports (ADR-0019, format.md §12.4), desktop only: File › Imports… lists
// the import runs of the open vault, newest first. Undo… shows the dry run of
// undoing one: which notes go to the trash, which get their previous version
// back, and which stay because they were changed here since. Undo import then
// does exactly that, and the list shows the run as undone.
import { invoke } from "@tauri-apps/api/core";

import { asError, el } from "./dom";
import { t } from "./strings";
import type { ImportRun, ImportSource, ImportUndo, UndoAction, UndoItem } from "./types";

const SOURCE_LABELS: Record<ImportSource, string> = t.imports.source;

const ACTION_LABELS: Record<UndoAction, string> = t.imports.action;

/** Notes that change first, then those that stay; otherwise in the order handled. */
const ACTION_ORDER: Record<UndoAction, number> = {
  trash: 0,
  restore: 1,
  keep: 2,
  inTrash: 3,
  missing: 4,
};

const dialog = el("imports-dialog") as HTMLDialogElement;
const list = el("imports-list");
const summary = el("imports-summary");
const runsBody = el("imports-runs");
const undoBox = el("imports-undo");
const undoSummary = el("imports-undo-summary");
const items = el("imports-items");
const errorText = el("imports-error");
const backButton = el("imports-back") as HTMLButtonElement;
const closeButton = el("imports-close") as HTMLButtonElement;
const startButton = el("imports-start") as HTMLButtonElement;

export interface ImportsHooks {
  /** Whether a vault is open, and why it is read-only if it is. */
  vaultState(): { open: false } | { open: true; readOnly: string | null };
  /** Opens a note by its path below notes/. */
  openNote(path: string, title: string): void;
  /** Notes were moved to the trash or restored: refresh the lists. */
  changed(): void;
}

function when(time: string): string {
  const date = new Date(time);
  return Number.isNaN(date.getTime()) ? time : date.toLocaleString();
}

function from(run: ImportRun): string {
  const source = run.source === "" ? "" : SOURCE_LABELS[run.source];
  return run.input === null ? source : `${source}: ${run.input}`;
}

export function createImports(hooks: ImportsHooks): { open(): void } {
  /** The run whose undo is shown, while it waits for Undo import. */
  let pending: ImportRun | null = null;
  /** Notes are being moved: the dialog cannot be closed. */
  let undoing = false;
  /** Counts views; a result that arrives after the user moved on is dropped. */
  let view = 0;

  function showError(message: string | null): void {
    errorText.textContent = message ?? "";
    errorText.hidden = message === null;
  }

  function readOnly(): string | null {
    const state = hooks.vaultState();
    return state.open ? state.readOnly : t.imports.openFirst;
  }

  async function showRuns(): Promise<void> {
    view += 1;
    const mine = view;
    pending = null;
    list.hidden = false;
    undoBox.hidden = true;
    backButton.hidden = true;
    startButton.hidden = true;
    closeButton.textContent = t.common.close;
    showError(null);
    runsBody.replaceChildren();
    if (!hooks.vaultState().open) {
      summary.textContent = t.imports.noVault;
      return;
    }
    summary.textContent = t.imports.reading;
    try {
      const runs = await invoke<ImportRun[]>("list_imports");
      if (mine !== view) return;
      summary.textContent = runs.length === 0 ? t.imports.none : t.imports.explain;
      runsBody.replaceChildren(...runs.map(runRow));
    } catch (e) {
      if (mine !== view) return;
      summary.textContent = "";
      showError(asError(e).message);
    }
  }

  function runRow(run: ImportRun): HTMLTableRowElement {
    const tr = document.createElement("tr");
    const date = document.createElement("td");
    date.textContent = run.started === "" ? "" : when(run.started);
    const source = document.createElement("td");
    source.textContent = from(run);
    const notes = document.createElement("td");
    notes.textContent =
      run.problem === null
        ? [
            ...(run.added > 0 || run.updated === 0 ? [t.imports.added(run.added)] : []),
            ...(run.updated > 0 ? [t.imports.updated(run.updated)] : []),
          ].join(", ")
        : "";
    const status = document.createElement("td");
    if (run.problem !== null) {
      tr.className = "skip";
      status.textContent = run.problem;
    } else if (run.undone !== null) {
      tr.className = "skip";
      status.textContent = t.imports.undoneAt(when(run.undone));
    } else {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "small";
      button.textContent = t.imports.undoButton;
      button.addEventListener("click", () => void planUndo(run));
      status.append(button);
    }
    tr.append(date, source, notes, status);
    return tr;
  }

  async function planUndo(run: ImportRun): Promise<void> {
    view += 1;
    const mine = view;
    showError(null);
    list.hidden = true;
    undoBox.hidden = false;
    backButton.hidden = false;
    startButton.hidden = false;
    startButton.disabled = true;
    undoSummary.textContent = t.imports.looking;
    items.replaceChildren();
    try {
      const planned = await invoke<ImportUndo>("plan_undo_import", { id: run.id });
      if (mine !== view) return;
      pending = planned.run;
      showUndo(planned, false);
      const changes = planned.items.filter(
        (i) => i.action === "trash" || i.action === "restore",
      ).length;
      const why = readOnly();
      if (why !== null) showError(why);
      startButton.disabled = changes === 0 || why !== null;
    } catch (e) {
      if (mine !== view) return;
      undoSummary.textContent = "";
      showError(asError(e).message);
    }
  }

  function showUndo(undo: ImportUndo, done: boolean): void {
    const count = (action: UndoAction): number =>
      undo.items.filter((i) => i.action === action).length;
    const trash = count("trash");
    const restore = count("restore");
    const keep = count("keep");
    const missing = count("missing");
    const inTrash = count("inTrash");
    const parts = [
      ...(trash > 0 ? [t.imports.intoTrash(trash)] : []),
      ...(restore > 0 ? [t.imports.restored(restore)] : []),
      ...(keep > 0 ? [t.imports.stay(keep)] : []),
      ...(inTrash > 0 ? [t.imports.alreadyInTrash(inTrash)] : []),
      ...(missing > 0 ? [t.imports.gone(missing)] : []),
    ].join(", ");
    const { run } = undo;
    const source = run.source === "" ? "" : SOURCE_LABELS[run.source];
    const what = t.imports.what(run.input ?? t.imports.theSource, source, when(run.started));
    if (done) {
      undoSummary.textContent = t.imports.undone(what, parts);
    } else if (trash + restore === 0) {
      undoSummary.textContent = t.imports.nothing(what, parts);
    } else {
      undoSummary.textContent = t.imports.plan(what, parts);
    }
    const sorted = undo.items
      .map((item, index) => ({ item, index }))
      .sort(
        (a, b) => ACTION_ORDER[a.item.action] - ACTION_ORDER[b.item.action] || a.index - b.index,
      );
    items.replaceChildren(...sorted.map(({ item }) => itemRow(item, done)));
  }

  function itemRow(item: UndoItem, done: boolean): HTMLTableRowElement {
    const tr = document.createElement("tr");
    if (item.action === "keep" || item.action === "missing" || item.action === "inTrash") {
      tr.className = "skip";
    }
    const title = document.createElement("td");
    const name = item.title || t.imports.untitled;
    const path = item.path;
    // A note that is still among the notes can be opened from the report.
    const openable =
      path !== null && (item.action === "keep" || (done && item.action === "restore"));
    if (openable) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "link";
      button.textContent = name;
      button.title = t.imports.openNote;
      button.addEventListener("click", () => {
        dialog.close();
        hooks.openNote(path, name);
      });
      title.append(button);
    } else {
      title.textContent = name;
    }
    const action = document.createElement("td");
    action.textContent = ACTION_LABELS[item.action];
    const detail = document.createElement("td");
    detail.textContent = path ?? "";
    detail.className = "path";
    if (item.problem !== null) {
      const problems = document.createElement("ul");
      problems.className = "problems";
      const li = document.createElement("li");
      li.textContent = item.problem;
      problems.append(li);
      detail.append(problems);
    }
    tr.append(title, action, detail);
    return tr;
  }

  async function start(): Promise<void> {
    const run = pending;
    if (run === null) return;
    showError(null);
    undoing = true;
    startButton.disabled = true;
    backButton.disabled = true;
    closeButton.disabled = true;
    undoSummary.textContent = t.imports.undoing;
    try {
      const undone = await invoke<ImportUndo>("undo_import", { id: run.id });
      pending = null;
      showUndo(undone, true);
      startButton.hidden = true;
    } catch (e) {
      showError(asError(e).message);
    } finally {
      undoing = false;
      backButton.disabled = false;
      closeButton.disabled = false;
      hooks.changed();
    }
  }

  function open(): void {
    if (dialog.open) return;
    void showRuns();
    dialog.showModal();
  }

  function close(): void {
    if (undoing) return;
    view += 1;
    dialog.close();
  }

  backButton.addEventListener("click", () => void showRuns());
  startButton.addEventListener("click", () => void start());
  closeButton.addEventListener("click", close);
  // Escape: like Close, but not while notes are moved.
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
    close();
  });

  return { open };
}
