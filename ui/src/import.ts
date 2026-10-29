// SPDX-License-Identifier: Apache-2.0
// Import (ADR-0012, ADR-0019, format.md §12), desktop only: File › Import…
// opens a dialog that explains the export, lets the user pick it, and shows
// the dry run: what would be added, updated or skipped and why. Import then
// writes exactly that plan, with a progress bar, and the report becomes the
// result, each imported note openable from it.
//
// For Apple Notes the dates and folders come from apple-notes-metadata.json,
// which the app writes by running the metadata script on request. Simplenote
// is one zip file; notes from its trash go to the trash.
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

import { asError, el } from "./dom";
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

const ACTION_LABELS: Record<PlannedAction, string> = {
  add: "New",
  update: "Update",
  skip: "Skipped",
};

/** What the source's export is, for the pick button. */
const PICK_LABELS: Record<ImportSource, string> = {
  "apple-notes": "Choose folder…",
  simplenote: "Choose zip file…",
  html: "Choose folder…",
  markdown: "Choose folder…",
};

const REASON_LABELS: Record<SkipReason, string> = {
  duplicate: "In the export twice; imported once",
  changedHere: "Changed here since the last import, so kept as it is",
  notNewer: "Not changed in the source since the last import",
  noModifiedTime: "Imported before; without a modification time it is not updated",
  missing: "Imported before and deleted here since",
  changedMeanwhile: "Changed here during the import, so kept as it is",
  failed: "Could not be written",
};

/** Adds first, then updates, then skipped notes; otherwise in export order. */
const ACTION_ORDER: Record<PlannedAction, number> = { add: 0, update: 1, skip: 2 };

const dialog = el("import-dialog") as HTMLDialogElement;
const sourceSelect = el("import-source") as HTMLSelectElement;
const choose = el("import-choose");
const pickButton = el("import-pick") as HTMLButtonElement;
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
  /** Something runs that must not be interrupted by closing. */
  let busy = false;

  function source(): ImportSource {
    return sourceSelect.value as ImportSource;
  }

  function showError(message: string | null): void {
    errorText.textContent = message ?? "";
    errorText.hidden = message === null;
  }

  function setBusy(value: boolean): void {
    busy = value;
    pickButton.disabled = value;
    sourceSelect.disabled = value;
    metadataButton.disabled = value;
    cancelButton.disabled = value;
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
    path = null;
    planned = false;
    pathText.textContent = "";
    pickButton.textContent = PICK_LABELS[source()];
    showHelp(true);
    choose.hidden = false;
    metadataBox.hidden = true;
    report.hidden = true;
    progressBox.hidden = true;
    startButton.hidden = false;
    startButton.disabled = true;
    cancelButton.textContent = "Cancel";
    showError(null);
    setBusy(false);
  }

  /** Starts over, as far as the open vault allows an import. */
  function restart(): void {
    reset();
    const state = hooks.vaultState();
    if (!state.open) showError("Open a vault first: the notes are imported into it.");
    else if (state.readOnly !== null) showError(state.readOnly);
    pickButton.disabled = !state.open || state.readOnly !== null;
  }

  function open(): void {
    if (dialog.open) return;
    restart();
    dialog.showModal();
  }

  async function close(): Promise<void> {
    if (busy) return;
    if (planned) {
      planned = false;
      await invoke("cancel_import").catch(() => undefined);
    }
    dialog.close();
  }

  async function pick(): Promise<void> {
    showError(null);
    try {
      const picked = await invoke<string | null>("pick_import", { source: source() });
      if (picked !== null) await dryRun(picked);
    } catch (e) {
      showError(asError(e).message);
    }
  }

  /** Reads the export at `folder` (a file for Simplenote) and shows what an import would do. */
  async function dryRun(folder: string): Promise<void> {
    path = folder;
    pathText.textContent = folder;
    showHelp(false);
    setBusy(true);
    summary.textContent = "Reading the export…";
    report.hidden = false;
    notices.replaceChildren();
    items.replaceChildren();
    try {
      const preview = await invoke<ImportPreview>("plan_import", {
        source: source(),
        path: folder,
      });
      planned = true;
      showMetadata(preview);
      showReport(preview.plan, false);
      setBusy(false);
      const writes = preview.plan.items.filter((i) => i.action !== "skip").length;
      startButton.disabled = writes === 0;
    } catch (e) {
      planned = false;
      report.hidden = true;
      setBusy(false);
      showError(asError(e).message);
    }
  }

  function showMetadata(preview: ImportPreview): void {
    metadataBox.hidden = preview.plan.source !== "apple-notes";
    if (preview.hasMetadata) {
      metadataText.textContent =
        "Dates and folders come from apple-notes-metadata.json in this folder. If you changed notes since, read them again.";
      metadataButton.textContent = "Read again from Notes";
    } else {
      metadataText.textContent =
        "Dates and folders are still missing: without them every note gets the time of the export, and importing again later with them would add the notes a second time.";
      metadataButton.textContent = "Read dates and folders from Notes";
    }
    metadataBox.classList.toggle("warn", !preview.hasMetadata);
  }

  async function readMetadata(): Promise<void> {
    if (path === null) return;
    const folder = path;
    showError(null);
    setBusy(true);
    metadataText.textContent =
      "Reading your notes from Notes… This takes about half a minute for a few hundred notes.";
    try {
      await invoke<string>("read_apple_notes_metadata", { path: folder });
      await dryRun(folder);
    } catch (e) {
      setBusy(false);
      showError(asError(e).message);
      metadataText.textContent = "Reading from Notes did not work.";
    }
  }

  async function start(): Promise<void> {
    showError(null);
    setBusy(true);
    choose.hidden = true;
    metadataBox.hidden = true;
    progressBox.hidden = false;
    progressBar.value = 0;
    progressText.textContent = "Importing…";
    summary.textContent = "Importing… The notes not yet written are the ones listed below.";
    const unlisten = await listen<{ done: number; total: number }>("import-progress", (event) => {
      const { done, total } = event.payload;
      progressBar.max = total;
      progressBar.value = done;
      progressText.textContent = `${String(done)} of ${String(total)} notes`;
    });
    try {
      const outcome = await invoke<ImportOutcome>("apply_import");
      planned = false;
      showReport(outcome.plan, true);
      startButton.hidden = true;
      cancelButton.textContent = "Close";
      hooks.imported();
    } catch (e) {
      planned = false;
      showError(
        `${asError(e).message}. Notes written up to then are in the vault and in its import log.`,
      );
      startButton.hidden = true;
      cancelButton.textContent = "Close";
      hooks.imported();
    } finally {
      unlisten();
      progressBox.hidden = true;
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
      ...(added > 0 || updated === 0 ? [`${String(added)} new`] : []),
      ...(trashed > 0 ? [`${String(trashed)} of them into the trash`] : []),
      ...(updated > 0 ? [`${String(updated)} updated`] : []),
      ...(skipped > 0 ? [`${String(skipped)} skipped`] : []),
    ].join(", ");
    if (done) {
      summary.textContent = `Imported: ${parts}.`;
    } else if (added + updated === 0) {
      summary.textContent = `Nothing to import: all ${String(plan.items.length)} notes are skipped.`;
    } else {
      summary.textContent = `${String(plan.items.length)} notes in ${plan.input}: ${parts}. Nothing is written until you choose Import.`;
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
    const name = item.title || "(untitled)";
    const notePath = item.path;
    const trashed = item.trashed === true;
    if (done && item.action !== "skip" && !trashed && notePath !== null) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = "link";
      button.textContent = name;
      button.title = "Open this note";
      button.addEventListener("click", () => {
        dialog.close();
        hooks.openNote(notePath, name);
      });
      title.append(button);
    } else {
      title.textContent = name;
    }
    const action = document.createElement("td");
    action.textContent = trashed ? "New, into the trash" : ACTION_LABELS[item.action];
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
  metadataButton.addEventListener("click", () => void readMetadata());
  startButton.addEventListener("click", () => void start());
  cancelButton.addEventListener("click", () => void close());
  sourceSelect.addEventListener("change", () => {
    if (planned) void invoke("cancel_import").catch(() => undefined);
    restart();
  });
  // Escape: like Cancel, but not while something runs.
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
    void close();
  });

  return { open };
}
