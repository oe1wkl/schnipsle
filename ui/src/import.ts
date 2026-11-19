// SPDX-License-Identifier: Apache-2.0
// Import (ADR-0012, ADR-0019, format.md §12), desktop only: File › Import…
// opens a dialog that explains the export, lets the user pick it, and shows
// the dry run: what would be added, updated or skipped and why. Import then
// writes exactly that plan, with a progress bar, and the report becomes the
// result, each imported note openable from it.
//
// For Apple Notes the dates and folders come from apple-notes-metadata.json,
// which the app writes by running the metadata script on request. Simplenote
// is one zip file; notes from its trash go to the trash. HTML and Markdown
// are a folder of files or a single file.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { asError, el } from "./dom";
import { t } from "./strings";
import type {
  ImportNotice,
  ImportOutcome,
  ImportPlan,
  ImportPreview,
  ImportSource,
  PlanItem,
  PlannedAction,
  SkipReason,
} from "./types";

const ACTION_LABELS: Record<PlannedAction, string> = t.import.action;

/** What the source's export is, for the pick button. */
const PICK_LABELS: Record<ImportSource, string> = t.import.pick;

const REASON_LABELS: Record<SkipReason, string> = t.import.reason;

/** Adds first, then updates, then skipped notes; otherwise in export order. */
const ACTION_ORDER: Record<PlannedAction, number> = { add: 0, update: 1, skip: 2 };

const dialog = el("import-dialog") as HTMLDialogElement;
const sourceSelect = el("import-source") as HTMLSelectElement;
const choose = el("import-choose");
const pickButton = el("import-pick") as HTMLButtonElement;
const pickFileButton = el("import-pick-file") as HTMLButtonElement;
const pathText = el("import-path");
const metadataBox = el("import-metadata");
const metadataText = el("import-metadata-text");
const metadataButton = el("import-read-metadata") as HTMLButtonElement;
const report = el("import-report");
const summary = el("import-summary");
const notices = el("import-notices") as HTMLUListElement;
const items = el("import-items");
const progressBox = el("import-progress");
const progressBar = el("import-bar") as HTMLProgressElement;
const progressText = el("import-progress-text");
const errorText = el("import-error");
const cancelButton = el("import-cancel") as HTMLButtonElement;
const startButton = el("import-start") as HTMLButtonElement;

export interface ImportHooks {
  /** Whether a vault is open, and why it is read-only if it is. */
  vaultState(): { open: false } | { open: true; readOnly: string | null };
  /** Opens a note by its path below notes/. */
  openNote(path: string, title: string): void;
  /** Notes were written: refresh the lists. */
  imported(): void;
}

export function createImport(hooks: ImportHooks): { open(): void } {
  /** The folder of the last dry run. */
  let path: string | null = null;
  /** A dry run waits in the app for Import. */
  let planned = false;
  /** Notes are being written: the dialog cannot be closed. */
  let writing = false;
  /**
   * Counts closes and restarts. Reading an export can take a while and may be
   * cancelled; a result that arrives after that is dropped.
   */
  let run = 0;

  function source(): ImportSource {
    return sourceSelect.value as ImportSource;
  }

  function showError(message: string | null): void {
    errorText.textContent = message ?? "";
    errorText.hidden = message === null;
  }

  /** Disables the controls while something runs; Cancel only while writing. */
  function setBusy(value: boolean): void {
    pickButton.disabled = value;
    pickFileButton.disabled = value;
    sourceSelect.disabled = value;
    metadataButton.disabled = value;
    cancelButton.disabled = writing;
    if (value) startButton.disabled = true;
  }

  /** The steps for the chosen source; hidden once there is a report to read. */
  function showHelp(visible: boolean): void {
    for (const option of sourceSelect.options) {
      const help = document.getElementById(`import-help-${option.value}`);
      if (help) help.hidden = !visible || option.value !== source();
    }
  }

  function reset(): void {
    run += 1;
    path = null;
    planned = false;
    pathText.textContent = "";
    pickButton.textContent = PICK_LABELS[source()];
    pickFileButton.hidden = source() !== "html" && source() !== "markdown";
    showHelp(true);
    choose.hidden = false;
    metadataBox.hidden = true;
    report.hidden = true;
    progressBox.hidden = true;
    startButton.hidden = false;
    startButton.disabled = true;
    cancelButton.textContent = t.common.cancel;
    showError(null);
    setBusy(false);
  }

  /** Starts over, as far as the open vault allows an import. */
  function restart(): void {
    reset();
    const state = hooks.vaultState();
    if (!state.open) showError(t.import.noVault);
    else if (state.readOnly !== null) showError(state.readOnly);
    pickButton.disabled = !state.open || state.readOnly !== null;
    pickFileButton.disabled = pickButton.disabled;
  }

  function open(): void {
    if (dialog.open) return;
    restart();
    dialog.showModal();
  }

  async function close(): Promise<void> {
    if (writing) return;
    run += 1;
    if (planned) {
      planned = false;
      await invoke("cancel_import").catch(() => undefined);
    }
    dialog.close();
  }

  /** Lets the user pick the export; `file` picks a single file where a folder is the default. */
  async function pick(file = false): Promise<void> {
    showError(null);
    try {
      const picked = await invoke<string | null>("pick_import", { source: source(), file });
      if (picked !== null) await dryRun(picked);
    } catch (e) {
      showError(asError(e).message);
    }
  }

  /** Reads the export at `folder` (or file) and shows what an import would do. */
  async function dryRun(folder: string): Promise<void> {
    path = folder;
    pathText.textContent = folder;
    showHelp(false);
    setBusy(true);
    summary.textContent = t.import.reading;
    report.hidden = false;
    notices.replaceChildren();
    items.replaceChildren();
    const mine = run;
    try {
      const preview = await invoke<ImportPreview>("plan_import", {
        source: source(),
        path: folder,
      });
      if (mine !== run) {
        // Cancelled meanwhile: forget the plan this result left in the app.
        void invoke("cancel_import").catch(() => undefined);
        return;
      }
      planned = true;
      showMetadata(preview);
      showReport(preview.plan, false);
      setBusy(false);
      const writes = preview.plan.items.filter((i) => i.action !== "skip").length;
      startButton.disabled = writes === 0;
    } catch (e) {
      if (mine !== run) return;
      planned = false;
      report.hidden = true;
      setBusy(false);
      showError(asError(e).message);
    }
  }

  function showMetadata(preview: ImportPreview): void {
    metadataBox.hidden = preview.plan.source !== "apple-notes";
    if (preview.hasMetadata) {
      metadataText.textContent = t.import.metadataFound;
      metadataButton.textContent = t.import.readAgain;
    } else {
      metadataText.textContent = t.import.metadataMissing;
      metadataButton.textContent = t.import.readMetadata;
    }
    metadataBox.classList.toggle("warn", !preview.hasMetadata);
  }

  async function readMetadata(): Promise<void> {
    if (path === null) return;
    const folder = path;
    showError(null);
    setBusy(true);
    metadataText.textContent = t.import.readingNotes;
    const mine = run;
    try {
      await invoke<string>("read_apple_notes_metadata", { path: folder });
      if (mine !== run) return;
      await dryRun(folder);
    } catch (e) {
      if (mine !== run) return;
      setBusy(false);
      showError(asError(e).message);
      metadataText.textContent = t.import.readingFailed;
    }
  }

  async function start(): Promise<void> {
    showError(null);
    writing = true;
    setBusy(true);
    choose.hidden = true;
    metadataBox.hidden = true;
    progressBox.hidden = false;
    progressBar.value = 0;
    progressText.textContent = t.import.importing;
    summary.textContent = t.import.importingList;
    const unlisten = await listen<{ done: number; total: number }>("import-progress", (event) => {
      const { done, total } = event.payload;
      progressBar.max = total;
      progressBar.value = done;
      progressText.textContent = t.import.progress(done, total);
    });
    try {
      const outcome = await invoke<ImportOutcome>("apply_import");
      planned = false;
      showReport(outcome.plan, true);
      startButton.hidden = true;
      cancelButton.textContent = t.common.close;
      hooks.imported();
    } catch (e) {
      planned = false;
      showError(t.import.failed(asError(e).message));
      startButton.hidden = true;
      cancelButton.textContent = t.common.close;
      hooks.imported();
    } finally {
      unlisten();
      progressBox.hidden = true;
      writing = false;
      setBusy(false);
    }
  }

  function count(plan: ImportPlan, action: PlannedAction): number {
    return plan.items.filter((i) => i.action === action).length;
  }

  function showReport(plan: ImportPlan, done: boolean): void {
    const added = count(plan, "add");
    const updated = count(plan, "update");
    const skipped = count(plan, "skip");
    const trashed = plan.items.filter((i) => i.trashed === true).length;
    const parts = [
      ...(added > 0 || updated === 0 ? [t.import.added(added)] : []),
      ...(trashed > 0 ? [t.import.intoTrash(trashed)] : []),
      ...(updated > 0 ? [t.import.updated(updated)] : []),
      ...(skipped > 0 ? [t.import.skipped(skipped)] : []),
    ].join(", ");
    if (done) {
      summary.textContent = t.import.imported(parts);
    } else if (added + updated === 0) {
      summary.textContent = t.import.nothing(plan.items.length);
    } else {
      summary.textContent = t.import.plan(plan.items.length, plan.input, parts);
    }
    notices.replaceChildren(...plan.notices.map(noticeItem));
    const sorted = plan.items
      .map((item, index) => ({ item, index }))
      .sort(
        (a, b) => ACTION_ORDER[a.item.action] - ACTION_ORDER[b.item.action] || a.index - b.index,
      );
    items.replaceChildren(...sorted.map(({ item }) => row(item, done)));
  }

  function noticeItem(notice: ImportNotice): HTMLLIElement {
    const li = document.createElement("li");
    if (notice.title !== null) {
      const title = document.createElement("strong");
      title.textContent = `${notice.title}: `;
      li.append(title);
    }
    li.append(notice.message);
    return li;
  }

  function row(item: PlanItem, done: boolean): HTMLTableRowElement {
    const tr = document.createElement("tr");
    tr.className = item.action;
    const title = document.createElement("td");
    const name = item.title || t.imports.untitled;
    const notePath = item.path;
    const trashed = item.trashed === true;
    if (done && item.action !== "skip" && !trashed && notePath !== null) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "link";
      button.textContent = name;
      button.title = t.imports.openNote;
      button.addEventListener("click", () => {
        dialog.close();
        hooks.openNote(notePath, name);
      });
      title.append(button);
    } else {
      title.textContent = name;
    }
    const action = document.createElement("td");
    action.textContent = trashed ? t.import.newIntoTrash : ACTION_LABELS[item.action];
    const detail = document.createElement("td");
    if (item.action === "skip") {
      detail.textContent = item.reason === null ? "" : REASON_LABELS[item.reason];
    } else {
      detail.textContent = item.path ?? "";
      detail.className = "path";
    }
    if (item.problems.length > 0) {
      const list = document.createElement("ul");
      list.className = "problems";
      for (const problem of item.problems) {
        const li = document.createElement("li");
        li.textContent = problem;
        list.append(li);
      }
      detail.append(list);
    }
    tr.append(title, action, detail);
    return tr;
  }

  pickButton.addEventListener("click", () => void pick());
  pickFileButton.addEventListener("click", () => void pick(true));
  metadataButton.addEventListener("click", () => void readMetadata());
  startButton.addEventListener("click", () => void start());
  cancelButton.addEventListener("click", () => void close());
  sourceSelect.addEventListener("change", () => {
    if (planned) void invoke("cancel_import").catch(() => undefined);
    restart();
  });
  // Escape: like Cancel, but not while notes are written.
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
    void close();
  });

  return { open };
}
