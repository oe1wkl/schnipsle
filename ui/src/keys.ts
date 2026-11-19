// SPDX-License-Identifier: Apache-2.0
// Keyboard access for lists and trees (ADR-0041 rule 7). A list or tree is one
// Tab stop: the item last focused, else the current one, else the first. Up
// and Down move between items, Home and End go to the first and last; Return
// and Space press the item's button as always. In a tree (the WAI-ARIA tree
// pattern), Right opens a folder or goes to its first subfolder, Left closes
// it or goes to its parent. The other controls of a row (✎, ≡, Restore) can
// be reached with Tab from its item; those of other rows wait their turn.

export interface RovingOptions {
  /** The items, relative to the container: buttons, one per row. */
  items: string;
  /** The row around an item and its other controls. */
  row: string;
  /** Tree keys: the rows are `role="treeitem"` with `aria-expanded`. */
  tree?: boolean;
  /** Controls left out of the Tab order: opened and closed by the arrow keys. */
  skip?: string;
}

/** Makes `container` one Tab stop with arrow keys inside; follows its changes. */
export function roving(container: HTMLElement, options: RovingOptions): void {
  /** The item that has the Tab stop, by its element and by its key (rows are re-rendered). */
  let active: HTMLElement | null = null;
  let activeKey: string | null = null;

  const items = (): HTMLElement[] => [...container.querySelectorAll<HTMLElement>(options.items)];
  const keyOf = (item: HTMLElement): string | null =>
    item.dataset["path"] ?? item.dataset["id"] ?? item.dataset["name"] ?? item.textContent;

  const setActive = (item: HTMLElement | null): void => {
    active = item;
    activeKey = item === null ? null : keyOf(item);
  };

  /** Gives the Tab stop to the active row; keeps focus on a row drawn anew. */
  const apply = (): void => {
    const all = items();
    let refocus = false;
    if (active === null || !all.includes(active)) {
      const again = activeKey === null ? undefined : all.find((i) => keyOf(i) === activeKey);
      refocus = again !== undefined && active !== null && !active.isConnected && lostFocus();
      setActive(
        again ?? all.find((i) => i.getAttribute("aria-current") === "true") ?? all[0] ?? null,
      );
    }
    const activeRow = active?.closest(options.row) ?? null;
    for (const control of container.querySelectorAll<HTMLElement>("button, input, select, a")) {
      const skipped = options.skip !== undefined && control.matches(options.skip);
      control.tabIndex = !skipped && control.closest(options.row) === activeRow ? 0 : -1;
    }
    if (refocus) active?.focus();
  };

  const lostFocus = (): boolean =>
    document.activeElement === null || document.activeElement === document.body;

  const go = (item: HTMLElement | undefined): void => {
    if (!item) return;
    setActive(item);
    apply();
    item.focus();
    item.scrollIntoView({ block: "nearest" });
  };

  container.addEventListener("focusin", (event) => {
    const item = (event.target as HTMLElement).closest(options.row)?.querySelector(options.items);
    if (item instanceof HTMLElement && item !== active) {
      setActive(item);
      apply();
    }
  });

  container.addEventListener("keydown", (event) => {
    const target = event.target as HTMLElement;
    if (!target.matches(options.items) || event.altKey || event.metaKey || event.ctrlKey) return;
    const all = items();
    const at = all.indexOf(target);
    let next: HTMLElement | undefined;
    switch (event.key) {
      case "ArrowDown":
        next = all[at + 1];
        break;
      case "ArrowUp":
        next = all[at - 1];
        break;
      case "Home":
        next = all[0];
        break;
      case "End":
        next = all[all.length - 1];
        break;
      case "ArrowRight":
      case "ArrowLeft":
        if (!options.tree) return;
        next = treeStep(target, event.key === "ArrowRight");
        break;
      default:
        return;
    }
    event.preventDefault();
    go(next);
  });

  /** Right or Left in a tree: opens or closes the folder, or moves in or out. */
  const treeStep = (item: HTMLElement, right: boolean): HTMLElement | undefined => {
    const node = item.closest<HTMLElement>('[role="treeitem"]');
    if (!node) return undefined;
    const expanded = node.getAttribute("aria-expanded");
    const toggle =
      options.skip === undefined
        ? null
        : item.closest(options.row)?.querySelector<HTMLElement>(options.skip);
    if (right) {
      if (expanded === "false") {
        toggle?.click();
        return undefined;
      }
      if (expanded === "true") {
        return node.querySelector<HTMLElement>(`[role="group"] ${options.items}`) ?? undefined;
      }
      return undefined;
    }
    if (expanded === "true") {
      toggle?.click();
      return undefined;
    }
    const parent = node.parentElement?.closest<HTMLElement>('[role="treeitem"]');
    return parent?.querySelector<HTMLElement>(options.items) ?? undefined;
  };

  new MutationObserver(apply).observe(container, { childList: true, subtree: true });
  apply();
}
