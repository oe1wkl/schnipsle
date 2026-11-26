// SPDX-License-Identifier: Apache-2.0
// Spike only (docs/spikes/icloud-container.md): Settings › Diagnostics runs the
// iCloud container probe of src-tauri `icloud_probe.rs` and shows its report.
// Built here and not translated, as it is removed after the spike.
import { invoke } from "@tauri-apps/api/core";

import { asError, el } from "./dom";

type Action = "report" | "make" | "remove";

function button(label: string): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.textContent = label;
  return b;
}

const box = el("icloud-probe");
const heading = document.createElement("h4");
heading.textContent = "iCloud probe (spike)";
const actions: Record<Action, HTMLButtonElement> = {
  report: button("Probe"),
  make: button("Make test vault"),
  remove: button("Remove test vault"),
};
const copyButton = button("Copy");
copyButton.disabled = true;
const status = document.createElement("span");
status.className = "muted";
status.setAttribute("role", "status");
const row = document.createElement("div");
row.className = "folder-buttons";
row.append(actions.report, actions.make, actions.remove, copyButton, status);
const output = document.createElement("textarea");
output.className = "diagnostics-text";
output.readOnly = true;
output.rows = 12;
output.setAttribute("aria-label", "iCloud probe");
output.hidden = true;
box.append(heading, row, output);

async function run(action: Action): Promise<void> {
  for (const b of Object.values(actions)) b.disabled = true;
  status.textContent = "Probing…";
  try {
    output.value = await invoke<string>("icloud_probe", { action });
    status.textContent = "";
  } catch (e) {
    output.value = asError(e).message;
    status.textContent = "Failed";
  } finally {
    for (const b of Object.values(actions)) b.disabled = false;
  }
  output.hidden = false;
  copyButton.disabled = false;
}

for (const action of Object.keys(actions) as Action[]) {
  actions[action].addEventListener("click", () => void run(action));
}

copyButton.addEventListener("click", () => {
  void navigator.clipboard.writeText(output.value).then(
    () => {
      status.textContent = "Copied";
    },
    () => {
      status.textContent = "Select the text to copy it";
      output.focus();
      output.select();
    },
  );
});
