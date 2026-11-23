// SPDX-License-Identifier: Apache-2.0
// Development playground for the settings view (ui/dev/settings.html), without
// Tauri: the app's page is loaded for its elements, and a stand-in for Tauri's
// IPC keeps the vault's settings in memory. Parameters: `?custom` for a vault
// with its own values, `?invalid` for a settings file with values that do not
// apply, `?readonly` for a read-only vault, `?novault` with none open, `?ios`
// as on the iPhone (no settings of the device), `?nopandoc` without Pandoc,
// `?chosen` with a Pandoc picked, `?broken` with a picked one that does not
// run, `?failed` for saving that fails, `?unmeasured` with nothing measured
// yet and no memory. Shortening the retention below
// 7 / 90 days counts versions to remove. What would be written is shown in the
// page title.
import type {
  ChordNotation,
  DeviceSettings,
  Diagnostics,
  MeasureSeries,
  Setting,
  SettingChange,
  VaultSettings,
} from "../types";

const params = new URLSearchParams(location.search);
const wait = (ms: number): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));

/** The keys of the stand-in `settings.yaml` and their written values. */
const file = new Map<string, string>();
if (params.has("custom")) {
  file.set("history_keep_all_days", "14");
  file.set("history_keep_daily_days", "180");
  file.set("chord_notation", "german");
} else if (params.has("invalid")) {
  file.set("history_keep_all_days", "30");
  file.set("history_keep_daily_days", "10");
  file.set("chord_notation", "latin");
}

function setting<T>(key: string, fallback: T, parse: (text: string) => T | null): Setting<T> {
  const written = file.get(key) ?? null;
  const parsed = written === null ? null : parse(written);
  return {
    value: parsed ?? fallback,
    default: fallback,
    written,
    deviation: written !== null && parsed === null ? "invalid" : null,
  };
}

const number = (text: string): number | null => (/^\d+$/.test(text) ? Number(text) : null);

function vaultSettings(): VaultSettings {
  const all = setting("history_keep_all_days", 7, number);
  const daily = setting("history_keep_daily_days", 90, number);
  if (daily.value < all.value) {
    daily.value = all.value;
    daily.deviation ??= "extended";
  }
  return {
    historyKeepAllDays: all,
    historyKeepDailyDays: daily,
    chordNotation: setting<ChordNotation>("chord_notation", "english", (text) =>
      text === "english" || text === "german" ? text : null,
    ),
  };
}

const KEYS: Record<SettingChange["key"], string> = {
  historyKeepAllDays: "history_keep_all_days",
  historyKeepDailyDays: "history_keep_daily_days",
  chordNotation: "chord_notation",
};

let device: DeviceSettings = {
  pandocChosen: params.has("chosen") || params.has("broken") ? "/Users/you/bin/pandoc" : null,
  pandoc:
    params.has("nopandoc") || params.has("broken")
      ? null
      : {
          path: params.has("chosen") ? "/Users/you/bin/pandoc" : "/opt/homebrew/bin/pandoc",
          version: "3.12.1",
        },
  pandocProblem: params.has("broken") ? "No such file or directory (os error 2)" : null,
  checkForUpdates: true,
};

/** Times like those of a vault of 10,000 notes (ADR-0045), invented. */
function series(kind: string, times: number[], detail: MeasureSeries["recent"][0]["detail"]) {
  const recent = times.map((ms, i) => ({ ms, detail, atMs: Date.now() - i * 60_000 }));
  const slowest = recent.reduce((a, b) => (b.ms > a.ms ? b : a));
  return { kind, count: times.length + 3, recent, slowest };
}

function diagnostics(): Diagnostics {
  const unmeasured = params.has("unmeasured");
  return {
    version: "0.1.0",
    device: params.has("ios") ? "iPhone18,1" : "Mac16,10",
    system: params.has("ios") ? "ios 27.1" : "macos 27.2",
    memory: unmeasured ? null : { now: 84_300_000, peak: 131_900_000 },
    memoryProblem: unmeasured ? "task_info failed: 5" : null,
    series: unmeasured
      ? []
      : [
          series("search", [12.4, 3.1, 34.2, 8.7], { notes: 412, query: "kabel" }),
          series("searchUi", [21.9, 9.5, 48.0, 15.2], { notes: 412 }),
          series("start", [812], {}),
          series("indexSync", [5400, 41], { notes: 10_000, read: 0, removed: 0 }),
          series("readNoteUi", [18.3, 22.1], { kilobytes: 3 }),
          series("export", [2310], { format: "pdf", notes: 25 }),
          series("somethingNew", [1.2], {}),
        ],
  };
}

async function answer(cmd: string, args?: Record<string, unknown>): Promise<unknown> {
  switch (cmd) {
    case "exports_are_shared":
      return params.has("ios");
    case "read_settings":
      return vaultSettings();
    case "history_to_thin": {
      await wait(200);
      const all = Number(args?.allDays);
      const daily = Math.max(Number(args?.dailyDays), all);
      const versions = Math.max(0, 7 - all) * 5 + Math.max(0, 90 - daily) * 2;
      return { versions, notes: Math.ceil(versions / 6) };
    }
    case "write_settings": {
      await wait(300);
      if (params.has("failed")) throw new Error("The disk is full");
      for (const change of args?.changes as SettingChange[]) {
        if (change.value === null) file.delete(KEYS[change.key]);
        else file.set(KEYS[change.key], String(change.value));
      }
      document.title = [...file].map(([k, v]) => `${k}: ${v}`).join(" | ") || "(empty file)";
      return vaultSettings();
    }
    case "read_device_settings":
      return params.has("ios") ? null : device;
    case "diagnostics":
      return diagnostics();
    case "set_update_check":
      device = { ...device, checkForUpdates: args?.on === true };
      return device;
    case "pick_pandoc":
      await wait(300);
      device = {
        pandocChosen: "/opt/pandoc-3.6/bin/pandoc",
        pandoc: { path: "/opt/pandoc-3.6/bin/pandoc", version: "3.6" },
        pandocProblem: null,
        checkForUpdates: device.checkForUpdates,
      };
      return device;
    case "find_pandoc_again":
      device = {
        pandocChosen: null,
        pandoc: params.has("nopandoc")
          ? null
          : { path: "/opt/homebrew/bin/pandoc", version: "3.12.1" },
        pandocProblem: null,
        checkForUpdates: device.checkForUpdates,
      };
      return device;
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
const { createSettings } = await import("../settings");
const settings = createSettings({
  vaultState: () =>
    params.has("novault")
      ? { open: false }
      : {
          open: true,
          readOnly: params.has("readonly")
            ? "This vault uses format 2, but this version of Schnipsle only knows format 1."
            : null,
        },
  chordNamesChanged: () => {
    console.info("chord names changed: the open note would be read again");
  },
});
await settings.open();
