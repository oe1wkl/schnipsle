// SPDX-License-Identifier: Apache-2.0
// Search field and tag tree above the note list (ADR-0008, ADR-0009).
//
// Both choose what the note list shows: all notes, a tag folder, the untagged
// notes, or search results. The tag tree loads a folder's subfolders when it
// is opened: hierarchy levels (`#projekte` › `tuner`) and combinations with
// other tags (`∩ #cw`), each with its note count; it remembers which folders
// are open across refreshes.
import { invoke } from "@tauri-apps/api/core";

import { asError, el } from "./dom";

/** A tag folder (schnipsle-core `TagFolder`). */
interface TagFolder {
  keys: string[];
  name: string;
  kind: "tag" | "hierarchy" | "combination";
  count: number;
}

/** What the note list shows. */
export type ListFilter =
  | { kind: "all" }
  | { kind: "tag"; keys: string[]; label: string }
  | { kind: "untagged" }
  | { kind: "search"; query: string };

const SEARCH_DELAY_MS = 250;

const search = el("search") as HTMLInputElement;
const tree = el("tag-tree") as HTMLUListElement;
const tagsPanel = el("tags-panel") as HTMLDetailsElement;

const pathKey = (keys: string[]): string => keys.join("\u0000");

/** Creates the sidebar; `onFilter` is called whenever the user picks a filter. */
export function createSidebar(onFilter: (filter: ListFilter) => void): {
  /** Reloads the tag tree, keeping open folders open. */
  refresh: () => Promise<void>;
  /** Clears search and selection, e.g. for another vault. */
  reset: () => void;
  /** Enables or disables search and tags (no vault open). */
  setEnabled: (on: boolean) => void;
  /** The filter in effect. */
  filter: () => ListFilter;
  /** Back to all notes. */
  showAll: () => void;
  /** Opens the tag panel (phone: the Tags view). */
  openTags: () => void;
} {
  let filter: ListFilter = { kind: "all" };
  /** The filter to return to when the search field is cleared. */
  let beforeSearch: ListFilter = { kind: "all" };
  const open = new Set<string>();
  let searchTimer: number | undefined;

  const select = (next: ListFilter): void => {
    filter = next;
    markSelected();
    onFilter(next);
  };

  const markSelected = (): void => {
    const selected =
      filter.kind === "tag" ? pathKey(filter.keys) : filter.kind === "untagged" ? "untagged" : null;
    for (const button of tree.querySelectorAll<HTMLButtonElement>("button.tag-name")) {
      if (button.dataset["path"] === selected) button.setAttribute("aria-current", "true");
      else button.removeAttribute("aria-current");
    }
  };

  /** One folder: a toggle for its subfolders, its name and count. */
  const node = (folder: TagFolder, depth: number): HTMLLIElement => {
    const item = document.createElement("li");
    item.setAttribute("role", "treeitem");
    const row = document.createElement("div");
    row.className = "tag-row";
    row.style.paddingLeft = `${String(depth * 14)}px`;

    const toggle = document.createElement("button");
    toggle.type = "button";
    toggle.className = "tag-toggle";
    const name = document.createElement("button");
    name.type = "button";
    name.className = "tag-name";
    name.dataset["path"] = pathKey(folder.keys);
    const label = labelOf(folder);
    name.textContent = label;
    if (folder.kind === "combination") name.title = "Notes with all of these tags";
    const count = document.createElement("span");
    count.className = "tag-count";
    count.textContent = String(folder.count);
    row.append(toggle, name, count);

    const children = document.createElement("ul");
    children.setAttribute("role", "group");
    item.append(row, children);

    const key = pathKey(folder.keys);
    const setOpen = async (on: boolean): Promise<void> => {
      toggle.textContent = on ? "▾" : "▸";
      toggle.setAttribute("aria-label", on ? `Close ${label}` : `Open ${label}`);
      item.setAttribute("aria-expanded", String(on));
      if (on) open.add(key);
      else open.delete(key);
      if (!on) {
        children.replaceChildren();
        return;
      }
      const subfolders = await invoke<TagFolder[]>("tag_folders", { keys: folder.keys });
      children.replaceChildren(...subfolders.map((f) => node(f, depth + 1)));
      if (subfolders.length === 0) toggle.style.visibility = "hidden";
      markSelected();
    };
    toggle.addEventListener("click", () => void setOpen(!open.has(key)).catch(showError));
    name.addEventListener("click", () => {
      select({ kind: "tag", keys: folder.keys, label: pathLabel(folder) });
    });
    void setOpen(open.has(key)).catch(showError);
    return item;
  };

  const showError = (e: unknown): void => {
    const err = asError(e);
    const hint = document.createElement("li");
    hint.className = "muted tag-hint";
    hint.textContent =
      err.kind === "noIndex" ? "Tags appear once the index is built." : err.message;
    tree.replaceChildren(hint);
  };

  const refresh = async (): Promise<void> => {
    try {
      const [folders, untagged] = await Promise.all([
        invoke<TagFolder[]>("tag_folders", { keys: [] }),
        invoke<number>("untagged_count"),
      ]);
      const items = folders.map((f) => node(f, 0));
      if (untagged > 0) items.push(untaggedNode(untagged));
      if (items.length === 0) {
        const hint = document.createElement("li");
        hint.className = "muted tag-hint";
        hint.textContent = "No tags yet. Write #tag anywhere in a note.";
        items.push(hint);
      }
      tree.replaceChildren(...items);
      markSelected();
    } catch (e) {
      showError(e);
    }
  };

  const untaggedNode = (count: number): HTMLLIElement => {
    const item = document.createElement("li");
    const row = document.createElement("div");
    row.className = "tag-row";
    const spacer = document.createElement("span");
    spacer.className = "tag-toggle";
    const name = document.createElement("button");
    name.type = "button";
    name.className = "tag-name untagged";
    name.dataset["path"] = "untagged";
    name.textContent = "Untagged";
    name.addEventListener("click", () => {
      select({ kind: "untagged" });
    });
    const badge = document.createElement("span");
    badge.className = "tag-count";
    badge.textContent = String(count);
    row.append(spacer, name, badge);
    item.append(row);
    return item;
  };

  search.addEventListener("input", () => {
    window.clearTimeout(searchTimer);
    searchTimer = window.setTimeout(() => {
      const query = search.value.trim();
      if (query === "") {
        select(beforeSearch);
        return;
      }
      if (filter.kind !== "search") beforeSearch = filter;
      select({ kind: "search", query });
    }, SEARCH_DELAY_MS);
  });
  search.addEventListener("keydown", (event) => {
    if (event.key === "Escape" && search.value !== "") {
      search.value = "";
      window.clearTimeout(searchTimer);
      select(beforeSearch);
    }
  });

  return {
    refresh,
    reset() {
      search.value = "";
      open.clear();
      filter = { kind: "all" };
      beforeSearch = filter;
      tree.replaceChildren();
    },
    setEnabled(on) {
      search.disabled = !on;
    },
    filter: () => filter,
    showAll() {
      search.value = "";
      beforeSearch = { kind: "all" };
      select({ kind: "all" });
    },
    openTags() {
      tagsPanel.open = true;
    },
  };
}

/** A folder's name in the tree: the tag, its last level, or the added tag. */
function labelOf(folder: TagFolder): string {
  switch (folder.kind) {
    case "tag":
      return `#${folder.name}`;
    case "hierarchy":
      return folder.name.split("/").pop() ?? folder.name;
    case "combination":
      return `∩ #${folder.name}`;
  }
}

/** A folder's full name for the list header: `#radio ∩ #cw`. */
function pathLabel(folder: TagFolder): string {
  // Earlier keys are normalized; the folder's own name has its spelling.
  return [...folder.keys.slice(0, -1).map((k) => `#${k}`), `#${folder.name}`].join(" ∩ ");
}
