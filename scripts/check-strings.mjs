// SPDX-License-Identifier: Apache-2.0
// Checks the page against the string catalog (ADR-0041 rule 3): every key
// that ui/index.html names in data-text, data-label, data-title or
// data-placeholder is a string in ui/src/strings/en.ts, and the page has no
// text of its own outside them. Node loads the catalog by stripping its
// types (Node 22.18 and later). Run by `pnpm check`.
import { readFileSync } from "node:fs";

import { en } from "../ui/src/strings/en.ts";

/** Words the page may hold itself: the app's name. */
const ALLOWED = new Set(["Schnipsle"]);

const page = readFileSync(new URL("../ui/index.html", import.meta.url), "utf8");
const problems = [];

const lookup = (key) =>
  key
    .split(".")
    .reduce((at, part) => (typeof at === "object" && at !== null ? at[part] : undefined), en);

let keys = 0;
for (const match of page.matchAll(/\bdata-(text|label|title|placeholder)="([^"]*)"/g)) {
  keys++;
  if (typeof lookup(match[2]) !== "string") {
    problems.push(`data-${match[1]}="${match[2]}" is not a string in the catalog`);
  }
}

for (const match of page.matchAll(/\s(aria-label|title|placeholder)="([^"]*)"/g)) {
  problems.push(
    `${match[1]}="${match[2]}" is text in the page; use data-${match[1] === "aria-label" ? "label" : match[1]}`,
  );
}

const body = page
  .replace(/<!--[\s\S]*?-->/g, "")
  .replace(/<(script|style|title)\b[\s\S]*?<\/\1>/g, "");
for (const match of body.matchAll(/>([^<]+)</g)) {
  const text = match[1].replace(/&#?\w+;/g, "").trim();
  if (/\p{L}/u.test(text) && !ALLOWED.has(text))
    problems.push(`text in the page: ${JSON.stringify(text)}`);
}

if (problems.length > 0) {
  console.error(`ui/index.html and the string catalog disagree:\n- ${problems.join("\n- ")}`);
  process.exit(1);
}
console.log(`Strings OK (${String(keys)} keys in the page)`);
