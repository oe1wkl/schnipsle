// SPDX-License-Identifier: Apache-2.0
// Development playground for the import dialog (ui/dev/import.html), without
// Tauri: the app's page is loaded for its elements, and a stand-in for
// Tauri's IPC answers the import commands with a sample report shaped like
// the golden files in tests/fixtures/import/apple-notes/expected/.
import type { ImportOutcome, ImportPlan, ImportPreview, PlanItem } from "../types";

const ID = "x-coredata://00000000-0000-4000-8000-0000000000A1/ICNote/";

function item(id: string, title: string, path: string, problems: string[] = []): PlanItem {
  return { sourceId: ID + id, title, action: "add", reason: null, path, problems };
}

const plan: ImportPlan = {
  source: "apple-notes",
  input: "Schnipsle-Import",
  items: [
    item("p303", "Bread rolls", "Apple Notes/Notes/bread-rolls.md"),
    item("p401", "Club history: the first ten years", "Apple Notes/Notes/club-history.md"),
    {
      sourceId: "export:Mixed/Old shopping list.md",
      title: "Old shopping list",
      action: "add",
      reason: null,
      path: "Apple Notes/Mixed/old-shopping-list.md",
      problems: [],
    },
    item("p101", "Antenna tuner", "Apple Notes/Amateurfunk & Radio/QRP Kits/antenna-tuner.md"),
    item(
      "p103",
      "Keyer firmware: changes for version 2.1 and the build setup on the “bench” machine",
      "Apple Notes/Amateurfunk & Radio/keyer-firmware-changes-for-version-2.1.md",
      [
        "#define, #include at the start of a line become tags; change the text where they are not meant as tags.",
      ],
    ),
    {
      sourceId: ID + "p201",
      title: "Morning song",
      action: "skip",
      reason: "changedHere",
      path: "Apple Notes/Musik & Lieder/morning-song.md",
      problems: [],
    },
    {
      sourceId: ID + "p756",
      title: "Chocolate cake",
      action: "skip",
      reason: "duplicate",
      path: null,
      problems: [],
    },
  ],
  notices: [
    {
      sourceId: "export:Mixed/Old shopping list.md",
      title: "Old shopping list",
      message:
        "No note in the metadata has this title; it keeps the time of the export and its folder in the export.",
    },
    {
      sourceId: ID + "p302",
      title: "Pizza dough",
      message:
        "Not in the export: Notes exports only one of several notes with the same title in a folder. Give it another title in Notes and export it again.",
    },
    {
      sourceId: ID + "p405",
      title: "Bank details",
      message:
        "This note is locked, so Notes does not export it. Unlock it in Notes and export it again to import it.",
    },
  ],
};

/** `?metadata=no` starts without the metadata file. */
let hasMetadata = new URLSearchParams(location.search).get("metadata") !== "no";
const callbacks = new Map<number, (message: unknown) => void>();
let nextCallback = 1;

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function answer(cmd: string): Promise<unknown> {
  switch (cmd) {
    case "pick_import_folder":
      return "/Users/me/Schnipsle-Import";
    case "plan_import":
      await wait(300);
      return { plan, path: "/Users/me/Schnipsle-Import", hasMetadata } satisfies ImportPreview;
    case "read_apple_notes_metadata":
      await wait(1500);
      hasMetadata = true;
      return "679 notes";
    case "apply_import": {
      const total = plan.items.filter((i) => i.action !== "skip").length;
      for (let done = 1; done <= total; done++) {
        await wait(250);
        for (const callback of callbacks.values()) {
          callback({ event: "import-progress", id: 0, payload: { done, total } });
        }
      }
      return {
        runId: "01M63HM980000000000000000B",
        plan,
        written: [],
      } satisfies ImportOutcome;
    }
    case "plugin:event|listen":
      return 1;
    default:
      return null;
  }
}

Object.assign(window, {
  __TAURI_INTERNALS__: {
    invoke: (cmd: string) => answer(cmd),
    transformCallback: (callback: (message: unknown) => void) => {
      const id = nextCallback++;
      callbacks.set(id, callback);
      return id;
    },
    unregisterCallback: (id: number) => callbacks.delete(id),
  },
  __TAURI_EVENT_PLUGIN_INTERNALS__: { unregisterListener: () => undefined },
});

const page = new DOMParser().parseFromString(
  await (await fetch("/index.html")).text(),
  "text/html",
);
document.body.replaceChildren(...page.body.childNodes);
const { createImport } = await import("../import");
const importer = createImport({
  vaultState: () => ({ open: true, readOnly: null }),
  openNote: (path) => {
    document.title = `Would open ${path}`;
  },
  imported: () => undefined,
});
importer.open();
