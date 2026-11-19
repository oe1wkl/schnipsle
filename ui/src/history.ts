// SPDX-License-Identifier: Apache-2.0
// History of the open note (ADR-0011): its versions, newest first, in place of
// the editor; the one selected shown read-only, or compared with the note as
// it is now or with another version (ADR-0038). The core computes the
// comparison; this module only draws it. Restoring and copying stay with the
// caller, which owns the editor and the save queue.
import { invoke } from "@tauri-apps/api/core";

import { asError, ui } from "./dom";
import { roving } from "./keys";
import { t } from "./strings";
import type { Comparison, DiffPart, DiffRow, KeyChange, Version } from "./types";

export interface HistoryHooks {
  /** The open note's path, or `null`; answers for another note are dropped. */
  note: () => string | null;
  /** Saves pending edits, so the history and "the note now" are up to date. */
  flush: () => Promise<void>;
  /** `true` in a read-only vault: no restoring or copying. */
  readOnly: () => boolean;
  /** Shows a version's text, read-only (an editor in the app). */
  preview: { load(text: string): void };
  /** Shows a message in the status line. */
  status: (message: string) => void;
  restore: (version: Version) => void;
  copy: (version: Version) => void;
}

/** "The note now" in the comparison's choice. */
const NOW = "";

export function versionLabel(v: Version): string {
  return v.takenMs === null ? v.name : new Date(v.takenMs).toLocaleString();
}

/** An RFC 3339 time in the front matter, shown in local time. */
const STAMP = /^\d{4}-\d\d-\d\dT\d\d:\d\d(:\d\d(\.\d+)?)?(Z|[+-]\d\d:\d\d)$/;

function frontMatterValue(value: string): string {
  const unquoted = value.replace(/^(["'])(.*)\1$/, "$2");
  return STAMP.test(unquoted) ? new Date(unquoted).toLocaleString() : value;
}

function node<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  ...children: (Node | string)[]
): HTMLElementTagNameMap[K] {
  const element = document.createElement(tag);
  if (className) element.className = className;
  element.append(...children);
  return element;
}

/** A line's pieces, the changed words in `<mark>`. */
function pieces(parts: DiffPart[]): (Node | string)[] {
  return parts.map((p) => (p.changed ? node("mark", "", p.text) : p.text));
}

/** A line of the comparison: its number, a sign that does not rely on colour, the text. */
function line(kind: "same" | "removed" | "added", number: number, content: (Node | string)[]) {
  const sign = { same: " ", removed: "−", added: "+" }[kind];
  const text =
    kind === "removed"
      ? node("del", "diff-text", ...content)
      : kind === "added"
        ? node("ins", "diff-text", ...content)
        : node("span", "diff-text", ...content);
  const row = node(
    "div",
    `diff-row diff-${kind}`,
    node("span", "diff-number", String(number)),
    node("span", "diff-sign", sign),
    text,
  );
  const said = text.textContent || t.history.emptyLine;
  row.setAttribute(
    "aria-label",
    kind === "removed"
      ? t.history.removedLine(said)
      : kind === "added"
        ? t.history.addedLine(said)
        : said,
  );
  return row;
}

function sameRows(lines: string[], before: number, after: number): HTMLElement[] {
  return lines.map((text, k) => {
    const row = line("same", after + k, [text]);
    row.dataset.before = String(before + k);
    return row;
  });
}

function rowElement(row: DiffRow): HTMLElement {
  switch (row.kind) {
    case "same":
      return line("same", row.after, [row.text]);
    case "removed":
      return line("removed", row.before, pieces(row.parts));
    case "added":
      return line("added", row.after, pieces(row.parts));
    case "folded": {
      const count = row.lines.length;
      const fold = node("button", "diff-fold", t.history.unchanged(count));
      fold.type = "button";
      fold.title = t.history.showThem;
      fold.addEventListener("click", () => {
        fold.replaceWith(...sameRows(row.lines, row.before, row.after));
      });
      return fold;
    }
  }
}

function keyChange(change: KeyChange): HTMLElement {
  // Values over several lines (lists, maps) go below the key, one under the other.
  const block = [change.before, change.after].some((v) => v?.includes("\n"));
  const value = (v: string, tag: "del" | "ins") => {
    const shown = node(tag, block ? "diff-block" : "", frontMatterValue(v));
    shown.title = v;
    return shown;
  };
  const parts: (Node | string)[] = [node("span", "diff-key", `${change.key}:`), " "];
  if (change.before !== null && change.after !== null) {
    parts.push(value(change.before, "del"), block ? "" : " → ", value(change.after, "ins"));
  } else if (change.after !== null) {
    parts.push(node("span", "muted", t.history.added), value(change.after, "ins"));
  } else if (change.before !== null) {
    parts.push(node("span", "muted", t.history.removed), value(change.before, "del"));
  }
  return node("li", "", ...parts);
}

/** What the comparison says above the lines. */
function summary(c: Comparison): string {
  const onlyModified = c.frontMatter.length === 1 && c.frontMatter[0]?.key === "modified";
  if (c.added === 0 && c.removed === 0) {
    if (c.frontMatter.length === 0) return t.history.same;
    if (onlyModified) return t.history.onlyModified;
    return t.history.onlyFrontMatter;
  }
  const counts = [];
  if (c.added > 0) counts.push(t.history.linesAdded(c.added));
  if (c.removed > 0) counts.push(t.history.linesRemoved(c.removed));
  return counts.join(", ");
}

/** The comparison as elements, for `ui.historyDiff`. */
export function renderComparison(c: Comparison, heading: string): HTMLElement[] {
  const out: HTMLElement[] = [
    node("p", "diff-heading", heading),
    node("p", "diff-summary", summary(c)),
  ];
  if (c.frontMatter.length > 0) {
    const list = node("ul", "diff-front-matter", ...c.frontMatter.map(keyChange));
    list.setAttribute("aria-label", t.history.frontMatter);
    out.push(list);
  }
  if (c.rows.length > 0) {
    const body = node("div", "diff-body", ...c.rows.map(rowElement));
    body.setAttribute("aria-label", t.history.text);
    out.push(body);
  }
  return out;
}

export function createHistory(hooks: HistoryHooks): {
  open(): Promise<void>;
  close(): void;
} {
  roving(ui.historyList, { items: "li > button", row: "li" });
  let versions: Version[] = [];
  let selected: Version | null = null;
  let comparing = false;
  /** The version compared with the selected one; `NOW` for the note itself. */
  let other = NOW;
  /** Bumped by every request, so a late answer does not overwrite a newer one. */
  let generation = 0;

  const setActions = (enabled: boolean): void => {
    ui.historyRestore.disabled = !enabled || hooks.readOnly();
    ui.historyCopy.disabled = !enabled || hooks.readOnly();
  };

  function close(): void {
    generation += 1;
    selected = null;
    versions = [];
    ui.history.hidden = true;
    ui.editorHost.hidden = false;
    ui.showHistory.setAttribute("aria-pressed", "false");
  }

  async function open(): Promise<void> {
    await hooks.flush();
    const path = hooks.note();
    if (path === null) return;
    const asked = ++generation;
    let listed: Version[];
    try {
      listed = await invoke<Version[]>("list_versions", { path });
    } catch (e) {
      hooks.status(t.history.notAvailable(asError(e).message));
      return;
    }
    if (asked !== generation || hooks.note() !== path) return;
    versions = listed;
    selected = null;
    other = NOW;
    ui.editorHost.hidden = true;
    ui.history.hidden = false;
    ui.showHistory.setAttribute("aria-pressed", "true");
    ui.historyHint.textContent =
      versions.length === 0 ? t.history.none : t.history.earlier(versions.length);
    ui.historyCompare.disabled = versions.length === 0;
    showMode();
    hooks.preview.load("");
    ui.historyDiff.replaceChildren();
    setActions(false);
    ui.historyList.replaceChildren(
      ...versions.map((version) => {
        const button = node("button", "", versionLabel(version));
        button.type = "button";
        button.dataset.name = version.name;
        button.addEventListener("click", () => void select(version));
        return node("li", "", button);
      }),
    );
    const [newest] = versions;
    if (newest) await select(newest);
  }

  /** Shows the preview or the comparison, as `comparing` says. */
  function showMode(): void {
    ui.historyCompare.setAttribute("aria-pressed", String(comparing));
    ui.historyWith.hidden = !comparing;
    ui.historyPreview.hidden = comparing;
    ui.historyDiff.hidden = !comparing;
  }

  /** The choice of what to compare with: the note now, or any other version. */
  function fillWith(): void {
    const options = [new Option(t.history.withNow, NOW)];
    for (const version of versions) {
      if (version !== selected) {
        options.push(new Option(t.history.withVersion(versionLabel(version)), version.name));
      }
    }
    if (other === selected?.name) other = NOW;
    ui.historyWith.replaceChildren(...options);
    ui.historyWith.value = other;
  }

  async function select(version: Version): Promise<void> {
    selected = version;
    for (const button of ui.historyList.querySelectorAll<HTMLButtonElement>("button")) {
      if (button.dataset.name === version.name) button.setAttribute("aria-current", "true");
      else button.removeAttribute("aria-current");
    }
    fillWith();
    await show();
  }

  /** Reads the selected version, or compares it, and shows the result. */
  async function show(): Promise<void> {
    const path = hooks.note();
    const version = selected;
    if (path === null || version === null) return;
    const asked = ++generation;
    const current = (): boolean => asked === generation && hooks.note() === path;
    setActions(false);
    if (!comparing) {
      try {
        const text = await invoke<string>("read_version", { path, name: version.name });
        if (!current()) return;
        hooks.preview.load(text);
        setActions(true);
      } catch (e) {
        if (current()) hooks.preview.load(t.history.cannotRead(asError(e).message));
      }
      return;
    }
    // The older version is always the "before" side; the list is newest first.
    const otherVersion = versions.find((v) => v.name === other);
    const selectedIsOlder =
      otherVersion === undefined || versions.indexOf(version) > versions.indexOf(otherVersion);
    const [before, after] = selectedIsOlder
      ? [version.name, otherVersion?.name ?? null]
      : [other, version.name];
    const heading =
      otherVersion === undefined
        ? t.history.changesToNow(versionLabel(version))
        : selectedIsOlder
          ? t.history.changes(versionLabel(version), versionLabel(otherVersion))
          : t.history.changes(versionLabel(otherVersion), versionLabel(version));
    try {
      if (otherVersion === undefined) await hooks.flush();
      const comparison = await invoke<Comparison>("compare_versions", { path, before, after });
      if (!current()) return;
      ui.historyDiff.replaceChildren(...renderComparison(comparison, heading));
      ui.historyDiff.scrollTop = 0;
      setActions(true);
    } catch (e) {
      if (current()) {
        ui.historyDiff.replaceChildren(
          node("p", "diff-summary", t.history.notCompared(asError(e).message)),
        );
      }
    }
  }

  ui.showHistory.addEventListener("click", () => {
    if (ui.history.hidden) void open();
    else close();
  });
  ui.historyClose.addEventListener("click", close);
  ui.historyCompare.addEventListener("click", () => {
    comparing = !comparing;
    showMode();
    void show();
  });
  ui.historyWith.addEventListener("change", () => {
    other = ui.historyWith.value;
    void show();
  });
  ui.historyRestore.addEventListener("click", () => {
    if (selected) hooks.restore(selected);
  });
  ui.historyCopy.addEventListener("click", () => {
    if (selected) hooks.copy(selected);
  });

  return { open, close };
}
