// SPDX-License-Identifier: Apache-2.0
// Settings › Diagnostics (ADR-0045): how long the app's steps took since it
// started, and its memory, as the shell measured them. Copy gives them as text
// for docs/design/performance.md. The page also times what it shows itself:
// `measured` reports a list or a note from the call until it is drawn, and
// `started` the end of the start, when the first note list is drawn.
import { invoke } from "@tauri-apps/api/core";

import { asError, el } from "./dom";
import "./icloud-probe";
import { t } from "./strings";
import type { Diagnostics, MeasureDetail, MeasureSeries, Measurement } from "./types";

const appLine = el("diagnostics-app");
const memoryLine = el("diagnostics-memory");
const list = el("diagnostics-list");
const nothing = el("diagnostics-nothing");
const refreshButton = el("diagnostics-refresh") as HTMLButtonElement;
const copyButton = el("diagnostics-copy") as HTMLButtonElement;
const status = el("diagnostics-status");
const text = el("diagnostics-text") as HTMLTextAreaElement;

/** What the page times itself (src-tauri `measuring::UI_KINDS`). */
export type UiKind = "allNotesUi" | "notesInTagsUi" | "smartFolderUi" | "searchUi" | "readNoteUi";

/**
 * Reports a time the page measured, from `since` (`performance.now()`) until
 * the next frame, so laying out what was just put into the page counts too.
 */
export function measured(kind: UiKind, since: number, detail: MeasureDetail = {}): void {
  requestAnimationFrame(() => {
    const ms = performance.now() - since;
    void invoke("ui_measured", { kind, ms, detail }).catch(() => undefined);
  });
}

let startReported = false;
/** The app started without a vault to open: a start measured later would include the user's choice. */
export function noStart(): void {
  startReported = true;
}
/** The first note list is drawn: the start is over. */
export function started(): void {
  if (startReported) return;
  startReported = true;
  void invoke("ui_started").catch(() => undefined);
}

const d = t.diagnostics;

function ms(value: number): string {
  return d.ms(value < 10 ? value.toFixed(1) : Math.round(value).toString());
}

function megabytes(bytes: number): string {
  return d.megabytes((bytes / 1_000_000).toFixed(1));
}

function detailText(detail: MeasureDetail): string {
  const parts: string[] = [];
  if (detail.format !== undefined) parts.push(d.detail.format(detail.format));
  if (detail.notes !== undefined) parts.push(d.detail.notes(detail.notes));
  if (detail.folders !== undefined) parts.push(d.detail.folders(detail.folders));
  if (detail.query !== undefined) parts.push(d.detail.query(detail.query));
  if (detail.kilobytes !== undefined) parts.push(d.detail.kilobytes(detail.kilobytes));
  if (detail.read !== undefined) parts.push(d.detail.read(detail.read));
  if (detail.removed !== undefined) parts.push(d.detail.removed(detail.removed));
  return parts.join(", ");
}

function sampleText(sample: Measurement): string {
  const detail = detailText(sample.detail);
  return detail === "" ? ms(sample.ms) : `${ms(sample.ms)} (${detail})`;
}

function median(samples: Measurement[]): number {
  const sorted = samples.map((s) => s.ms).sort((a, b) => a - b);
  const middle = Math.floor(sorted.length / 2);
  if (sorted.length % 2 === 1) return sorted[middle] ?? 0;
  return ((sorted[middle - 1] ?? 0) + (sorted[middle] ?? 0)) / 2;
}

function kindName(kind: string): string {
  return d.kind[kind] ?? kind;
}

function summary(series: MeasureSeries): string {
  const slowest = series.slowest === null ? "" : sampleText(series.slowest);
  return d.summary(series.count, ms(median(series.recent)), slowest);
}

/** The order of the list: as a session goes, then the rest by name. */
const ORDER = [
  "start",
  "openVault",
  "indexSync",
  "allNotes",
  "allNotesUi",
  "tagFolders",
  "notesInTags",
  "notesInTagsUi",
  "smartFolder",
  "smartFolderUi",
  "search",
  "searchUi",
  "readNote",
  "readNoteUi",
  "export",
];

function sorted(series: MeasureSeries[]): MeasureSeries[] {
  const rank = (kind: string): number => {
    const at = ORDER.indexOf(kind);
    return at === -1 ? ORDER.length : at;
  };
  return [...series].sort((a, b) => rank(a.kind) - rank(b.kind) || a.kind.localeCompare(b.kind));
}

/** The measurements as text, for the report: a summary per kind and every time kept. */
function report(diag: Diagnostics, series: MeasureSeries[]): string {
  const lines = [
    d.app(diag.version, diag.device, diag.system),
    new Date().toISOString(),
    memoryText(diag),
  ];
  for (const s of series) {
    lines.push("", `${kindName(s.kind)}: ${summary(s)}`);
    for (const sample of s.recent) lines.push(`  ${sampleText(sample)}`);
  }
  return lines.join("\n");
}

function memoryText(diag: Diagnostics): string {
  if (diag.memory === null) return d.noMemory(diag.memoryProblem ?? "");
  const peak = diag.memory.peak === null ? null : megabytes(diag.memory.peak);
  return d.memory(megabytes(diag.memory.now), peak);
}

let shown = "";

/** Reads the measurements again and shows them. */
export async function refreshDiagnostics(): Promise<void> {
  status.textContent = "";
  let diag: Diagnostics;
  try {
    diag = await invoke<Diagnostics>("diagnostics");
  } catch (e) {
    appLine.textContent = d.unreadable(asError(e).message);
    memoryLine.textContent = "";
    list.replaceChildren();
    nothing.hidden = true;
    copyButton.disabled = true;
    return;
  }
  const series = sorted(diag.series);
  appLine.textContent = d.app(diag.version, diag.device, diag.system);
  memoryLine.textContent = memoryText(diag);
  list.replaceChildren(
    ...series.map((s) => {
      const item = document.createElement("li");
      const name = document.createElement("strong");
      name.textContent = kindName(s.kind);
      const line = document.createElement("small");
      line.textContent = summary(s);
      item.append(name, line);
      return item;
    }),
  );
  nothing.hidden = series.length > 0;
  shown = report(diag, series);
  copyButton.disabled = false;
  if (!text.hidden) text.value = shown;
}

refreshButton.addEventListener("click", () => void refreshDiagnostics());
copyButton.addEventListener("click", () => {
  void navigator.clipboard.writeText(shown).then(
    () => {
      status.textContent = d.copied;
    },
    () => {
      // Without access to the clipboard, the text is shown to copy by hand.
      status.textContent = d.notCopied;
      text.value = shown;
      text.hidden = false;
      text.focus();
      text.select();
    },
  );
});
