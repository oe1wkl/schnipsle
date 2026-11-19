// SPDX-License-Identifier: Apache-2.0
// The UI's text (ADR-0041): one catalog per language in `strings/`, English
// only in 1.0. `t` is the catalog for the user's language; `applyStrings`
// fills the page's elements that name their text by key (`data-text`,
// `data-label`, `data-title`, `data-placeholder`).
import { en, type Strings } from "./strings/en";

/** Catalogs by language code; a new language is one more `Strings` here. */
const catalogs: Partial<Record<string, Strings>> = { en };

function pick(): { lang: string; strings: Strings } {
  for (const tag of navigator.languages) {
    const lang = tag.split("-")[0]?.toLowerCase() ?? "";
    const strings = catalogs[lang];
    if (strings) return { lang, strings };
  }
  return { lang: "en", strings: en };
}

const chosen = pick();

/** The text of the UI, in the user's language. */
export const t: Strings = chosen.strings;
/** The language of `t`, for `lang` attributes and `Intl`. */
export const lang: string = chosen.lang;

/** The string at a dotted key such as `history.compare`, if there is one. */
export function lookup(key: string): string | undefined {
  let at: unknown = t;
  for (const part of key.split(".")) {
    if (typeof at !== "object" || at === null) return undefined;
    at = (at as Record<string, unknown>)[part];
  }
  return typeof at === "string" ? at : undefined;
}

const ATTRIBUTES = [
  ["data-label", "aria-label"],
  ["data-title", "title"],
  ["data-placeholder", "placeholder"],
] as const;

/** Fills the texts and attributes under `root` from the catalog. */
export function applyStrings(root: ParentNode = document): void {
  if (root === document) document.documentElement.lang = lang;
  const text = (key: string): string => {
    const found = lookup(key);
    if (found === undefined) throw new Error(`missing string ${key}`);
    return found;
  };
  for (const element of root.querySelectorAll<HTMLElement>("[data-text]")) {
    element.textContent = text(element.dataset.text ?? "");
  }
  for (const [from, to] of ATTRIBUTES) {
    for (const element of root.querySelectorAll(`[${from}]`)) {
      element.setAttribute(to, text(element.getAttribute(from) ?? ""));
    }
  }
}
