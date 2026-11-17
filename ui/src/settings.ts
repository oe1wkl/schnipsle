// SPDX-License-Identifier: Apache-2.0
// The settings view (ADR-0039): Settings… (⌘,) on the Mac, the Settings button
// on the iPhone. Settings of the vault (how long earlier versions are kept, the
// chord names of song sheets) are saved in its `settings.yaml` and apply on
// every device; settings of this Mac (the Pandoc program, the update check) are
// saved on the Mac only. Each change is saved at once, as is usual on the Mac.
// A change of the retention that would remove versions asks first, saying how
// many.
import { invoke } from "@tauri-apps/api/core";

import { asError, el } from "./dom";
import type {
  ChordNotation,
  DeviceSettings,
  Setting,
  SettingChange,
  Thinning,
  VaultSettings,
} from "./types";

export interface SettingsHooks {
  /** Whether a vault is open, and why it is read-only if it is. */
  vaultState: () => { open: false } | { open: true; readOnly: string | null };
  /** The chord names changed: song sheets in the open note are drawn again. */
  chordNamesChanged: () => void;
}

const dialog = el("settings-dialog") as HTMLDialogElement;
const vaultFields = el("settings-vault-fields");
const vaultHint = el("settings-vault-hint");
const keepAll = el("settings-keep-all") as HTMLInputElement;
const keepDaily = el("settings-keep-daily") as HTMLInputElement;
const retentionNote = el("settings-retention-note");
const chords = el("settings-chords") as HTMLSelectElement;
const chordsNote = el("settings-chords-note");
const confirmBox = el("settings-confirm");
const confirmText = el("settings-confirm-text");
const confirmYes = el("settings-confirm-yes") as HTMLButtonElement;
const confirmNo = el("settings-confirm-no") as HTMLButtonElement;
const vaultError = el("settings-vault-error");
const device = el("settings-device");
const pandocStatus = el("settings-pandoc-status");
const pandocPick = el("settings-pandoc-pick") as HTMLButtonElement;
const pandocFind = el("settings-pandoc-find") as HTMLButtonElement;
const updates = el("settings-updates") as HTMLInputElement;
const deviceError = el("settings-device-error");
const closeButton = el("settings-close") as HTMLButtonElement;

function plural(count: number, one: string, many: string): string {
  return `${String(count)} ${count === 1 ? one : many}`;
}

function showError(target: HTMLElement, message: string | null): void {
  target.textContent = message ?? "";
  target.hidden = message === null;
}

/** What to say about a setting whose file value does not apply as written. */
function deviationText(name: string, setting: Setting<unknown>, valid: string): string | null {
  if (setting.deviation === "invalid") {
    return `The vault's settings file has “${setting.written ?? ""}” for ${name}, which is not ${valid}; the default applies.`;
  }
  return null;
}

/** The text below the retention fields: the defaults, and what does not apply as written. */
function retentionText(s: VaultSettings): string {
  const notes = [
    deviationText("keeping every version", s.historyKeepAllDays, "a whole number of days"),
    deviationText("keeping one a day", s.historyKeepDailyDays, "a whole number of days"),
  ];
  if (s.historyKeepDailyDays.deviation === "extended") {
    notes.push(
      `One a day is kept until ${String(s.historyKeepDailyDays.value)} days, as every version is kept that long.`,
    );
  }
  notes.push(
    `Defaults: every version for ${String(s.historyKeepAllDays.default)} days, one a day until ${String(s.historyKeepDailyDays.default)} days.`,
  );
  return notes.filter((n) => n !== null).join(" ");
}

/** A number of days typed into `input`, or `null` if it is not a whole number ≥ 0. */
function days(input: HTMLInputElement): number | null {
  const text = input.value.trim();
  return /^\d{1,6}$/.test(text) ? Number(text) : null;
}

/** The change that sets `value`, removing the key when it is the default (ADR-0039). */
function daysChange(
  key: "historyKeepAllDays" | "historyKeepDailyDays",
  value: number,
  setting: Setting<number>,
): SettingChange {
  return { key, value: value === setting.default ? null : value };
}

export function createSettings(hooks: SettingsHooks): { open(): Promise<void> } {
  let settings: VaultSettings | null = null;
  /** A change waiting for the user to agree that versions are removed. */
  let pending: SettingChange[] | null = null;
  /** Bumped when the dialog opens, so an answer for an earlier opening is dropped. */
  let generation = 0;

  function showVault(s: VaultSettings | null): void {
    settings = s;
    if (s === null) return;
    keepAll.value = String(s.historyKeepAllDays.value);
    // An extended daily period shows what the file says; the note below explains.
    const daily = s.historyKeepDailyDays;
    keepDaily.value =
      daily.deviation === "extended" && daily.written !== null
        ? daily.written
        : String(daily.value);
    retentionNote.textContent = retentionText(s);
    chords.value = s.chordNotation.value;
    const note = deviationText("the chord names", s.chordNotation, "“english” or “german”");
    chordsNote.textContent = note ?? "";
    chordsNote.hidden = note === null;
  }

  function showDevice(d: DeviceSettings | null): void {
    device.hidden = d === null;
    if (d === null) return;
    if (d.pandocProblem !== null) {
      pandocStatus.textContent = `${d.pandocChosen ?? ""} does not run: ${d.pandocProblem}. Word and EPUB are not offered.`;
    } else if (d.pandoc === null) {
      pandocStatus.textContent =
        "Not found. Install Pandoc (for example with Homebrew) or choose the program.";
    } else {
      const how = d.pandocChosen === null ? "found" : "chosen";
      pandocStatus.textContent = `Pandoc ${d.pandoc.version}, ${how} at ${d.pandoc.path}`;
    }
    pandocFind.hidden = d.pandocChosen === null;
    updates.checked = d.checkForUpdates;
  }

  async function open(): Promise<void> {
    const asked = ++generation;
    showError(vaultError, null);
    showError(deviceError, null);
    confirmBox.hidden = true;
    pending = null;
    const state = hooks.vaultState();
    vaultFields.hidden = !state.open;
    if (!state.open) {
      vaultHint.textContent = "Open a vault to see its settings.";
    } else if (state.readOnly !== null) {
      vaultHint.textContent = `The vault is open read-only, so its settings cannot be changed. ${state.readOnly}`;
    } else {
      vaultHint.textContent = "Saved in the vault, so they apply on every device that opens it.";
    }
    for (const control of [keepAll, keepDaily, chords]) {
      control.disabled = !state.open || state.readOnly !== null;
    }
    const [vault, here] = await Promise.all([
      state.open
        ? invoke<VaultSettings>("read_settings").catch((e: unknown) => {
            showError(vaultError, `The vault's settings cannot be read: ${asError(e).message}`);
            vaultFields.hidden = true;
            return null;
          })
        : Promise.resolve(null),
      invoke<DeviceSettings | null>("read_device_settings").catch((e: unknown) => {
        showError(deviceError, `This Mac's settings cannot be read: ${asError(e).message}`);
        return null;
      }),
    ]);
    if (asked !== generation) return;
    showVault(vault);
    showDevice(here);
    if (!dialog.open) dialog.showModal();
  }

  /** Saves `changes` to the vault; the dialog shows what is in effect afterwards. */
  async function save(changes: SettingChange[]): Promise<void> {
    showError(vaultError, null);
    try {
      const saved = await invoke<VaultSettings>("write_settings", { changes });
      const notationBefore = settings?.chordNotation.value;
      showVault(saved);
      if (saved.chordNotation.value !== notationBefore) hooks.chordNamesChanged();
    } catch (e) {
      showError(vaultError, `Not saved: ${asError(e).message}`);
      showVault(settings);
    }
  }

  /** The retention fields changed: saved at once, or after asking if versions would go. */
  async function retentionChanged(): Promise<void> {
    const s = settings;
    if (s === null) return;
    const all = days(keepAll);
    const daily = days(keepDaily);
    if (all === null || daily === null) {
      showError(vaultError, "Enter a whole number of days.");
      return;
    }
    showError(vaultError, null);
    const changes = [
      daysChange("historyKeepAllDays", all, s.historyKeepAllDays),
      daysChange("historyKeepDailyDays", daily, s.historyKeepDailyDays),
    ];
    let thinning: Thinning;
    try {
      thinning = await invoke<Thinning>("history_to_thin", { allDays: all, dailyDays: daily });
    } catch (e) {
      showError(vaultError, `Not saved: ${asError(e).message}`);
      return;
    }
    if (thinning.versions === 0) {
      await save(changes);
      return;
    }
    pending = changes;
    confirmText.textContent = `This removes ${plural(thinning.versions, "earlier version", "earlier versions")} of ${plural(thinning.notes, "note", "notes")} for good, on every device.`;
    confirmBox.hidden = false;
    for (const control of [keepAll, keepDaily, chords, closeButton]) control.disabled = true;
    confirmNo.focus();
  }

  function endConfirm(): void {
    pending = null;
    confirmBox.hidden = true;
    for (const control of [keepAll, keepDaily, chords, closeButton]) control.disabled = false;
  }

  async function changeDevice(command: string, args?: Record<string, unknown>): Promise<void> {
    showError(deviceError, null);
    try {
      const shown = await invoke<DeviceSettings | null>(command, args);
      if (shown !== null) showDevice(shown);
    } catch (e) {
      showError(deviceError, asError(e).message);
      void invoke<DeviceSettings | null>("read_device_settings").then(showDevice);
    }
  }

  keepAll.addEventListener("change", () => void retentionChanged());
  keepDaily.addEventListener("change", () => void retentionChanged());
  chords.addEventListener("change", () => {
    const s = settings;
    if (s === null) return;
    const value = chords.value as ChordNotation;
    void save([{ key: "chordNotation", value: value === s.chordNotation.default ? null : value }]);
  });
  confirmYes.addEventListener("click", () => {
    const changes = pending;
    endConfirm();
    if (changes) void save(changes);
  });
  confirmNo.addEventListener("click", () => {
    endConfirm();
    showVault(settings);
    keepAll.focus();
  });
  pandocPick.addEventListener("click", () => void changeDevice("pick_pandoc"));
  pandocFind.addEventListener("click", () => void changeDevice("find_pandoc_again"));
  updates.addEventListener("change", () => {
    void changeDevice("set_update_check", { on: updates.checked });
  });
  closeButton.addEventListener("click", () => {
    dialog.close();
  });
  // Escape: like Done, but not while a removal waits for an answer.
  dialog.addEventListener("cancel", (event) => {
    if (pending !== null) {
      event.preventDefault();
      confirmNo.click();
    }
  });

  return { open };
}
