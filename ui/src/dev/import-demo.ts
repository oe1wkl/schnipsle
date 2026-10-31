// SPDX-License-Identifier: Apache-2.0
// Development playground for the import dialog (ui/dev/import.html), without
// Tauri: the app's page is loaded for its elements, and a stand-in for
// Tauri's IPC answers the import commands with a sample report shaped like
// the golden files in tests/fixtures/import/apple-notes/expected/; smaller
// ones for Simplenote, with a note from its trash, and for HTML.
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

const simplenotePlan: ImportPlan = {
  source: "simplenote",
  input: "notes.zip",
  items: [
    {
      sourceId: "0a1b2c3d4e5f40718293a4b5c6d7e8f9",
      title: "Lake trip",
      action: "add",
      reason: null,
      path: "Simplenote/lake-trip.md",
      problems: [],
    },
    {
      sourceId: "8293a4b5c6d7e8f90a1b2c3d4e5f6071",
      title: "Old idea",
      action: "add",
      reason: null,
      path: "Simplenote/old-idea.md",
      problems: [],
      trashed: true,
    },
  ],
  notices: [],
};

const htmlPlan: ImportPlan = {
  source: "html",
  input: "Saved pages",
  items: [
    {
      sourceId: "Pizza dough.html",
      title: "Pizza dough",
      action: "add",
      reason: null,
      path: "HTML/pizza-dough.md",
      problems: [],
    },
    {
      sourceId: "Radio/QRP Kits/Antenna tuner (old).html",
      title: "Antenna tuner",
      action: "add",
      reason: null,
      path: "HTML/Radio/QRP Kits/antenna-tuner.md",
      problems: [],
    },
  ],
  notices: [
    {
      sourceId: "Radio/QRP Kits/Antenna tuner (old).html",
      title: "Antenna tuner",
      message:
        "The page links to Radio/QRP Kits/pics/missing.png, which is not there; the link is left as it is.",
    },
    {
      sourceId: null,
      title: null,
      message:
        "Notes/Japanese.html is in the encoding shift_jis, which the import cannot read; it is not imported.",
    },
  ],
};
let current = plan;

/** `?metadata=no` starts without the metadata file. */
let hasMetadata = new URLSearchParams(location.search).get("metadata") !== "no";
const callbacks = new Map<number, (message: unknown) => void>();
let nextCallback = 1;

function wait(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

async function answer(cmd: string, args: Record<string, unknown> = {}): Promise<unknown> {
  switch (cmd) {
    case "pick_import":
      if (args.source === "simplenote") return "/Users/me/Downloads/notes.zip";
      if (args.source === "html") {
        return args.file === true
          ? "/Users/me/Saved pages/Pizza dough.html"
          : "/Users/me/Saved pages";
      }
      return "/Users/me/Schnipsle-Import";
    case "plan_import": {
      await wait(300);
      const simple = args.source === "simplenote";
      current = simple ? simplenotePlan : args.source === "html" ? htmlPlan : plan;
      return {
        plan: current,
        path: String(args.path),
        hasMetadata: !simple && hasMetadata,
      } satisfies ImportPreview;
    }
    case "read_apple_notes_metadata":
      await wait(1500);
      hasMetadata = true;
      return "679 notes";
    case "apply_import": {
      const total = current.items.filter((i) => i.action !== "skip").length;
      for (let done = 1; done <= total; done++) {
        await wait(250);
        for (const callback of callbacks.values()) {
          callback({ event: "import-progress", id: 0, payload: { done, total } });
        }
      }
      return {
        runId: "01M63HM980000000000000000B",
        plan: current,
        written: [],
        trashed: [],
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
    invoke: (cmd: string, args?: Record<string, unknown>) => answer(cmd, args),
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
