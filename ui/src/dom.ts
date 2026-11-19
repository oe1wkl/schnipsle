// SPDX-License-Identifier: Apache-2.0
// The page's elements and small helpers shared by the UI modules.

import { applyStrings, t } from "./strings";
import type { CommandError } from "./types";

// The page names its text by key (ADR-0041); fill it before anything reads it.
applyStrings();

// A dialog that closes gives focus back to where it was before it opened
// (ADR-0041 rule 7). `close` does not bubble, so it is caught on the way down.
let focusedOutside: HTMLElement | null = null;
document.addEventListener("focusin", (event) => {
  const target = event.target;
  if (target instanceof HTMLElement && target.closest("dialog") === null) {
    focusedOutside = target;
  }
});
document.addEventListener(
  "close",
  (event) => {
    if (!(event.target instanceof HTMLDialogElement)) return;
    const back = focusedOutside;
    // Not if the dialog's own action already moved focus, e.g. by opening a note.
    if (back?.isConnected && (document.activeElement ?? document.body) === document.body) {
      back.focus();
    }
  },
  true,
);

export function el(id: string): HTMLElement {
  const found = document.getElementById(id);
  if (!found) throw new Error(`missing #${id}`);
  return found;
}

export const ui = {
  layout: document.querySelector<HTMLElement>(".layout") ?? document.body,
  back: el("back") as HTMLButtonElement,
  vaultName: el("vault-name"),
  vaultHint: el("vault-hint"),
  pick: el("pick-vault") as HTMLButtonElement,
  /** Settings, on iOS, which has no app menu (ADR-0039). */
  showSettings: el("show-settings") as HTMLButtonElement,
  newNote: el("new-note") as HTMLButtonElement,
  empty: el("empty"),
  notes: el("notes") as HTMLUListElement,
  editorHost: el("editor-host"),
  mode: el("mode") as HTMLButtonElement,
  sort: el("sort") as HTMLSelectElement,
  showTags: el("show-tags") as HTMLButtonElement,
  listFilter: el("list-filter"),
  listFilterText: el("list-filter-text"),
  clearFilter: el("clear-filter") as HTMLButtonElement,
  showTrash: el("show-trash") as HTMLButtonElement,
  deleteNote: el("delete-note") as HTMLButtonElement,
  pin: el("pin") as HTMLButtonElement,
  addImage: el("add-image") as HTMLButtonElement,
  addMusic: el("add-music") as HTMLButtonElement,
  showHistory: el("show-history") as HTMLButtonElement,
  /** Export, on iOS, which has no File menu. */
  exportNote: el("export-note") as HTMLButtonElement,
  exportList: el("export-list") as HTMLButtonElement,
  history: el("history"),
  historyHint: el("history-hint"),
  historyRestore: el("history-restore") as HTMLButtonElement,
  historyCopy: el("history-copy") as HTMLButtonElement,
  historyClose: el("history-close") as HTMLButtonElement,
  historyCompare: el("history-compare") as HTMLButtonElement,
  historyWith: el("history-with") as HTMLSelectElement,
  historyDiff: el("history-diff"),
  historyList: el("history-list") as HTMLUListElement,
  historyPreview: el("history-preview"),
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
  quitDialog: el("quit-dialog") as HTMLDialogElement,
  quitText: el("quit-text"),
  quitAnyway: el("quit-anyway") as HTMLButtonElement,
  quitCancel: el("quit-cancel") as HTMLButtonElement,
};

/**
 * A command's error, or any other failure, as a `CommandError`. An error of a
 * kind the catalog knows gets its message from there (ADR-0041 rule 5); the
 * others keep the core's text, usually the system's own.
 */
export function asError(e: unknown): CommandError {
  if (typeof e !== "object" || e === null || !("kind" in e)) {
    return { kind: "other", message: String(e), currentHash: null };
  }
  const err = e as CommandError;
  const words: Partial<Record<string, (value: string) => string>> = t.errors;
  const word = words[err.kind];
  // A text about a path or name needs the value; without one, the core's text stays.
  if (!word || (word.length > 0 && (err.value ?? null) === null)) return err;
  return { ...err, message: word(err.value ?? "") };
}

/** Shows a message in the status line under the editor. */
export function setStatus(message: string): void {
  ui.status.textContent = message;
}
