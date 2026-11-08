// SPDX-License-Identifier: Apache-2.0
// Song sheets in live preview (format.md §5.7, ADR-0014, ADR-0030, ADR-0031).
//
// A `chordpro` block shows as the sheet the core renders, with each chord
// over its syllable, unless the cursor is in it while the editor has focus;
// then its source shows, like a table's. Transposing, the names shown and
// lyrics only are view settings of the block and never change the note.
// Only "Apply transposition" rewrites the block, after the app has stored
// the note in its history (rule 5).

import { syntaxTree } from "@codemirror/language";
import {
  type EditorState,
  type Extension,
  Facet,
  MapMode,
  type Range,
  StateEffect,
  StateField,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import type { Notation, SongLine, SongSection, SongSheet } from "./types";

/** How the viewer shows a block, besides what the note says. */
export interface SongView {
  /** Semitones added to those of the block. */
  transpose: number;
  lyricsOnly: boolean;
  /** The names to show the chords in; `null` for those they are written in. */
  shown: Notation | null;
}

/** What the editor needs from the app for song sheets. */
export interface SongHost {
  render(source: string, info: string, view: SongView): Promise<SongSheet>;
  /**
   * "Apply transposition": stores the note in its history, then returns the
   * block's content with its chords moved; `null` if that could not be done
   * (the app says why).
   */
  applyTransposition(source: string, semitones: number, notation: Notation): Promise<string | null>;
  /** Tells the user something, such as why nothing was rewritten. */
  report(message: string): void;
}

export const songHost = Facet.define<SongHost, SongHost | null>({
  combine: (hosts) => hosts[0] ?? null,
});

const PLAIN: SongView = { transpose: 0, lyricsOnly: false, shown: null };

const sameView = (a: SongView, b: SongView): boolean =>
  a.transpose === b.transpose && a.lyricsOnly === b.lyricsOnly && a.shown === b.shown;

// --- The blocks and their view settings ---

interface SongBlock {
  /** Start of the opening fence's line. */
  from: number;
  /** End of the closing fence's line (or of the note, if it is not closed). */
  to: number;
  /** The content between the fences. */
  textFrom: number;
  textTo: number;
  info: string;
}

const isSong = (info: string): boolean => /^chordpro(?:\s|$)/i.test(info);

function songBlocks(state: EditorState): SongBlock[] {
  const blocks: SongBlock[] = [];
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name !== "FencedCode") return;
      const infoNode = node.node.getChild("CodeInfo");
      const info = infoNode ? state.sliceDoc(infoNode.from, infoNode.to) : "";
      if (isSong(info)) {
        const firstLine = state.doc.lineAt(node.from);
        const text = node.node.getChild("CodeText");
        const textFrom = text?.from ?? Math.min(firstLine.to + 1, node.to);
        blocks.push({
          from: firstLine.from,
          to: state.doc.lineAt(node.to).to,
          textFrom,
          textTo: text?.to ?? textFrom,
          info,
        });
      }
      return false;
    },
  });
  return blocks;
}

/** A block's view settings, by the start of its opening fence. */
interface BlockView {
  pos: number;
  view: SongView;
}

const setSongView = StateEffect.define<BlockView>({
  map: (value, changes) => ({ pos: changes.mapPos(value.pos, 1), view: value.view }),
});

/** View settings that differ from the plain view; reset when another note opens. */
const songViews = StateField.define<readonly BlockView[]>({
  create: () => [],
  update(views, tr) {
    let next = views;
    if (tr.docChanged) {
      next = next.flatMap((v) => {
        const pos = tr.changes.mapPos(v.pos, 1, MapMode.TrackDel);
        return pos === null ? [] : [{ pos, view: v.view }];
      });
    }
    for (const effect of tr.effects) {
      if (!effect.is(setSongView)) continue;
      const { pos, view } = effect.value;
      next = next.filter((v) => v.pos !== pos);
      if (!sameView(view, PLAIN)) next = [...next, { pos, view }];
    }
    return next;
  },
});

const viewAt = (state: EditorState, pos: number): SongView =>
  state.field(songViews).find((v) => v.pos === pos)?.view ?? PLAIN;

// --- Rendering ---

/** Sheets rendered lately, so a block shows at once when the cursor leaves it. */
const sheets = new Map<string, Promise<SongSheet>>();
const CACHED_SHEETS = 64;

/** Forgets the rendered sheets, e.g. when another vault (another default naming) opens. */
export function forgetSongSheets(): void {
  sheets.clear();
}

function sheetFor(
  host: SongHost,
  source: string,
  info: string,
  view: SongView,
): Promise<SongSheet> {
  const key = JSON.stringify([source, info, view.transpose, view.lyricsOnly, view.shown]);
  let sheet = sheets.get(key);
  if (!sheet) {
    sheet = host.render(source, info, view);
    sheet.catch(() => sheets.delete(key));
    if (sheets.size >= CACHED_SHEETS) {
      const oldest = sheets.keys().next().value;
      if (oldest !== undefined) sheets.delete(oldest);
    }
    sheets.set(key, sheet);
  }
  return sheet;
}

function element<K extends keyof HTMLElementTagNameMap>(
  tag: K,
  className: string,
  text?: string,
): HTMLElementTagNameMap[K] {
  const node = document.createElement(tag);
  node.className = className;
  if (text !== undefined) node.textContent = text;
  return node;
}

const signed = (n: number): string => (n > 0 ? `+${String(n)}` : String(n));

function songLine(line: SongLine): HTMLElement {
  switch (line.type) {
    case "lyrics": {
      const row = element("div", "cm-song-line");
      if (line.segments.every((s) => s.chord === null)) {
        row.textContent = line.segments.map((s) => s.text).join("");
        return row;
      }
      row.classList.add("cm-song-chorded");
      for (const { chord, text } of line.segments) {
        const segment = element("span", "cm-song-seg");
        // A chord over nothing still needs a line of lyrics under it.
        segment.append(
          element("span", "cm-song-chord", chord ?? ""),
          element("span", "cm-song-text", text === "" ? " " : text),
        );
        row.append(segment);
      }
      return row;
    }
    case "chords": {
      const row = element("div", "cm-song-line cm-song-chords");
      for (const chord of line.chords) row.append(element("span", "cm-song-chord", chord));
      return row;
    }
    case "comment":
      return element(
        "div",
        line.italic ? "cm-song-comment cm-song-italic" : "cm-song-comment",
        line.text,
      );
    case "tab":
      return element("div", "cm-song-tab", line.text);
    case "unknown":
      return element("div", "cm-song-unknown", line.text);
    case "empty":
      return element("div", "cm-song-gap");
  }
}

const SECTION_NAMES: Record<SongSection["kind"], string | null> = {
  plain: null,
  verse: null,
  chorus: "Chorus",
  bridge: "Bridge",
  tab: null,
};

function songSection(section: SongSection): HTMLElement {
  const block = element("div", `cm-song-section cm-song-${section.kind}`);
  const label = section.label ?? SECTION_NAMES[section.kind];
  if (label !== null) block.append(element("div", "cm-song-label", label));
  block.append(...section.lines.map(songLine));
  return block;
}

function songDetails(meta: SongSheet["meta"]): HTMLElement[] {
  const value = (name: string): string | undefined => meta.find((m) => m.name === name)?.value;
  const out: HTMLElement[] = [];
  const title = value("title");
  if (title !== undefined) out.push(element("div", "cm-song-title", title));
  const subtitle = value("subtitle");
  if (subtitle !== undefined) out.push(element("div", "cm-song-subtitle", subtitle));
  const line = (parts: (string | undefined)[]): void => {
    const shown = parts.filter((p): p is string => p !== undefined && p.trim() !== "");
    if (shown.length > 0) out.push(element("div", "cm-song-details", shown.join(" · ")));
  };
  const labelled = (label: string, name: string): string | undefined => {
    const v = value(name);
    return v === undefined ? undefined : `${label} ${v}`;
  };
  line([value("artist"), labelled("Music:", "composer"), labelled("Words:", "lyricist")]);
  line([
    labelled("Key", "key"),
    labelled("Capo", "capo"),
    labelled("Tempo", "tempo"),
    labelled("Time", "time"),
  ]);
  return out;
}

/** The widget each sheet element shows, so a late render does not overwrite a newer one. */
const shownIn = new WeakMap<HTMLElement, SongWidget>();

/** A `chordpro` block shown as its song sheet, with the view controls above it. */
class SongWidget extends WidgetType {
  constructor(
    private readonly source: string,
    private readonly info: string,
    private readonly view: SongView,
    private readonly host: SongHost,
  ) {
    super();
  }

  override eq(other: SongWidget): boolean {
    return (
      other.source === this.source && other.info === this.info && sameView(other.view, this.view)
    );
  }

  override toDOM(editor: EditorView): HTMLElement {
    const wrap = element("div", "cm-song");
    this.fill(wrap, editor);
    return wrap;
  }

  /** Another view of the same block: the sheet changes in place, so nothing jumps. */
  override updateDOM(dom: HTMLElement, editor: EditorView): boolean {
    if (!dom.classList.contains("cm-song")) return false;
    this.fill(dom, editor);
    return true;
  }

  /**
   * Puts the sheet into `wrap` once it is rendered, in place of what it
   * shows until then. The control that had focus keeps it.
   */
  private fill(wrap: HTMLElement, editor: EditorView): void {
    shownIn.set(wrap, this);
    const show = (...children: HTMLElement[]): void => {
      if (shownIn.get(wrap) !== this) return; // a newer view came first
      const active = document.activeElement;
      const control =
        active instanceof HTMLElement && wrap.contains(active) ? active.dataset["control"] : null;
      wrap.replaceChildren(...children);
      if (control) wrap.querySelector<HTMLElement>(`[data-control="${control}"]`)?.focus();
      editor.requestMeasure();
    };
    sheetFor(this.host, this.source, this.info, this.view).then(
      (sheet) => {
        const bar = element("div", "cm-song-bar");
        bar.setAttribute("role", "toolbar");
        bar.setAttribute("aria-label", "Song sheet view");
        this.fillBar(bar, sheet, editor, wrap);
        const body = element("div", "cm-song-body");
        body.append(...songDetails(sheet.meta), ...sheet.sections.map(songSection));
        show(bar, body);
      },
      (e: unknown) => {
        const message =
          e instanceof Error ? e.message : String((e as { message?: unknown }).message);
        show(
          element("div", "cm-song-unknown", `Song sheet not shown: ${message}`),
          element("pre", "cm-song-tab", this.source),
        );
      },
    );
  }

  private fillBar(bar: HTMLElement, sheet: SongSheet, editor: EditorView, wrap: HTMLElement): void {
    const set = (change: Partial<SongView>): void => {
      editor.dispatch({
        effects: setSongView.of({ pos: editor.posAtDOM(wrap), view: { ...this.view, ...change } }),
      });
    };
    const button = (
      control: string,
      text: string,
      label: string,
      onClick: () => void,
    ): HTMLButtonElement => {
      const b = element("button", "cm-song-button", text);
      b.dataset["control"] = control;
      b.type = "button";
      b.setAttribute("aria-label", label);
      b.title = label;
      b.addEventListener("click", onClick);
      return b;
    };

    const amount = element("span", "cm-song-amount", signed(this.view.transpose));
    amount.title = "Semitones the viewer adds";
    bar.append(
      button("down", "−", "Transpose down a semitone", () => {
        set({ transpose: this.view.transpose - 1 });
      }),
      amount,
      button("up", "+", "Transpose up a semitone", () => {
        set({ transpose: this.view.transpose + 1 });
      }),
    );

    const names = element("select", "cm-song-select");
    names.dataset["control"] = "names";
    names.setAttribute("aria-label", "Chord names");
    names.title = `Chord names (written in ${sheet.notation === "german" ? "German" : "English"})`;
    for (const [value, text] of [
      ["english", "B, B♭ (English)"],
      ["german", "H, B (German)"],
    ] as const) {
      const option = element("option", "", text);
      option.value = value;
      option.selected = sheet.shown === value;
      names.append(option);
    }
    names.addEventListener("change", () => {
      const shown = names.value === "german" ? "german" : "english";
      set({ shown: shown === sheet.notation ? null : shown });
    });
    bar.append(names);

    const lyrics = element("label", "cm-song-toggle");
    const box = element("input", "");
    box.type = "checkbox";
    box.dataset["control"] = "lyrics";
    box.checked = sheet.lyricsOnly;
    // `nochords` in the info string: the block itself asks for lyrics only.
    box.disabled = sheet.lyricsOnly && !this.view.lyricsOnly;
    box.addEventListener("change", () => {
      set({ lyricsOnly: box.checked });
    });
    lyrics.append(box, " Lyrics only");
    bar.append(lyrics);

    if (this.view.transpose % 12 !== 0 && !editor.state.readOnly) {
      bar.append(
        button(
          "apply",
          "Apply transposition",
          "Rewrite the chords in the note; the version before is kept in the history",
          () => void applyTransposition(editor, editor.posAtDOM(wrap), sheet.notation),
        ),
      );
    }
  }

  override ignoreEvent(event: Event): boolean {
    // The controls handle their own events; a click elsewhere puts the cursor
    // into the block, which shows its source.
    return event.target instanceof Element && event.target.closest(".cm-song-bar") !== null;
  }
}

/** "Apply transposition" for the block starting at `pos` (rule 5). */
async function applyTransposition(
  editor: EditorView,
  pos: number,
  notation: Notation,
): Promise<void> {
  const host = editor.state.facet(songHost);
  const block = songBlocks(editor.state).find((b) => b.from === pos);
  if (!host || !block || editor.state.readOnly) return;
  const { transpose } = viewAt(editor.state, pos);
  const source = editor.state.sliceDoc(block.textFrom, block.textTo);
  const result = await host.applyTransposition(source, transpose, notation);
  if (result === null) return;
  // The note may have changed, or turned read-only, while the history stored it.
  const state: EditorState = editor.state;
  const now = songBlocks(state).find(
    (b) => b.from === pos && state.sliceDoc(b.textFrom, b.textTo) === source,
  );
  if (!now || state.readOnly) {
    host.report("The song changed meanwhile, so its chords were not rewritten.");
    return;
  }
  editor.dispatch({
    changes: { from: now.textFrom, to: now.textTo, insert: result },
    effects: setSongView.of({ pos, view: { ...viewAt(editor.state, pos), transpose: 0 } }),
    userEvent: "input.transpose",
  });
}

function songDecorations(state: EditorState, hasFocus: boolean): DecorationSet {
  const host = state.facet(songHost);
  if (!host) return Decoration.none;
  const found: Range<Decoration>[] = [];
  for (const block of songBlocks(state)) {
    const edited =
      hasFocus && state.selection.ranges.some((r) => r.from <= block.to && r.to >= block.from);
    if (edited) continue;
    const widget = new SongWidget(
      state.sliceDoc(block.textFrom, block.textTo),
      block.info,
      viewAt(state, block.from),
      host,
    );
    found.push(Decoration.replace({ widget, block: true }).range(block.from, block.to));
  }
  return Decoration.set(found);
}

/** Song sheets in live preview; `focused` says whether the editor has focus. */
export function songPreview(focused: StateField<boolean>): Extension {
  const hasFocus = (state: EditorState): boolean => state.field(focused, false) ?? false;
  const preview = StateField.define<DecorationSet>({
    create: (state) => songDecorations(state, hasFocus(state)),
    update(decorations, tr) {
      const changed =
        tr.docChanged ||
        tr.selection !== undefined ||
        tr.effects.length > 0 ||
        hasFocus(tr.startState) !== hasFocus(tr.state) ||
        syntaxTree(tr.startState) !== syntaxTree(tr.state); // the parser caught up
      return changed ? songDecorations(tr.state, hasFocus(tr.state)) : decorations;
    },
    provide: (field) => EditorView.decorations.from(field),
  });
  return [songViews, preview, songTheme];
}

const MONO = 'ui-monospace, "SF Mono", Menlo, monospace';

const songTheme = EditorView.theme({
  ".cm-song": {
    margin: "6px 0",
    padding: "8px 12px 10px",
    border: "1px solid var(--line)",
    borderRadius: "6px",
  },
  ".cm-song-bar": {
    display: "flex",
    flexWrap: "wrap",
    alignItems: "center",
    gap: "6px 8px",
    marginBottom: "8px",
    fontSize: "0.85em",
    color: "var(--muted)",
  },
  ".cm-song-button": {
    font: "inherit",
    padding: "1px 8px",
    border: "1px solid var(--line)",
    borderRadius: "4px",
    background: "var(--panel)",
    color: "var(--fg)",
    cursor: "pointer",
  },
  ".cm-song-amount": { minWidth: "2.2em", textAlign: "center", fontVariantNumeric: "tabular-nums" },
  ".cm-song-select": { font: "inherit" },
  ".cm-song-toggle": { display: "inline-flex", alignItems: "center", gap: "4px" },
  ".cm-song-title": { fontSize: "1.2em", fontWeight: "700" },
  ".cm-song-subtitle": { fontStyle: "italic" },
  ".cm-song-details": { color: "var(--muted)", fontSize: "0.9em" },
  ".cm-song-section": { marginTop: "10px" },
  ".cm-song-chorus": { paddingLeft: "12px", borderLeft: "3px solid var(--line)" },
  ".cm-song-label": { fontWeight: "600", fontSize: "0.9em", color: "var(--muted)" },
  ".cm-song-line": { whiteSpace: "pre-wrap" },
  ".cm-song-chorded": { lineHeight: "1.25", margin: "2px 0" },
  ".cm-song-seg": {
    display: "inline-flex",
    flexDirection: "column",
    verticalAlign: "bottom",
  },
  ".cm-song-chord": {
    color: "var(--accent)",
    fontWeight: "600",
    fontSize: "0.9em",
    whiteSpace: "pre",
    paddingRight: "0.4em",
    minHeight: "1.25em",
  },
  ".cm-song-text": { whiteSpace: "pre-wrap" },
  ".cm-song-chords": { display: "flex", flexWrap: "wrap", gap: "0 0.8em" },
  ".cm-song-comment": { color: "var(--muted)", fontSize: "0.9em", margin: "4px 0" },
  ".cm-song-italic": { fontStyle: "italic" },
  ".cm-song-tab": {
    fontFamily: MONO,
    fontSize: "0.85em",
    whiteSpace: "pre",
    overflowX: "auto",
    margin: "0",
  },
  ".cm-song-unknown": {
    fontFamily: MONO,
    fontSize: "0.85em",
    color: "var(--muted)",
    opacity: "0.75",
  },
  ".cm-song-gap": { height: "0.6em" },
});
