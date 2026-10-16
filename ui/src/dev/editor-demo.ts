// SPDX-License-Identifier: Apache-2.0
// Development playground for the note editor (ui/dev/editor.html), without Tauri.
import { createEditor } from "../editor";

const SAMPLE = `---
id: 01ARZ3NDEKTSV4RRFFQ69G5FAV
created: 2026-10-16T07:00:00Z
pinned: true
---
# Antenna test #radio

Some *emphasis*, **strong** and ~~struck~~ text, \`inline code\` and a [link](https://example.org "Title").

## Shopping list

- [ ] Milch
- [x] Brot
- Lötzinn 0,5 mm

> A quote with **bold** inside.

1. first
2. second

\`\`\`sh
cargo test --workspace
\`\`\`
`;

function el(id: string): HTMLElement {
  const found = document.getElementById(id);
  if (!found) throw new Error(`missing #${id}`);
  return found;
}

const status = el("status");
const editor = createEditor(el("editor-host"), {
  onChange: () => {
    status.textContent = `Changed: ${String(editor.text().length)} characters`;
  },
  onBlur: () => {
    status.textContent = "Blurred";
  },
});
editor.load(SAMPLE);
editor.setEnabled(true);

let source = false;
const mode = el("mode");
mode.addEventListener("click", () => {
  source = !source;
  editor.setMode(source ? "source" : "live");
  mode.textContent = source ? "Preview" : "Source";
});

// For checks from the browser console.
Object.assign(window, { editor });
