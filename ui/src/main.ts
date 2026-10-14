// SPDX-License-Identifier: Apache-2.0
import { invoke } from "@tauri-apps/api/core";

/** Mirrors `schnipsle_core::Tag`. */
interface Tag {
  name: string;
  key: string;
}

const note = document.querySelector<HTMLTextAreaElement>("#note");
const list = document.querySelector<HTMLUListElement>("#tags");

async function refresh(): Promise<void> {
  if (!note || !list) return;
  const tags = await invoke<Tag[]>("extract_tags", { markdown: note.value });
  list.replaceChildren(
    ...tags.map((tag) => {
      const item = document.createElement("li");
      item.textContent = `#${tag.name}`;
      item.title = tag.key;
      return item;
    }),
  );
}

let timer: number | undefined;
note?.addEventListener("input", () => {
  window.clearTimeout(timer);
  timer = window.setTimeout(() => void refresh(), 150);
});
window.addEventListener("DOMContentLoaded", () => void refresh());
