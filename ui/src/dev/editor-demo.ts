// SPDX-License-Identifier: Apache-2.0
// Development playground for the note editor (ui/dev/editor.html), without Tauri.
/// <reference types="vite/client" />
import { createEditor } from "../editor";
import type { SongHost } from "../song";
import type { SongSheet } from "../types";
// Without the core, song sheets come from its golden files (format.md §11).
import song from "../../../tests/fixtures/format/5.7-render/song.cho?raw";
import songPlus3 from "../../../tests/fixtures/format/5.7-render/song+3.cho?raw";
import plain from "../../../tests/fixtures/format/5.7-render/render.json";
import german from "../../../tests/fixtures/format/5.7-render/render-german.json";
import lyricsOnly from "../../../tests/fixtures/format/5.7-render/render-lyrics-only.json";
import transposed from "../../../tests/fixtures/format/5.7-render/render-transposed.json";

const SAMPLE = `---
id: 01ARZ3NDEKTSV4RRFFQ69G5FAV
created: 2026-10-16T07:00:00Z
pinned: true
---
# Antenna test #radio

Some *emphasis*, **strong** and ~~struck~~ text, \`inline code\` and a [link](https://example.org "Title").

Links to open with ⌘-click: the [manual](<../attachments/61/manual v2.pdf>), the [tuner](Projects/Tuner.md), <https://example.org/club> and www.example.org.

## Shopping list

- [ ] Milch
- [x] Brot
- Lötzinn 0,5 mm

> A quote with **bold** inside.

![Schaltplan](../attachments/3f/3f9a.svg) and an image that is missing: ![Datenblatt](gone.png)

1. first
2. second

| Band | Frequenz | Notiz |
|:-----|--------:|:-----:|
| 40 m | 7,030 MHz | QRP \\| CW |
| 20 m | 14,060 MHz | *gut* |

\`\`\`sh
cargo test --workspace
\`\`\`

\`\`\`chordpro
${song.trimEnd()}
\`\`\`
`;

/**
 * The golden sheets for the plain view, lyrics only, German names and any
 * transposition; "Apply transposition" gives the song moved by 3.
 */
const songs: SongHost = {
  render: (source, _info, view) => {
    if (source.trim() !== song.trim() && source.trim() !== songPlus3.trim()) {
      return Promise.reject(new Error("the demo knows only the fixture song"));
    }
    const sheet = view.lyricsOnly
      ? lyricsOnly
      : view.shown === "german"
        ? german
        : view.transpose !== 0
          ? transposed
          : plain;
    return Promise.resolve({ ...sheet, transpose: view.transpose } as SongSheet);
  },
  applyTransposition: () => Promise.resolve(songPlus3.trimEnd()),
  report: (message) => {
    status.textContent = message;
  },
};

const DRAWING = `<svg xmlns="http://www.w3.org/2000/svg" width="240" height="80">
<rect width="240" height="80" rx="8" fill="#d0e4ff"/><text x="20" y="48" font-size="20">Schaltplan</text></svg>`;

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
  onFiles: (files, pos) => {
    const links = files
      .map((f) => `![${f.name}](../attachments/${encodeURIComponent(f.name)})`)
      .join("\n");
    editor.insert(links, pos);
  },
  // Every `.svg` link shows the same drawing; anything else is missing.
  loadImage: (url) =>
    url.endsWith(".svg")
      ? Promise.resolve(`data:image/svg+xml,${encodeURIComponent(DRAWING)}`)
      : Promise.reject(new Error("not found")),
  openLink: (link) => {
    status.textContent = `Open: ${link}`;
  },
  songs,
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
