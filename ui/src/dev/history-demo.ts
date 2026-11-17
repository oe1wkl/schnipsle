// SPDX-License-Identifier: Apache-2.0
// Development playground for the history view (ui/dev/history.html), without
// Tauri: the app's page is loaded for its elements, and a stand-in for Tauri's
// IPC answers the history commands. The note has two versions, the `before`
// and `after` files of a case in tests/fixtures/format/7-compare/, and the
// note now is the `after` text; comparisons are the core's golden JSON for
// that case. Parameters: `?case=<name>` (default `edit`), `?empty` for a note
// without versions, `?failed` for a comparison that fails, `?readonly` for a
// read-only vault. Restore and As new note only say what they would do.
/// <reference types="vite/client" />
import type { Comparison, Version } from "../types";

const params = new URLSearchParams(location.search);
const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

const files = import.meta.glob<string>("../../../tests/fixtures/format/7-compare/*", {
  query: "?raw",
  import: "default",
  eager: true,
});
const fixture = (name: string): string | undefined =>
  files[`../../../tests/fixtures/format/7-compare/${name}`];
const cases = Object.keys(files)
  .map((path) => /([^/]+)\.before\.md$/.exec(path)?.[1])
  .filter((name): name is string => name !== undefined)
  .sort();
const caseName = params.get("case") ?? "edit";
const before = fixture(`${caseName}.before.md`) ?? "";
const after = fixture(`${caseName}.after.md`) ?? "";
const compared = JSON.parse(fixture(`${caseName}.json`) ?? "null") as Comparison | null;

const OLDER = "20261117T080000Z.md.zst";
const NEWER = "20261117T090500Z.md.zst";
const VERSIONS: Version[] = params.has("empty")
  ? []
  : [
      { name: NEWER, takenMs: Date.UTC(2026, 10, 17, 9, 5) },
      { name: OLDER, takenMs: Date.UTC(2026, 10, 17, 8, 0) },
    ];

/** The newer version with the note now (the same text): nothing differs.
 * Stand-in only; the app asks the core. */
function unchanged(text: string): Comparison {
  const lines = text.split("\n");
  if (lines.at(-1) === "") lines.pop();
  const close = text.startsWith("---\n") ? lines.indexOf("---", 1) + 1 : 0;
  const body = lines.slice(close);
  const first = close + 1;
  return {
    frontMatter: [],
    added: 0,
    removed: 0,
    rows: body.length > 0 ? [{ kind: "folded", before: first, after: first, lines: body }] : [],
  };
}

async function answer(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  await wait(150);
  switch (cmd) {
    case "list_versions":
      return VERSIONS;
    case "read_version":
      return args?.name === OLDER ? before : after;
    case "compare_versions": {
      if (params.has("failed")) throw new Error("The snapshot is damaged");
      const pair = [args?.before ?? null, args?.after ?? null];
      if (pair[0] === OLDER) return compared;
      return unchanged(after);
    }
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

// The cases, to switch between them.
const nav = document.createElement("nav");
nav.style.cssText = "display:flex;flex-wrap:wrap;gap:12px;padding:6px 12px;font-size:14px";
nav.append(
  "Case:",
  ...cases.map((name) => {
    const link = document.createElement("a");
    link.href = `?case=${name}`;
    link.textContent = name;
    if (name === caseName) link.setAttribute("aria-current", "page");
    return link;
  }),
);
document.body.prepend(nav);

const { ui, setStatus } = await import("../dom");
const { createEditor } = await import("../editor");
const { createHistory, versionLabel } = await import("../history");
const preview = createEditor(ui.historyPreview, {
  onChange: () => undefined,
  onBlur: () => undefined,
  onFiles: () => undefined,
  loadImage: () => Promise.reject(new Error("no pictures in the playground")),
  placeholderText: "Select a version",
});
const history = createHistory({
  note: () => "pumpkin-soup.md",
  flush: () => Promise.resolve(),
  readOnly: () => params.has("readonly"),
  preview,
  status: setStatus,
  restore: (version) => {
    setStatus(`Would restore the version of ${versionLabel(version)}`);
  },
  copy: (version) => {
    setStatus(`Would copy the version of ${versionLabel(version)} as a new note`);
  },
});
ui.showHistory.disabled = false;
ui.layout.dataset.view = "editor";
await history.open();
