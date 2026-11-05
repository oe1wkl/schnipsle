// SPDX-License-Identifier: Apache-2.0
// Export (ADR-0010, ADR-0021), desktop only: File › Export… asks for the open
// note or the notes in the list, the format (Word and EPUB if Pandoc is there),
// one document or one file per note, the page size and whether to keep the line
// of tags; then a save dialog or folder picker. The report says what is different in the export, each note
// openable from it, and the result can be shown in Finder.
import { invoke } from "@tauri-apps/api/core";

import { asError, el } from "./dom";
import type {
  ExportChoices,
  ExportFormat,
  ExportLayout,
  ExportPaper,
  ExportProblem,
  ExportProblemKind,
  ExportReport,
} from "./types";

const FORMAT_LABELS: Record<ExportFormat, string> = {
  html: "HTML",
  pdf: "PDF",
  odt: "OpenDocument text (ODT)",
  docx: "Word (DOCX)",
  epub: "EPUB",
};

/** Only a folder export to HTML copies attachments (ADR-0021 rule 6). */
const COPIES_ATTACHMENTS: ExportFormat = "html";

/** Formats whose documents have pages of a chosen size. */
const WITH_PAGES: readonly ExportFormat[] = ["pdf", "odt", "docx"];

const PROBLEM_LABELS: Record<ExportProblemKind, (target: string) => string> = {
  noteNotRead: () => "Could not be read, perhaps not downloaded yet, so it is not in the export",
  pictureMissing: (t) => `Picture not found, its description is shown instead: ${t}`,
  webPictureAsLink: (t) => `Picture on the web, shown as a link: ${t}`,
  pictureNotConverted: (t) => `Picture left as it is, which many programs cannot show: ${t}`,
  attachmentNotIncluded: (t) => `Attachment not included, only the text of its link: ${t}`,
  noteNotExported: (t) => `Link to a note not in this export; only its text stays: ${t}`,
  noteInOtherFile: (t) => `Link to a note in another file; only its text stays: ${t}`,
};

const SETTINGS_KEY = "schnipsle.export";

/** How many numbered file names the result spells out. */
const NUMBERED_SHOWN = 5;

const dialog = el("export-dialog") as HTMLDialogElement;
const choose = el("export-choose");
const whatNote = el("export-what-note") as HTMLInputElement;
const whatList = el("export-what-list") as HTMLInputElement;
const noteLabel = el("export-note-label");
const listLabel = el("export-list-label");
const formatSelect = el("export-format") as HTMLSelectElement;
const layoutOne = el("export-layout-one") as HTMLInputElement;
const layoutMany = el("export-layout-many") as HTMLInputElement;
const oneLabel = el("export-one-label");
const manyLabel = el("export-many-label");
const filesBox = el("export-files");
const noPandoc = el("export-no-pandoc");
const paperRow = el("export-paper-row");
const paperSelect = el("export-paper") as HTMLSelectElement;
const keepTags = el("export-keep-tags") as HTMLInputElement;
const unsavedText = el("export-unsaved");
const progressBox = el("export-progress");
const progressText = el("export-progress-text");
const report = el("export-report");
const summary = el("export-summary");
const numberedText = el("export-numbered");
const warningsList = el("export-warnings");
const problemsTable = el("export-problems-table");
const problems = el("export-problems");
const errorText = el("export-error");
const revealButton = el("export-reveal") as HTMLButtonElement;
const cancelButton = el("export-cancel") as HTMLButtonElement;
const startButton = el("export-start") as HTMLButtonElement;

/** What can be exported at the moment. */
export interface ExportSelection {
  /** The open note. `unsaved`: it has changes that could not be saved. */
  note: { path: string; title: string; unsaved: boolean } | null;
  /** The notes in the list, in its order; `null` while it shows the trash. */
  list: {
    notes: string[];
    /** Search hits in the trash, which are left out. */
    inTrash: number;
    /** The title of one document made of them. */
    name: string | null;
    /** What the list shows, in words; empty for all notes. */
    label: string;
  } | null;
}

export interface ExportHooks {
  vaultOpen(): boolean;
  /** Saves the open note and says what can be exported. */
  selection(): Promise<ExportSelection>;
  /** Opens a note by its path below notes/. */
  openNote(path: string, title: string): void;
}

/** The choices kept for the next export. */
interface Settings {
  format?: ExportFormat;
  layout?: ExportLayout;
  paper?: ExportPaper;
  keepTagLine?: boolean;
}

function loadSettings(): Settings {
  try {
    return JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "{}") as Settings;
  } catch {
    return {};
  }
}

function plural(count: number, one: string, many: string): string {
  return `${String(count)} ${count === 1 ? one : many}`;
}

/** Says which files got a number, so nothing looks replaced or lost. */
function numberedSentence(names: string[]): string {
  const shown = names.slice(0, NUMBERED_SHOWN).join(", ");
  const more = names.length - NUMBERED_SHOWN;
  const rest = more > 0 ? ` and ${String(more)} more` : "";
  return names.length === 1
    ? `1 file got a number, as its name was already taken: ${shown}.`
    : `${String(names.length)} files got a number, as their names were already taken: ${shown}${rest}.`;
}

export function createExport(hooks: ExportHooks): { open(): Promise<void> } {
  let selection: ExportSelection = { note: null, list: null };
  /** Where the last export went, for "Show in Finder". */
  let target: string | null = null;
  /** Picking or writing: the dialog cannot be closed. */
  let busy = false;

  function showError(message: string | null): void {
    errorText.textContent = message ?? "";
    errorText.hidden = message === null;
  }

  function format(): ExportFormat {
    return formatSelect.value as ExportFormat;
  }

  /** A folder gives a single note nothing unless its attachments come along. */
  function layoutMatters(): boolean {
    return chosenNotes().length > 1 || format() === COPIES_ATTACHMENTS;
  }

  function layout(): ExportLayout {
    return layoutMany.checked && layoutMatters() ? "filePerNote" : "singleFile";
  }

  /** The notes chosen, by path. */
  function chosenNotes(): string[] {
    if (whatNote.checked) return selection.note ? [selection.note.path] : [];
    return selection.list?.notes ?? [];
  }

  /** The note the chosen notes consist of, or the name of the list. */
  function title(): string | null {
    return whatNote.checked ? (selection.note?.title ?? null) : (selection.list?.name ?? null);
  }

  /** Brings the form in line with the current selection and choices. */
  function update(): void {
    const { note, list } = selection;
    whatNote.disabled = note === null;
    noteLabel.textContent = note ? `This note: ${note.title}` : "This note (none is open)";
    whatList.disabled = list === null || list.notes.length === 0;
    if (list === null) {
      listLabel.textContent = "The notes in the list (it shows the trash now)";
    } else {
      const shown = plural(list.notes.length, "note", "notes");
      const left = list.inTrash > 0 ? `, without ${String(list.inTrash)} in the trash` : "";
      listLabel.textContent =
        list.label === "" ? `All notes in the list (${shown})` : `${list.label} (${shown}${left})`;
    }
    if (whatNote.disabled) whatList.checked = true;
    if (whatList.disabled) whatNote.checked = true;

    const several = chosenNotes().length > 1;
    const copies = format() === COPIES_ATTACHMENTS;
    filesBox.hidden = !layoutMatters();
    oneLabel.textContent = several ? "One document" : "One file";
    if (several) {
      manyLabel.textContent = copies
        ? "One file per note, in a folder with copies of their attachments"
        : "One file per note, in a folder";
    } else {
      manyLabel.textContent = "In a folder, with copies of its attachments";
    }
    paperRow.hidden = !WITH_PAGES.includes(format());

    const chosen = chosenNotes();
    const unsaved = note?.unsaved === true && chosen.includes(note.path);
    unsavedText.hidden = !unsaved;
    unsavedText.textContent = unsaved
      ? `“${note.title}” has changes that are not saved. The export holds the version last saved.`
      : "";
    startButton.disabled = busy || chosen.length === 0;
  }

  function saveSettings(): void {
    const settings: Settings = {
      format: format(),
      layout: layoutMany.checked ? "filePerNote" : "singleFile",
      paper: paperSelect.value as ExportPaper,
      keepTagLine: keepTags.checked,
    };
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    } catch {
      // storage unavailable: the choices are simply not remembered
    }
  }

  function showForm(): void {
    choose.hidden = false;
    report.hidden = true;
    progressBox.hidden = true;
    revealButton.hidden = true;
    startButton.hidden = false;
    cancelButton.textContent = "Cancel";
    showError(null);
  }

  async function open(): Promise<void> {
    // Another dialog is open, or this one.
    if (document.querySelector("dialog[open]")) return;
    showForm();
    target = null;
    selection = { note: null, list: null };
    busy = false;
    whatNote.checked = true;
    if (!hooks.vaultOpen()) {
      update();
      showError("Open a vault first: the notes are exported from it.");
      dialog.showModal();
      return;
    }
    try {
      const [{ formats, pandoc }, selected] = await Promise.all([
        invoke<ExportChoices>("export_choices"),
        hooks.selection(),
      ]);
      selection = selected;
      const settings = loadSettings();
      formatSelect.replaceChildren(
        ...formats.map((value) => {
          const option = document.createElement("option");
          option.value = value;
          const through = (value === "docx" || value === "epub") && pandoc !== null;
          option.textContent = through
            ? `${FORMAT_LABELS[value]}, through Pandoc ${pandoc}`
            : FORMAT_LABELS[value];
          return option;
        }),
      );
      noPandoc.hidden = pandoc !== null;
      if (settings.format && formats.includes(settings.format)) {
        formatSelect.value = settings.format;
      }
      paperSelect.value = settings.paper === "letter" ? "letter" : "a4";
      keepTags.checked = settings.keepTagLine === true;
      layoutMany.checked = settings.layout === "filePerNote";
      layoutOne.checked = !layoutMany.checked;
    } catch (e) {
      showError(asError(e).message);
    }
    update();
    if (!dialog.open) dialog.showModal();
  }

  function close(): void {
    if (!busy) dialog.close();
  }

  function setBusy(value: boolean): void {
    busy = value;
    cancelButton.disabled = value;
    for (const control of choose.querySelectorAll<HTMLInputElement | HTMLSelectElement>(
      "input, select",
    )) {
      control.disabled = value;
    }
    if (!value) update();
    else startButton.disabled = true;
  }

  async function start(): Promise<void> {
    showError(null);
    saveSettings();
    setBusy(true);
    try {
      const chosenLayout = layout();
      const picked = await invoke<string | null>("pick_export", {
        format: format(),
        layout: chosenLayout,
        title: title(),
      });
      if (picked === null) return;
      // The list may have changed while the save dialog was open.
      selection = await hooks.selection();
      const notes = chosenNotes();
      if (notes.length === 0) {
        showError("These notes are no longer there.");
        return;
      }
      progressText.textContent = `Exporting ${plural(notes.length, "note", "notes")}…`;
      progressBox.hidden = false;
      const result = await invoke<ExportReport>("export_notes", {
        format: format(),
        input: { notes, name: whatNote.checked ? null : (selection.list?.name ?? null) },
        options: {
          layout: chosenLayout,
          keepTagLine: keepTags.checked,
          paper: paperSelect.value,
        },
        to: picked,
      });
      target = picked;
      showResult(result, picked, chosenLayout);
    } catch (e) {
      const err = asError(e);
      // Refused before anything was written; the user can choose another place.
      showError(
        err.kind === "insideVault"
          ? err.message
          : `${err.message}. Files written up to then stay where they are.`,
      );
    } finally {
      progressBox.hidden = true;
      setBusy(false);
    }
  }

  function showResult(result: ExportReport, to: string, chosenLayout: ExportLayout): void {
    choose.hidden = true;
    startButton.hidden = true;
    cancelButton.textContent = "Close";
    report.hidden = false;
    const written = result.files.length;
    if (written === 0) {
      target = null;
      summary.textContent = "Nothing was exported.";
    } else if (chosenLayout === "singleFile") {
      summary.textContent = `Exported to ${to}.`;
    } else {
      summary.textContent = `Exported ${plural(written, "file", "files")} to ${to}.`;
    }
    numberedText.hidden = result.numbered.length === 0;
    numberedText.textContent = numberedSentence(result.numbered);
    warningsList.hidden = result.warnings.length === 0;
    warningsList.replaceChildren(
      ...result.warnings.map((warning) => {
        const item = document.createElement("li");
        item.textContent = warning;
        return item;
      }),
    );
    const count = result.problems.length;
    if (count > 0 && written > 0) {
      summary.textContent += ` ${plural(count, "thing is", "things are")} different from the notes:`;
    }
    problemsTable.hidden = count === 0;
    problems.replaceChildren(...result.problems.map(problemRow));
    revealButton.hidden = target === null;
    revealButton.textContent = /Mac/.test(navigator.userAgent)
      ? "Show in Finder"
      : "Show in folder";
  }

  function problemRow(problem: ExportProblem): HTMLTableRowElement {
    const row = document.createElement("tr");
    const note = document.createElement("td");
    const button = document.createElement("button");
    button.type = "button";
    button.className = "link";
    button.textContent = problem.title;
    button.title = "Open this note";
    button.addEventListener("click", () => {
      dialog.close();
      hooks.openNote(problem.path, problem.title);
    });
    note.append(button);
    const what = document.createElement("td");
    what.textContent = PROBLEM_LABELS[problem.kind](problem.target);
    row.append(note, what);
    return row;
  }

  async function reveal(): Promise<void> {
    if (target === null) return;
    showError(null);
    try {
      await invoke("reveal_export", { path: target });
    } catch (e) {
      showError(asError(e).message);
    }
  }

  for (const control of [whatNote, whatList, layoutOne, layoutMany, formatSelect]) {
    control.addEventListener("change", update);
  }
  startButton.addEventListener("click", () => void start());
  revealButton.addEventListener("click", () => void reveal());
  cancelButton.addEventListener("click", close);
  // Escape: like Cancel, but not while exporting.
  dialog.addEventListener("cancel", (event) => {
    event.preventDefault();
    close();
  });

  return { open };
}
