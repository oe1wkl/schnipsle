// SPDX-License-Identifier: Apache-2.0
// Development playground for the export dialog (ui/dev/export.html), without
// Tauri: the app's page is loaded for its elements, and a stand-in for Tauri's
// IPC answers the export commands. Parameters: `?clean` for an export without
// problems, `?failed` for one that stops with an error, `?nothing` for one with
// no note readable, `?numbered` for files with names already taken, `?warnings`
// for what a converter said, `?nopandoc` without Pandoc, `?unsaved` for an open note with unsaved changes, `?nonote`
// with no note open.
import type { ExportProblem, ExportReport } from "../types";

const params = new URLSearchParams(location.search);
const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

function problem(
  title: string,
  path: string,
  kind: ExportProblem["kind"],
  target: string,
): ExportProblem {
  return { title, path, kind, target };
}

const PROBLEMS = [
  problem("Bread rolls", "bread-rolls.md", "pictureMissing", "attachments/ab/crumb.jpg"),
  problem("Antenna tuner", "antenna-tuner.md", "webPictureAsLink", "https://example.org/kit.png"),
  problem("Antenna tuner", "antenna-tuner.md", "attachmentNotIncluded", "attachments/cd/kit.pdf"),
  problem("Club history", "club-history.md", "noteNotExported", "founders.md"),
  problem("Morning song", "morning-song.md", "noteNotRead", ""),
];

async function answer(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "export_choices":
      return params.has("nopandoc")
        ? { formats: ["html", "pdf", "odt"], pandoc: null }
        : { formats: ["html", "pdf", "odt", "docx", "epub"], pandoc: "3.12.1" };
    case "pick_export":
      await wait(300);
      return args?.layout === "singleFile"
        ? `/Users/you/Documents/${typeof args.title === "string" ? args.title : "Notes"}.${String(args.format)}`
        : "/Users/you/Documents/Export";
    case "export_notes": {
      await wait(1200);
      if (params.has("failed")) throw new Error("The disk is full");
      const report: ExportReport = params.has("nothing")
        ? { files: [], numbered: [], warnings: [], problems: PROBLEMS.slice(4) }
        : {
            files: ["one-2.pdf", "two-2.pdf"],
            numbered: params.has("numbered") ? ["one-2.pdf", "two-2.pdf"] : [],
            warnings: params.has("warnings")
              ? [
                  "Bread rolls: Could not convert image picture-1.svg: check that rsvg-convert is in path.",
                ]
              : [],
            problems: params.has("clean") ? [] : PROBLEMS,
          };
      return report;
    }
    case "reveal_export":
      document.title = `Would show ${String(args?.path)}`;
      return null;
    default:
      return null;
  }
}

Object.assign(window, {
  __TAURI_INTERNALS__: {
    invoke: (cmd: string, args?: Record<string, unknown>) => answer(cmd, args),
  },
});

const page = new DOMParser().parseFromString(
  await (await fetch("/index.html")).text(),
  "text/html",
);
document.body.replaceChildren(...page.body.childNodes);
const { createExport } = await import("../export");
const exporter = createExport({
  vaultOpen: () => true,
  selection: () =>
    Promise.resolve({
      note: params.has("nonote")
        ? null
        : {
            path: "antenna-tuner.md",
            title: "Antenna tuner",
            unsaved: params.has("unsaved"),
          },
      list: {
        notes: ["antenna-tuner.md", "bread-rolls.md", "club-history.md"],
        inTrash: 1,
        name: "#projekte",
        label: "#projekte",
      },
    }),
  openNote: (path) => {
    document.title = `Would open ${path}`;
  },
});
await exporter.open();
