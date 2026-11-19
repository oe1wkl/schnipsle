// SPDX-License-Identifier: Apache-2.0
// Development playground for renaming a tag (ui/dev/retag.html), without
// Tauri: the app's page is loaded for its elements, and a stand-in for Tauri's
// IPC answers the rename commands for the tag `#Radio`. Every new name except
// `funk` is a merge; a name with a space is refused, as the core refuses it.
// Parameters: `?skipped` for notes changed meanwhile, `?folders` for smart
// folders that cannot be changed, `?failed` for a rename that fails.
import type { RetagOutcome, RetagPlan } from "../types";

const params = new URLSearchParams(location.search);
const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** An error as a command returns it, with its `kind` and the name it is about. */
const failure = (kind: string, message: string, value: string | null = null): Error =>
  Object.assign(new Error(message), { kind, currentHash: null, value });

const TITLES = ["Antenna tuner", "QRP kit", "Field day 2026", "Club history"];

function plan(old: string, name: string): RetagPlan {
  return {
    old,
    new: name,
    merge: name.toLowerCase() !== "funk" && name.toLowerCase() !== old.toLowerCase(),
    notes: TITLES.map((title, i) => ({
      path: `${title.toLowerCase().replaceAll(" ", "-")}.md`,
      title,
      hash: String(i),
      occurrences: i + 1,
    })),
    folders: [
      { id: "1", name: "Open radio tasks", query: "#radio has:task", renamed: `#${name} has:task` },
    ],
    skipped: [],
  };
}

async function answer(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  await wait(250);
  switch (cmd) {
    case "plan_tag_rename": {
      const name = String(args?.new);
      if (/\s/.test(name)) {
        throw failure(
          "notATag",
          `#${name} is not a tag: use letters, digits, _, - and / between levels`,
          name,
        );
      }
      return plan(String(args?.old), name);
    }
    case "rename_tag": {
      if (params.has("failed")) {
        throw failure("readOnly", "The vault is open read-only");
      }
      const p = args?.plan as RetagPlan;
      const skipped = params.has("skipped") ? p.notes.slice(0, 1) : [];
      const outcome: RetagOutcome = {
        changed: p.notes.slice(skipped.length).map((n) => n.path),
        skipped: skipped.map((n) => ({
          path: n.path,
          title: n.title,
          reason: "It changed since the rename was planned",
        })),
        folders: params.has("folders") ? 0 : p.folders.length,
        foldersError: params.has("folders") ? "smart-folders.yaml cannot be read: not YAML" : null,
      };
      return outcome;
    }
    case "undo_tag_rename":
      return {
        changed: ["qrp-kit.md", "field-day-2026.md"],
        skipped: [],
        folders: 1,
        foldersError: null,
      };
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
const { createRetag } = await import("../retag");
const retag = createRetag({
  beforeChange: () => Promise.resolve(),
  changed: (from, to) => {
    document.title = `Renamed ${from} to ${to}`;
  },
  status: (message) => {
    document.title = message;
  },
});
retag.open("Radio");
