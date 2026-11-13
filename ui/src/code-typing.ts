// SPDX-License-Identifier: Apache-2.0
//
// Straight quotes and dashes in code. The editor lets the system correct
// prose (smart quotes, dashes), but in code a typographic quote breaks what
// the text means: ABC chord symbols and voice names, ChordPro, YAML front
// matter, shell lines. Where the system turns a typed `"`, `'` or `--` into
// „ “ ” ‚ ‘ ’ or —, this puts back what was typed. A typographic quote typed on
// purpose (e.g. ⌥-key on the Mac) or pasted stays.

import { syntaxTree } from "@codemirror/language";
import { type ChangeSpec, EditorState, type Extension, Transaction } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

/** The straight quote each typographic one stands for. */
const QUOTES: Readonly<Record<string, string>> = {
  "„": '"',
  "“": '"',
  "”": '"',
  "«": '"',
  "»": '"',
  "‚": "'",
  "‘": "'",
  "’": "'",
  "‹": "'",
  "›": "'",
};

const DASHES = new Set(["—", "–"]);

/** Nodes whose text is code: fenced and indented blocks, inline code, front matter. */
const CODE = new Set(["FencedCode", "CodeBlock", "InlineCode", "Frontmatter"]);

/** Whether `pos` is in code. */
export function inCode(state: EditorState, pos: number): boolean {
  for (const side of [-1, 1] as const) {
    for (let node = syntaxTree(state).resolveInner(pos, side); ;) {
      if (CODE.has(node.name)) return true;
      const parent = node.parent;
      if (!parent) break;
      node = parent;
    }
  }
  return false;
}

/**
 * What was typed, if the system replaced it: a quote typed as `"` or `'` (by
 * the key pressed, or by the straight quote it replaces), or `--` made a dash.
 */
export function asTyped(inserted: string, deleted: string, key: string | null): string | null {
  const straight = QUOTES[inserted];
  if (straight !== undefined) return key === straight || deleted === straight ? straight : null;
  if (DASHES.has(inserted) && deleted.endsWith("-")) return `${deleted}-`;
  return null;
}

/** Keeps quotes and dashes in code as they were typed. */
export function straightInCode(): Extension {
  // The key of the last keydown: the system's replacement comes with the key
  // of the straight quote, a typographic quote typed on purpose with its own.
  let lastKey: string | null = null;
  const filter = EditorState.transactionFilter.of((tr: Transaction) => {
    if (!tr.docChanged || !tr.isUserEvent("input") || tr.isUserEvent("input.paste")) return tr;
    if (tr.isUserEvent("input.drop")) return tr;
    const start = tr.startState;
    const changes: ChangeSpec[] = [];
    let fixed = 0;
    tr.changes.iterChanges((fromA, toA, _fromB, _toB, text) => {
      const inserted = text.toString();
      const deleted = start.sliceDoc(fromA, toA);
      const typed = asTyped(inserted, deleted, lastKey);
      if (typed !== null && inCode(start, fromA)) {
        changes.push({ from: fromA, to: toA, insert: typed });
        fixed++;
      } else {
        changes.push({ from: fromA, to: toA, insert: text });
      }
    });
    if (fixed === 0) return tr;
    // The same edit with what was typed, and the edit's selection moved onto it.
    const set = start.changes(changes);
    return {
      changes: set,
      selection: (tr.selection?.map(tr.changes.invertedDesc, 1) ?? start.selection).map(set, 1),
      effects: tr.effects,
      userEvent: tr.annotation(Transaction.userEvent) ?? "input.type",
      scrollIntoView: tr.scrollIntoView,
    };
  });
  const keys = EditorView.domEventHandlers({
    keydown(event) {
      lastKey = event.key.length === 1 ? event.key : null;
      return false;
    },
  });
  return [filter, keys];
}
