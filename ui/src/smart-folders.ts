// SPDX-License-Identifier: Apache-2.0
// Smart folders (F-10, format.md §9, ADR-0018): saved searches in a tree above
// the tags.
//
// Every change is one command that the core applies to the file as it is now,
// so a change made on another device meanwhile is kept; afterwards the tree is
// read again. A form adds and edits folders (name, query, parent, order) and
// deletes them after a confirmation. Folders are reordered and nested by
// dragging their handle (pointer events, so it works with a mouse and by
// touch) or with the arrow keys on the handle.
import { invoke } from "@tauri-apps/api/core";

import { asError, el } from "./dom";

/** Order of a smart folder's notes (schnipsle-core `FolderSort`). */
export type FolderSort = "modified-desc" | "modified-asc" | "created-desc" | "title-asc";

/** A smart folder (schnipsle-core `SmartFolder`). */
export interface SmartFolder {
  id: string;
  name: string;
  query: string;
  /** The parent in effect; `null` at the top level. */
  parent: string | null;
  sort: FolderSort;
  depth: number;
}

/** What `list_smart_folders` returns. */
interface SmartFolderList {
  folders: SmartFolder[];
  /** Why the folders cannot be changed, if they cannot. */
  readOnly: string | null;
}

const SORT_LABELS: Record<FolderSort, string> = {
  "modified-desc": "Recently changed",
  "modified-asc": "Least recently changed",
  "created-desc": "Recently created",
  "title-asc": "Title",
};

/** Pointer movement before a press on the handle becomes a drag. */
const DRAG_THRESHOLD_PX = 4;
/** Distance from the tree's edge at which a drag scrolls it. */
const SCROLL_EDGE_PX = 28;

const tree = el("smart-tree") as HTMLUListElement;
const newFolder = el("new-smart-folder") as HTMLButtonElement;
const dialog = el("folder-dialog") as HTMLDialogElement;
const form = el("folder-form") as HTMLFormElement;
const dialogTitle = el("folder-dialog-title");
const nameInput = el("folder-name") as HTMLInputElement;
const queryInput = el("folder-query") as HTMLInputElement;
const parentSelect = el("folder-parent") as HTMLSelectElement;
const sortSelect = el("folder-sort") as HTMLSelectElement;
const formError = el("folder-error");
const deleteButton = el("folder-delete") as HTMLButtonElement;
const cancelButton = el("folder-cancel") as HTMLButtonElement;
const confirmBox = el("folder-confirm");
const confirmText = el("folder-confirm-text");
const confirmYes = el("folder-delete-yes") as HTMLButtonElement;
const confirmNo = el("folder-delete-no") as HTMLButtonElement;

/** Where a dragged folder would go. */
interface Drop {
  parent: string | null;
  index: number;
  /** The row that shows it, and how. */
  row: HTMLElement | null;
  zone: "before" | "after" | "into" | "end";
}

export function createSmartFolders(options: {
  /** The user picked a folder. */
  onSelect: (folder: SmartFolder) => void;
  /** The folder the note list shows, if any. */
  selectedId: () => string | null;
  /** Called after each refresh with the folders now in the file. */
  onRefresh: (folders: SmartFolder[]) => void;
  /** Reports a failed change; the tree is read again afterwards. */
  onError: (message: string) => void;
}): {
  /** Reads the folders again, keeping open ones open. */
  refresh: () => Promise<void>;
  /** Forgets everything, e.g. for another vault. */
  reset: () => void;
  /** Opens the form for a new folder with `query`. */
  saveSearch: (query: string) => void;
  /** `true` if folders can be added and changed. */
  editable: () => boolean;
  /** Marks the selected folder. */
  markSelected: () => void;
} {
  let folders: SmartFolder[] = [];
  let readOnly: string | null = "No vault is open";
  const open = new Set<string>();
  /** The folder being edited; `null` while adding one. */
  let editing: SmartFolder | null = null;

  const childrenOf = (parent: string | null): SmartFolder[] =>
    folders.filter((f) => f.parent === parent);

  /** `id` and every folder below it. */
  const subtree = (id: string): Set<string> => {
    const ids = new Set([id]);
    for (const f of folders) {
      // `folders` is depth first: a parent comes before its subfolders.
      if (f.parent !== null && ids.has(f.parent)) ids.add(f.id);
    }
    return ids;
  };

  const markSelected = (): void => {
    const selected = options.selectedId();
    for (const button of tree.querySelectorAll<HTMLButtonElement>("button.tag-name")) {
      if (button.dataset["id"] === selected) button.setAttribute("aria-current", "true");
      else button.removeAttribute("aria-current");
    }
  };

  /** Runs one change, then reads the tree again; reports a failure. */
  const change = async (command: string, args: Record<string, unknown>): Promise<boolean> => {
    try {
      await invoke(command, args);
      return true;
    } catch (e) {
      options.onError(asError(e).message);
      return false;
    } finally {
      await refresh();
    }
  };

  // --- Tree ---

  const node = (folder: SmartFolder): HTMLLIElement => {
    const item = document.createElement("li");
    item.setAttribute("role", "treeitem");
    const row = document.createElement("div");
    row.className = "tag-row smart-row";
    row.dataset["id"] = folder.id;
    row.style.paddingLeft = `${String(folder.depth * 14)}px`;

    const subfolders = childrenOf(folder.id);
    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "tag-toggle";
    const isOpen = open.has(folder.id);
    if (subfolders.length === 0) {
      toggle.style.visibility = "hidden";
      toggle.tabIndex = -1;
    } else {
      toggle.textContent = isOpen ? "▾" : "▸";
      toggle.setAttribute("aria-label", `${isOpen ? "Close" : "Open"} ${folder.name}`);
      item.setAttribute("aria-expanded", String(isOpen));
    }
    toggle.addEventListener("click", () => {
      if (open.has(folder.id)) open.delete(folder.id);
      else open.add(folder.id);
      render();
    });

    const name = document.createElement("button");
    name.type = "button";
    name.className = "tag-name";
    name.dataset["id"] = folder.id;
    name.textContent = folder.name;
    name.title = folder.query;
    name.addEventListener("click", () => {
      options.onSelect(folder);
    });
    row.append(toggle, name);

    if (readOnly === null) {
      const edit = document.createElement("button");
      edit.type = "button";
      edit.className = "row-action";
      edit.textContent = "✎";
      edit.setAttribute("aria-label", `Edit ${folder.name}`);
      edit.addEventListener("click", () => {
        showForm(folder);
      });
      const handle = document.createElement("button");
      handle.type = "button";
      handle.className = "row-action drag-handle";
      handle.textContent = "≡";
      handle.setAttribute(
        "aria-label",
        `Move ${folder.name}: drag, or use the arrow keys (left and right change the level)`,
      );
      handle.addEventListener("pointerdown", (event) => {
        startDrag(event, folder, row);
      });
      handle.addEventListener("keydown", (event) => {
        moveByKey(event, folder);
      });
      row.append(edit, handle);
    }

    item.append(row);
    if (subfolders.length > 0 && isOpen) {
      const group = document.createElement("ul");
      group.setAttribute("role", "group");
      group.append(...subfolders.map(node));
      item.append(group);
    }
    return item;
  };

  const render = (): void => {
    const items = childrenOf(null).map(node);
    if (items.length === 0) {
      const hint = document.createElement("li");
      hint.className = "muted tag-hint";
      hint.textContent =
        readOnly === null
          ? "No smart folders yet. Search, then Save, or tap New smart folder."
          : "No smart folders.";
      items.push(hint);
    }
    tree.replaceChildren(...items);
    newFolder.hidden = readOnly !== null;
    markSelected();
  };

  const refresh = async (): Promise<void> => {
    try {
      const list = await invoke<SmartFolderList>("list_smart_folders");
      folders = list.folders;
      readOnly = list.readOnly;
      render();
      options.onRefresh(folders);
    } catch (e) {
      folders = [];
      const hint = document.createElement("li");
      hint.className = "muted tag-hint";
      hint.textContent = asError(e).message;
      tree.replaceChildren(hint);
      newFolder.hidden = true;
    }
  };

  // --- Keyboard moves on the handle ---

  const moveByKey = (event: KeyboardEvent, folder: SmartFolder): void => {
    const siblings = childrenOf(folder.parent);
    const at = siblings.findIndex((f) => f.id === folder.id);
    let target: { parent: string | null; index: number } | null = null;
    switch (event.key) {
      case "ArrowUp":
        if (at > 0) target = { parent: folder.parent, index: at - 1 };
        break;
      case "ArrowDown":
        if (at < siblings.length - 1) target = { parent: folder.parent, index: at + 1 };
        break;
      case "ArrowRight": {
        // Into the folder above, as its last subfolder.
        const above = siblings[at - 1];
        if (above) {
          target = { parent: above.id, index: childrenOf(above.id).length };
          open.add(above.id);
        }
        break;
      }
      case "ArrowLeft": {
        // Out of its parent, right after it.
        const parent = folders.find((f) => f.id === folder.parent);
        if (parent) {
          const index = childrenOf(parent.parent).findIndex((f) => f.id === parent.id) + 1;
          target = { parent: parent.parent, index };
        }
        break;
      }
      default:
        return;
    }
    event.preventDefault();
    if (!target) return;
    void move(folder.id, target.parent, target.index).then(() => {
      tree.querySelector<HTMLElement>(`.smart-row[data-id="${folder.id}"] .drag-handle`)?.focus();
    });
  };

  const move = (id: string, parent: string | null, index: number): Promise<boolean> =>
    change("move_smart_folder", { id, parent, index });

  // --- Drag and drop ---

  const clearMarks = (): void => {
    for (const row of tree.querySelectorAll(".drop-before, .drop-after, .drop-into, .dragging")) {
      row.classList.remove("drop-before", "drop-after", "drop-into", "dragging");
    }
    tree.classList.remove("drop-end");
  };

  /** Where `dragged` would go with the pointer at `y` over `element`. */
  const dropAt = (dragged: SmartFolder, element: Element | null, y: number): Drop | null => {
    if (!element || !tree.contains(element)) return null;
    const row = element.closest<HTMLElement>(".smart-row");
    const excluded = subtree(dragged.id);
    if (!row) {
      // Below the last row: the end of the top level.
      const index = childrenOf(null).filter((f) => f.id !== dragged.id).length;
      return { parent: null, index, row: null, zone: "end" };
    }
    const target = folders.find((f) => f.id === row.dataset["id"]);
    if (!target || excluded.has(target.id)) return null;
    const rect = row.getBoundingClientRect();
    const zone =
      y < rect.top + rect.height / 4
        ? "before"
        : y > rect.bottom - rect.height / 4
          ? "after"
          : "into";
    if (zone === "into") {
      const index = childrenOf(target.id).filter((f) => f.id !== dragged.id).length;
      return { parent: target.id, index, row, zone };
    }
    const siblings = childrenOf(target.parent).filter((f) => f.id !== dragged.id);
    const at = siblings.findIndex((f) => f.id === target.id);
    return { parent: target.parent, index: zone === "before" ? at : at + 1, row, zone };
  };

  const startDrag = (start: PointerEvent, folder: SmartFolder, row: HTMLElement): void => {
    if (start.button !== 0) return;
    const handle = start.currentTarget as HTMLElement;
    handle.setPointerCapture(start.pointerId);
    start.preventDefault();
    let dragging = false;
    let drop: Drop | null = null;

    const onMove = (event: PointerEvent): void => {
      if (!dragging) {
        if (
          Math.abs(event.clientY - start.clientY) + Math.abs(event.clientX - start.clientX) <
          DRAG_THRESHOLD_PX
        )
          return;
        dragging = true;
        row.classList.add("dragging");
      }
      const box = tree.getBoundingClientRect();
      if (event.clientY < box.top + SCROLL_EDGE_PX) tree.scrollTop -= 8;
      else if (event.clientY > box.bottom - SCROLL_EDGE_PX) tree.scrollTop += 8;
      clearMarks();
      row.classList.add("dragging");
      drop = dropAt(folder, document.elementFromPoint(event.clientX, event.clientY), event.clientY);
      if (drop?.row) drop.row.classList.add(`drop-${drop.zone}`);
      else if (drop) tree.classList.add("drop-end");
    };
    const finish = (event: PointerEvent): void => {
      handle.removeEventListener("pointermove", onMove);
      handle.removeEventListener("pointerup", finish);
      handle.removeEventListener("pointercancel", finish);
      clearMarks();
      if (!dragging || event.type === "pointercancel" || !drop) return;
      if (drop.zone === "into" && drop.parent !== null) open.add(drop.parent);
      const unchanged =
        drop.parent === folder.parent &&
        drop.index === childrenOf(folder.parent).findIndex((f) => f.id === folder.id);
      if (!unchanged) void move(folder.id, drop.parent, drop.index);
    };
    handle.addEventListener("pointermove", onMove);
    handle.addEventListener("pointerup", finish);
    handle.addEventListener("pointercancel", finish);
  };

  // --- Form ---

  const showForm = (folder: SmartFolder | null, query = ""): void => {
    editing = folder;
    dialogTitle.textContent = folder ? "Edit smart folder" : "New smart folder";
    nameInput.value = folder?.name ?? "";
    queryInput.value = folder?.query ?? query;
    sortSelect.replaceChildren(
      ...Object.entries(SORT_LABELS).map(([value, label]) => new Option(label, value)),
    );
    sortSelect.value = folder?.sort ?? "modified-desc";
    // Not into itself or below it.
    const excluded = folder ? subtree(folder.id) : new Set<string>();
    parentSelect.replaceChildren(
      new Option("Top level", ""),
      ...folders
        .filter((f) => !excluded.has(f.id))
        .map((f) => new Option(`${"  ".repeat(f.depth + 1)}${f.name}`, f.id)),
    );
    parentSelect.value = folder?.parent ?? "";
    formError.hidden = true;
    confirmBox.hidden = true;
    deleteButton.hidden = folder === null;
    dialog.showModal();
    nameInput.focus();
  };

  const showFormError = (message: string): void => {
    formError.textContent = message;
    formError.hidden = false;
  };

  form.addEventListener("submit", (event) => {
    event.preventDefault();
    const name = nameInput.value.trim();
    const query = queryInput.value.trim();
    if (name === "" || query === "") {
      showFormError(
        name === "" ? "Give the folder a name." : "Enter what the folder searches for.",
      );
      return;
    }
    const args = {
      name,
      query,
      parent: parentSelect.value === "" ? null : parentSelect.value,
      sort: sortSelect.value as FolderSort,
    };
    const folder = editing;
    const saved = folder
      ? invoke("update_smart_folder", { id: folder.id, ...args })
      : invoke<string>("add_smart_folder", args);
    saved
      .then(async (result) => {
        dialog.close();
        if (args.parent !== null) open.add(args.parent);
        await refresh();
        // A new folder is shown right away; an edited one again if it is selected.
        const id = folder?.id ?? (result as string);
        const now = folders.find((f) => f.id === id);
        if (now && (!folder || options.selectedId() === id)) options.onSelect(now);
      })
      .catch(async (e: unknown) => {
        showFormError(asError(e).message);
        await refresh();
      });
  });

  cancelButton.addEventListener("click", () => {
    dialog.close();
  });
  deleteButton.addEventListener("click", () => {
    if (!editing) return;
    const below = subtree(editing.id).size - 1;
    confirmText.textContent =
      `Delete “${editing.name}”? No notes are deleted.` +
      (below > 0 ? ` Its ${below === 1 ? "subfolder moves" : "subfolders move"} up a level.` : "");
    confirmBox.hidden = false;
    confirmNo.focus();
  });
  confirmNo.addEventListener("click", () => {
    confirmBox.hidden = true;
    deleteButton.focus();
  });
  confirmYes.addEventListener("click", () => {
    if (!editing) return;
    const id = editing.id;
    dialog.close();
    void change("remove_smart_folder", { id });
  });
  newFolder.addEventListener("click", () => {
    showForm(null);
  });

  return {
    refresh,
    reset() {
      folders = [];
      readOnly = "No vault is open";
      open.clear();
      tree.replaceChildren();
      newFolder.hidden = true;
      if (dialog.open) dialog.close();
    },
    saveSearch(query) {
      showForm(null, query);
    },
    editable: () => readOnly === null,
    markSelected,
  };
}
