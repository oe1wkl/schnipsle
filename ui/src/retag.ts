// SPDX-License-Identifier: Apache-2.0
// Renaming and merging tags (F-11, ADR-0040). The dialog asks for the new
// name and shows, while it is typed, what would change: notes, smart folders,
// and whether the tag is merged into one that exists. Nothing is written until
// Rename is pressed; the result can be undone until the app quits, from this
// dialog when it is opened again.
import { invoke } from "@tauri-apps/api/core";

import { asError, el } from "./dom";
import { t } from "./strings";
import type { RetagOutcome, RetagPlan } from "./types";

const PLAN_DELAY_MS = 300;
/** Notes listed by title in the report; the rest are counted. */
const LISTED = 50;

const dialog = el("retag-dialog") as HTMLDialogElement;
const form = el("retag-form") as HTMLFormElement;
const title = el("retag-title");
const nameInput = el("retag-name") as HTMLInputElement;
const help = el("retag-help");
const report = el("retag-report");
const summary = el("retag-summary");
const list = el("retag-notes") as HTMLUListElement;
const errorText = el("retag-error");
const undoButton = el("retag-undo") as HTMLButtonElement;
const cancelButton = el("retag-cancel") as HTMLButtonElement;
const startButton = el("retag-start") as HTMLButtonElement;

export function createRetag(options: {
  /** Saves the open note, as the rename reads the files. */
  beforeChange: () => Promise<void>;
  /** Notes and smart folders changed: `from` was renamed to `to`. */
  changed: (from: string, to: string) => void;
  status: (message: string) => void;
}): {
  /** Opens the dialog for the tag `name` (without `#`). */
  open: (name: string) => void;
  /** Forgets the last rename, e.g. for another vault. */
  reset: () => void;
} {
  let old = "";
  let plan: RetagPlan | null = null;
  let planTimer: number | undefined;
  /** Counts plan requests, so an older answer never replaces a newer one. */
  let asked = 0;
  let busy = false;
  /** The rename Undo puts back, while the app runs. */
  let last: { old: string; new: string } | null = null;

  const showError = (message: string | null): void => {
    errorText.hidden = message === null;
    errorText.textContent = message ?? "";
  };

  const item = (text: string, muted = false): HTMLLIElement => {
    const li = document.createElement("li");
    li.textContent = text;
    if (muted) li.className = "muted";
    return li;
  };

  const showPlan = (p: RetagPlan): void => {
    plan = p;
    const tags = p.notes.reduce((sum, n) => sum + n.occurrences, 0);
    const parts: string[] = [];
    if (p.notes.length > 0) parts.push(t.retag.summary(p.notes.length, tags));
    if (p.folders.length > 0) parts.push(t.retag.folders(p.folders.length));
    if (p.merge && parts.length > 0) parts.push(t.retag.merge(p.old, p.new));
    if (parts.length === 0) parts.push(t.retag.nothing);
    summary.textContent = parts.join(" ");
    const items = p.notes.slice(0, LISTED).map((n) => item(n.title));
    if (p.notes.length > LISTED) {
      items.push(item(t.retag.more(p.notes.length - LISTED), true));
    }
    for (const f of p.folders) {
      items.push(item(t.retag.folder(f.name, f.query, f.renamed)));
    }
    for (const s of p.skipped) items.push(item(t.retag.notRead(s.title, s.reason), true));
    list.replaceChildren(...items);
    report.hidden = false;
    startButton.disabled = p.notes.length === 0 && p.folders.length === 0;
  };

  const showOutcome = (done: RetagOutcome, undo: boolean): void => {
    const parts = [
      done.changed.length > 0
        ? (undo ? t.retag.undoneIn : t.retag.renamedIn)(done.changed.length)
        : undo
          ? t.retag.noneUndone
          : t.retag.noneRenamed,
    ];
    if (done.folders > 0) parts.push(t.retag.foldersChanged(done.folders));
    if (done.skipped.length > 0) parts.push(t.retag.leftAsIs(done.skipped.length));
    summary.textContent = parts.join(" ");
    const items = done.skipped.map((s) => item(t.retag.skipped(s.title, s.reason)));
    if (done.foldersError !== null) {
      items.push(item(t.retag.foldersError(done.foldersError)));
    }
    list.replaceChildren(...items);
    report.hidden = false;
  };

  const ask = async (): Promise<void> => {
    const asking = ++asked;
    const name = nameInput.value.trim().replace(/^#+/, "");
    if (name === "") {
      plan = null;
      report.hidden = true;
      showError(null);
      startButton.disabled = true;
      return;
    }
    try {
      const p = await invoke<RetagPlan>("plan_tag_rename", { old, new: name });
      if (asking !== asked) return;
      showError(null);
      showPlan(p);
    } catch (e) {
      if (asking !== asked) return;
      plan = null;
      report.hidden = true;
      startButton.disabled = true;
      showError(asError(e).message);
    }
  };

  /** The form before a rename: name field, Rename and Cancel. */
  const showForm = (): void => {
    nameInput.disabled = false;
    help.hidden = false;
    startButton.hidden = false;
    startButton.disabled = true;
    cancelButton.textContent = t.common.cancel;
    undoButton.hidden = last === null;
    undoButton.textContent = last === null ? t.common.undo : t.retag.undoLast(last.old, last.new);
    report.hidden = true;
    showError(null);
  };

  /** After a rename or undo: the result, Done, and Undo if it can be undone. */
  const showDone = (): void => {
    nameInput.disabled = true;
    help.hidden = true;
    startButton.hidden = true;
    cancelButton.textContent = t.common.done;
    undoButton.hidden = last === null;
    undoButton.textContent = t.common.undo;
  };

  nameInput.addEventListener("input", () => {
    window.clearTimeout(planTimer);
    startButton.disabled = true;
    planTimer = window.setTimeout(() => void ask(), PLAN_DELAY_MS);
  });

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    void (async () => {
      if (busy) return;
      window.clearTimeout(planTimer);
      if (plan === null || plan.new !== nameInput.value.trim().replace(/^#+/, "")) {
        await ask();
      }
      const p = plan;
      if (p === null || startButton.disabled) return;
      busy = true;
      startButton.disabled = true;
      try {
        await options.beforeChange();
        const done = await invoke<RetagOutcome>("rename_tag", { plan: p });
        last = done.changed.length > 0 || done.folders > 0 ? { old: p.old, new: p.new } : null;
        showOutcome(done, false);
        showDone();
        options.changed(p.old, p.new);
        options.status(t.retag.renamed(p.old, p.new));
      } catch (e) {
        showError(asError(e).message);
        startButton.disabled = false;
      } finally {
        busy = false;
      }
    })();
  });

  undoButton.addEventListener("click", () => {
    void (async () => {
      const undoing = last;
      if (busy || undoing === null) return;
      busy = true;
      undoButton.disabled = true;
      try {
        await options.beforeChange();
        const done = await invoke<RetagOutcome>("undo_tag_rename");
        last = null;
        showOutcome(done, true);
        showDone();
        options.changed(undoing.new, undoing.old);
        options.status(t.retag.undone(undoing.old));
      } catch (e) {
        showError(asError(e).message);
      } finally {
        busy = false;
        undoButton.disabled = false;
      }
    })();
  });

  cancelButton.addEventListener("click", () => {
    dialog.close();
  });
  dialog.addEventListener("close", () => {
    window.clearTimeout(planTimer);
    asked++;
    plan = null;
  });

  return {
    open(name) {
      old = name;
      plan = null;
      title.textContent = t.retag.title(name);
      nameInput.value = name;
      showForm();
      dialog.showModal();
      nameInput.select();
    },
    reset() {
      last = null;
      if (dialog.open) dialog.close();
    },
  };
}
