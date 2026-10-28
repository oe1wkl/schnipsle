// SPDX-License-Identifier: Apache-2.0
// The page's elements and small helpers shared by the UI modules.

import type { CommandError } from "./types";

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
  showHistory: el("show-history") as HTMLButtonElement,
  history: el("history"),
  historyHint: el("history-hint"),
  historyRestore: el("history-restore") as HTMLButtonElement,
  historyCopy: el("history-copy") as HTMLButtonElement,
  historyClose: el("history-close") as HTMLButtonElement,
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
};

/** A command's error, or any other failure, as a `CommandError`. */
export function asError(e: unknown): CommandError {
  if (typeof e === "object" && e !== null && "kind" in e) return e as CommandError;
  return { kind: "other", message: String(e), currentHash: null };
}

/** Shows a message in the status line under the editor. */
export function setStatus(message: string): void {
  ui.status.textContent = message;
}
