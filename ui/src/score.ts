// SPDX-License-Identifier: Apache-2.0
// Scores in live preview (format.md §5.6, ADR-0013, ADR-0032).
//
// An `abc` block shows as its engraved score unless the cursor is in it
// while the editor has focus; then its source shows, like a table's. abcjs
// is loaded the first time a note has music. A block abcjs warns about
// stays source, with the warnings under it (rule 3): nothing is engraved
// that would leave out part of what the source says.
//
// A bar over each score plays it with the piano sounds the app brings along,
// slower or faster, transposed, with voices muted or alone (F-17, ADR-0035).
// These are view settings of the block, like a song sheet's; the note never
// changes.

import { syntaxTree } from "@codemirror/language";
import {
  type EditorState,
  type Extension,
  MapMode,
  type Range,
  StateEffect,
  StateField,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  WidgetType,
} from "@codemirror/view";

type Abcjs = typeof import("abcjs");
type TuneObject = import("abcjs").TuneObject;
type MidiBuffer = import("abcjs").MidiBuffer;

// --- Loading abcjs ---

let abcjs: Abcjs | null = null;
let loadError: string | null = null;
let loading: Promise<void> | null = null;

/** abcjs is loaded (or failed to load): the blocks are drawn again. */
const musicLoaded = StateEffect.define();

function loadAbcjs(): Promise<void> {
  loading ??= import("abcjs").then(
    (module: Abcjs) => {
      // A CommonJS module: its exports may come as the default export.
      const withDefault = module as Abcjs & { default?: Abcjs };
      abcjs = withDefault.default ?? module;
    },
    (e: unknown) => {
      loadError = e instanceof Error ? e.message : String(e);
      console.error("abcjs could not be loaded", e);
    },
  );
  return loading;
}

// --- The blocks ---

interface ScoreBlock {
  /** Start of the opening fence's line. */
  from: number;
  /** End of the closing fence's line (or of the note, if it is not closed). */
  to: number;
  /** The content between the fences. */
  textFrom: number;
  textTo: number;
  /** `transpose=` of the info string (rule 6); 0 without one. */
  transpose: number;
  nosource: boolean;
}

/** The options of an `abc` info string, or `null` for another block. */
function scoreOptions(info: string): { transpose: number; nosource: boolean } | null {
  const [lang, ...options] = info.trim().split(/\s+/);
  if (lang?.toLowerCase() !== "abc") return null;
  let transpose = 0;
  let nosource = false;
  for (const option of options) {
    const semitones = /^transpose=([+-]?\d{1,3})$/i.exec(option)?.[1];
    if (semitones !== undefined) transpose = Number(semitones);
    else if (option.toLowerCase() === "nosource") nosource = true;
    // Unknown options are ignored (rule 6).
  }
  return { transpose, nosource };
}

/** The `abc` block around `pos` (its fences included), or `null`. */
export function scoreBlockAt(
  state: EditorState,
  pos: number,
): { from: number; to: number; textFrom: number; textTo: number } | null {
  return scoreBlocks(state).find((b) => b.from <= pos && pos <= b.to) ?? null;
}

function scoreBlocks(state: EditorState): ScoreBlock[] {
  const blocks: ScoreBlock[] = [];
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name !== "FencedCode") return;
      const infoNode = node.node.getChild("CodeInfo");
      const options = infoNode ? scoreOptions(state.sliceDoc(infoNode.from, infoNode.to)) : null;
      if (options) {
        const firstLine = state.doc.lineAt(node.from);
        const text = node.node.getChild("CodeText");
        const textFrom = text?.from ?? Math.min(firstLine.to + 1, node.to);
        blocks.push({
          from: firstLine.from,
          to: state.doc.lineAt(node.to).to,
          textFrom,
          textTo: text?.to ?? textFrom,
          ...options,
        });
      }
      return false;
    },
  });
  return blocks;
}

// --- View settings of a block ---

/** How the viewer plays and shows a block, besides what the note says. */
interface ScoreView {
  /** Semitones added to the block's `transpose=`. */
  transpose: number;
  /** Speed in percent of the tune's own tempo. */
  tempo: number;
  /** Voices by their number in the tune, from 0. */
  muted: readonly number[];
  solo: readonly number[];
  /** The accompaniment abcjs plays to the chord symbols. */
  chordsOff: boolean;
}

const PLAIN_VIEW: ScoreView = { transpose: 0, tempo: 100, muted: [], solo: [], chordsOff: false };
const TEMPO_STEP = 10;
const TEMPO_MIN = 30;
const TEMPO_MAX = 200;

const sameView = (a: ScoreView, b: ScoreView): boolean => JSON.stringify(a) === JSON.stringify(b);

/** A block's view settings, by the start of its opening fence. */
interface BlockView {
  pos: number;
  view: ScoreView;
}

const setScoreView = StateEffect.define<BlockView>({
  map: (value, changes) => ({ pos: changes.mapPos(value.pos, 1), view: value.view }),
});

/** View settings that differ from the plain view; reset when another note opens. */
const scoreViews = StateField.define<readonly BlockView[]>({
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
      if (!effect.is(setScoreView)) continue;
      const { pos, view } = effect.value;
      next = next.filter((v) => v.pos !== pos);
      if (!sameView(view, PLAIN_VIEW)) next = [...next, { pos, view }];
    }
    return next;
  },
});

const viewAt = (state: EditorState, pos: number): ScoreView =>
  state.field(scoreViews, false)?.find((v) => v.pos === pos)?.view ?? PLAIN_VIEW;

// --- Parsing ---

interface Warning {
  /** 1-based, in the block's content. */
  line: number;
  column: number;
  message: string;
}

interface Parsed {
  /** What abcjs reads (see `forAbcjs`). */
  abc: string;
  tunes: number;
  warnings: Warning[];
}

/** Blocks parsed lately, by source and transposition. */
const parsed = new Map<string, Parsed>();
const CACHED_SCORES = 64;

/** abcjs warnings are HTML; only their text is shown, never their markup. */
function plainText(html: string): string {
  return new DOMParser().parseFromString(html, "text/html").body.textContent;
}

/** "Music Line:3:7: Unknown character ignored:  C D <span>$</span> F" */
function readWarning(html: string): Warning {
  const text = plainText(html);
  const match = /^[^:]*Line:(\d+):(\d+):\s*(.*)$/s.exec(text);
  if (!match) return { line: 1, column: 1, message: text.trim() };
  // The quoted source line after two spaces repeats what the cursor shows.
  const message = (match[3] ?? "").split(/:\s{2}/)[0]?.trim() ?? "";
  return { line: Number(match[1]), column: Number(match[2]), message };
}

/** A piece of the source replaced by text of the same length. */
interface Swap {
  from: number;
  to: number;
  text: string;
}

/** Fills the width a code took; shows nothing and breaks nothing. */
const WORD_JOINER = "\u2060";

/** HTML entities, escapes and what they stand for, by code. */
const decoded = new Map<string, string | null>();

/**
 * The character(s) `code` stands for: an HTML entity (`&uuml;`, `&#252;`,
 * `&#xFC;`) or a Unicode escape (`\u00fc`, `\U0001F3B5`); `null` if none, or
 * if it is one that would change how abcjs reads the line (`"`, `%`, `\`).
 */
function decode(code: string): string | null {
  let text = decoded.get(code);
  if (text !== undefined) return text;
  text = null;
  const hex = /^\\u([0-9a-f]{4})$|^\\U([0-9a-f]{8})$|^&#x([0-9a-f]{1,6});$/i.exec(code);
  const digits = hex?.[1] ?? hex?.[2] ?? hex?.[3];
  const decimal = /^&#(\d{1,7});$/.exec(code)?.[1];
  const point = digits !== undefined ? parseInt(digits, 16) : Number(decimal ?? NaN);
  if (!Number.isNaN(point)) {
    if (point > 0 && point <= 0x10ffff) text = String.fromCodePoint(point);
  } else {
    // The browser knows every named entity; the parsed document is inert.
    const parsed = new DOMParser().parseFromString(code, "text/html").body.textContent;
    if (parsed !== code) text = parsed;
  }
  if (text !== null && (text.length > code.length || /["%\\]/.test(text))) text = null;
  decoded.set(code, text);
  return text;
}

/**
 * Entities and Unicode escapes. Escaped characters (`\&`, `\\`) match too,
 * so an escaped `&` or backslash starts no code (Safari before 16.4 has no
 * lookbehind).
 */
const CODES =
  /\\[^uU]|&(?:#[xX][0-9a-fA-F]{1,6}|#\d{1,7}|[A-Za-z][A-Za-z0-9]{1,31});|\\u[0-9a-fA-F]{4}|\\U[0-9a-fA-F]{8}/g;

/** Swaps for the codes in `source` from `from` to `to`. */
function decodeIn(source: string, from: number, to: number, swaps: Swap[]): void {
  for (const match of source.slice(from, to).matchAll(CODES)) {
    // `\&` is a plain `&` in ABC; abcjs would show the backslash.
    const text = match[0] === "\\&" ? "&" : match[0].length > 2 ? decode(match[0]) : null;
    if (text === null) continue;
    const start = from + match.index;
    const fill = WORD_JOINER.repeat(match[0].length - text.length);
    swaps.push({ from: start, to: start + match[0].length, text: text + fill });
  }
}

/** Where a `%` comment starts in `line`, or its length if it has none. */
function commentAt(line: string): number {
  for (const match of line.matchAll(/\\.|%/g)) if (match[0] === "%") return match.index;
  return line.length;
}

/**
 * The block as abcjs is to read it, every character in its place, so clicks
 * and warnings still point into the source:
 *
 * - ABC 2.1 continues a field on the next line with `+:`, which abcjs does
 *   not read. Each continuation is joined to its field: the line end, the
 *   `+:` and the field's `%` comment turn into spaces.
 * - abcjs reads only the backslash forms of special letters (`\"u`), not
 *   HTML entities (`&uuml;`), Unicode escapes (`\u00fc`) or `\&`. In
 *   fields and in the quoted chords and annotations of music lines they are
 *   decoded, and word joiners fill the rest of their width.
 */
function forAbcjs(source: string): string {
  if (
    !source.includes("+:") &&
    !source.includes("&") &&
    !source.includes("\\u") &&
    !source.includes("\\U")
  ) {
    return source;
  }
  const swaps: Swap[] = [];
  let start = 0;
  let fieldStart = -1; // start of the field the line before belongs to, or -1
  for (const line of source.split("\n")) {
    const end = start + commentAt(line);
    if (line.startsWith("+:") && fieldStart >= 0) {
      // The comment of the line before ends the text the field has so far.
      const comment = fieldStart + commentAt(source.slice(fieldStart, start - 1));
      if (comment < start - 1) swaps.push({ from: comment, to: start - 1, text: "" });
      swaps.push({ from: start - 1, to: start + 2, text: "" }); // "\n+:"
      decodeIn(source, start + 2, end, swaps);
      fieldStart = start;
    } else if (/^[A-Za-z]:/.test(line)) {
      decodeIn(source, start + 2, end, swaps);
      fieldStart = start;
    } else {
      if (!line.startsWith("%")) {
        for (const quoted of line.slice(0, end - start).matchAll(/"[^"]*"/g)) {
          decodeIn(source, start + quoted.index, start + quoted.index + quoted[0].length, swaps);
        }
      }
      fieldStart = -1;
    }
    start += line.length + 1;
  }
  let out = "";
  let at = 0;
  for (const swap of swaps) {
    // In source order and apart; blanks are spaces of the same width.
    out += source.slice(at, swap.from) + swap.text.padEnd(swap.to - swap.from, " ");
    at = swap.to;
  }
  return out + source.slice(at);
}

/** Line and column (1-based) of a warning in `from`, at the same offset in `to`. */
function moveWarning(warning: Warning, from: string, to: string): Warning {
  if (from === to) return warning;
  let offset = 0;
  for (let line = 1; line < warning.line; line++) {
    const end = from.indexOf("\n", offset);
    if (end < 0) break;
    offset = end + 1;
  }
  offset += warning.column - 1;
  const before = to.slice(0, offset);
  const lineStart = before.lastIndexOf("\n") + 1;
  return {
    ...warning,
    line: before.split("\n").length,
    column: offset - lineStart + 1,
  };
}

function parse(lib: Abcjs, source: string, transpose: number): Parsed {
  const key = JSON.stringify([source, transpose]);
  let result = parsed.get(key);
  if (!result) {
    const abc = forAbcjs(source);
    const tunes = lib.parseOnly(abc, { visualTranspose: transpose });
    result = {
      abc,
      tunes: tunes.length,
      warnings: tunes.flatMap((t) =>
        (t.warnings ?? []).map((w) => moveWarning(readWarning(w), abc, source)),
      ),
    };
    if (parsed.size >= CACHED_SCORES) {
      const oldest = parsed.keys().next().value;
      if (oldest !== undefined) parsed.delete(oldest);
    }
    parsed.set(key, result);
  }
  return result;
}

// --- Widgets ---

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

/** Heights of scores drawn lately, so a score scrolled back into view does not jump. */
const heights = new Map<string, number>();

/** Puts the cursor at `pos` and focuses the editor, which shows the block's source. */
function editAt(editor: EditorView, pos: number): void {
  editor.dispatch({ selection: { anchor: pos }, scrollIntoView: true });
  editor.focus();
}

/** The voices of a tune and whether it has an accompaniment to its chord symbols. */
interface Voices {
  names: string[];
  chords: boolean;
}

/** The tunes a score element shows, and their voices, for the bar and for playing. */
const shownTunes = new WeakMap<HTMLElement, { tunes: TuneObject[]; voices: Voices[] }>();

function voicesOf(tune: TuneObject): Voices {
  const count = Math.max(
    1,
    ...tune.lines.map((line) =>
      (line.staff ?? []).reduce((n, s) => n + (s.voices?.length ?? 0), 0),
    ),
  );
  const tracks = tune.setUpAudio({}).tracks;
  const names = tracks.slice(0, count).map((track, index) => {
    const name = track.find((event) => event.cmd === "text");
    const text = name?.cmd === "text" ? name.text.replaceAll(WORD_JOINER, "").trim() : "";
    return text !== "" ? text : `Voice ${String(index + 1)}`;
  });
  return { names, chords: tracks.length > count };
}

/** An `abc` block shown as its engraved score, with the bar that plays it. */
class ScoreWidget extends WidgetType {
  constructor(
    private readonly lib: Abcjs,
    private readonly source: string,
    private readonly transpose: number,
    private readonly tunes: number,
    private readonly view: ScoreView,
    /** The tune of this block that plays, and whether it is still loading. */
    private readonly playing: { tune: number; loading: boolean } | null,
  ) {
    super();
  }

  private get key(): string {
    return JSON.stringify([this.source, this.transpose]);
  }

  override eq(other: ScoreWidget): boolean {
    return (
      other.key === this.key &&
      sameView(other.view, this.view) &&
      JSON.stringify(other.playing) === JSON.stringify(this.playing)
    );
  }

  override get estimatedHeight(): number {
    return heights.get(this.key) ?? -1;
  }

  override toDOM(editor: EditorView): HTMLElement {
    const wrap = element("div", "cm-score");
    wrap.dataset["key"] = this.key;
    const bar = element("div", "cm-score-bar");
    bar.setAttribute("role", "toolbar");
    bar.setAttribute("aria-label", "Score playback");
    const music = element("div", "cm-score-music");
    music.setAttribute("role", "img");
    music.setAttribute("aria-label", "Music score; click to edit its ABC source");
    const targets = Array.from({ length: Math.max(this.tunes, 1) }, () =>
      element("div", "cm-score-tune"),
    );
    music.append(...targets);
    wrap.append(bar, music);
    // Set by abcjs while a click is handled, before the click event itself.
    let picked: number | null = null;
    // abcjs takes one element per tune, though its types name only one.
    const tunes = this.lib.renderAbc(targets as unknown as Element, this.source, {
      responsive: "resize",
      visualTranspose: this.transpose,
      add_classes: true,
      // A clicked note is not marked, as the block turns into source at once.
      selectionColor: "currentColor",
      clickListener: (abcelem) => {
        picked = abcelem.startChar ?? null;
      },
    });
    shownTunes.set(wrap, { tunes, voices: tunes.map(voicesOf) });
    this.fillBar(bar, editor, wrap);
    // Without this the browser moves the cursor to the block's start, which
    // shows the source before the click says which note it was.
    wrap.addEventListener("mousedown", (event) => {
      event.preventDefault();
    });
    music.addEventListener("click", () => {
      const offset = picked !== null && picked >= 0 ? picked : 0;
      picked = null;
      const block = scoreBlocks(editor.state).find((b) => b.from === editor.posAtDOM(wrap));
      if (block) editAt(editor, Math.min(block.textFrom + offset, block.textTo));
    });
    requestAnimationFrame(() => {
      if (wrap.isConnected) heights.set(this.key, wrap.getBoundingClientRect().height);
    });
    return wrap;
  }

  /** Other view settings of the same score: only the bar changes, nothing is engraved again. */
  override updateDOM(dom: HTMLElement, editor: EditorView): boolean {
    const bar = dom.querySelector<HTMLElement>(":scope > .cm-score-bar");
    if (dom.dataset["key"] !== this.key || !bar) return false;
    const active = document.activeElement;
    const control =
      active instanceof HTMLElement && bar.contains(active) ? active.dataset["control"] : null;
    bar.replaceChildren();
    this.fillBar(bar, editor, dom);
    if (control) bar.querySelector<HTMLElement>(`[data-control="${control}"]`)?.focus();
    return true;
  }

  private fillBar(bar: HTMLElement, editor: EditorView, wrap: HTMLElement): void {
    const shown = shownTunes.get(wrap);
    if (!shown) return;
    const view = this.view;
    const set = (change: Partial<ScoreView>): void => {
      const pos = editor.posAtDOM(wrap);
      const next = { ...view, ...change };
      editor.dispatch({ effects: setScoreView.of({ pos, view: next }) });
      if (change.transpose !== undefined) stopPlayback();
      else replay(editor, pos, next);
    };
    const button = (
      control: string,
      text: string,
      label: string,
      onClick: () => void,
      pressed?: boolean,
    ): HTMLButtonElement => {
      const b = element("button", "cm-score-button", text);
      b.dataset["control"] = control;
      b.type = "button";
      b.setAttribute("aria-label", label);
      b.title = label;
      if (pressed !== undefined) b.setAttribute("aria-pressed", String(pressed));
      b.addEventListener("click", onClick);
      return b;
    };
    const group = (label: string, ...children: HTMLElement[]): HTMLElement => {
      const g = element("span", "cm-score-group");
      g.setAttribute("role", "group");
      g.setAttribute("aria-label", label);
      g.append(...children);
      return g;
    };

    const several = shown.tunes.length > 1;
    shown.tunes.forEach((tune, index) => {
      // Without the word joiners of the preparation, which a screen reader might spell out.
      const title =
        tune.metaText.title?.replaceAll(WORD_JOINER, "").trim() || `Tune ${String(index + 1)}`;
      const playingThis = this.playing?.tune === index;
      const text = playingThis
        ? this.playing.loading
          ? "Loading…"
          : "■ Stop"
        : several
          ? `▶ ${title}`
          : "▶ Play";
      bar.append(
        button(
          `play-${String(index)}`,
          text,
          playingThis ? `Stop ${title}` : `Play ${title}`,
          () => {
            if (playingThis) {
              stopPlayback();
            } else {
              void play(
                this.lib,
                editor,
                editor.posAtDOM(wrap),
                index,
                tune,
                shown.voices[index],
                this.transpose,
                view,
              );
            }
          },
        ),
      );
    });

    const tempo = element("span", "cm-score-amount", `${String(view.tempo)} %`);
    tempo.title = "Speed in percent of the tune's tempo";
    bar.append(
      group(
        "Tempo",
        element("span", "cm-score-label", "Tempo"),
        button("slower", "−", "Slower", () => {
          set({ tempo: Math.max(TEMPO_MIN, view.tempo - TEMPO_STEP) });
        }),
        tempo,
        button("faster", "+", "Faster", () => {
          set({ tempo: Math.min(TEMPO_MAX, view.tempo + TEMPO_STEP) });
        }),
      ),
    );

    const semitones = element("span", "cm-score-amount", signed(view.transpose));
    semitones.title = "Semitones the viewer adds";
    bar.append(
      group(
        "Transpose",
        element("span", "cm-score-label", "Transpose"),
        button("down", "−", "Transpose down a semitone", () => {
          set({ transpose: view.transpose - 1 });
        }),
        semitones,
        button("up", "+", "Transpose up a semitone", () => {
          set({ transpose: view.transpose + 1 });
        }),
      ),
    );

    // The voices of the tune with the most of them; numbers are the same in every tune.
    const voices = shown.voices.reduce<Voices>(
      (most, v) => ({
        names: v.names.length > most.names.length ? v.names : most.names,
        chords: most.chords || v.chords,
      }),
      { names: [], chords: false },
    );
    if (voices.names.length > 1) {
      voices.names.forEach((name, index) => {
        const muted = view.muted.includes(index);
        const solo = view.solo.includes(index);
        const toggle = (list: readonly number[]): number[] =>
          list.includes(index) ? list.filter((i) => i !== index) : [...list, index];
        bar.append(
          group(
            name,
            element("span", "cm-score-label", name),
            button(
              `mute-${String(index)}`,
              "M",
              `Mute ${name}`,
              () => {
                set({ muted: toggle(view.muted) });
              },
              muted,
            ),
            button(
              `solo-${String(index)}`,
              "S",
              `${name} alone`,
              () => {
                set({ solo: toggle(view.solo) });
              },
              solo,
            ),
          ),
        );
      });
    }
    if (voices.chords) {
      bar.append(
        button(
          "chords",
          "Chords",
          "Play an accompaniment to the chord symbols",
          () => {
            set({ chordsOff: !view.chordsOff });
          },
          !view.chordsOff,
        ),
      );
    }
  }

  override ignoreEvent(): boolean {
    return true; // the bar handles its own clicks, and a click on the music puts the cursor there
  }
}

const signed = (n: number): string => (n > 0 ? `+${String(n)}` : String(n));

/** Why a block shows as source: its warnings, or abcjs not loading. */
class WarningsWidget extends WidgetType {
  constructor(
    private readonly warnings: readonly Warning[],
    private readonly loadFailure: string | null,
  ) {
    super();
  }

  override eq(other: WarningsWidget): boolean {
    return (
      other.loadFailure === this.loadFailure &&
      JSON.stringify(other.warnings) === JSON.stringify(this.warnings)
    );
  }

  override toDOM(editor: EditorView): HTMLElement {
    const wrap = element("div", "cm-score-warnings");
    wrap.setAttribute("role", "status");
    if (this.loadFailure !== null) {
      wrap.append(
        element("div", "cm-score-warning-title", `Score not engraved: ${this.loadFailure}`),
      );
      return wrap;
    }
    const n = this.warnings.length;
    wrap.append(
      element(
        "div",
        "cm-score-warning-title",
        `Score not engraved: ${String(n)} ${n === 1 ? "problem" : "problems"} in the ABC`,
      ),
    );
    const list = element("ul", "cm-score-warning-list");
    for (const warning of this.warnings) {
      const item = element("li", "");
      const link = element(
        "button",
        "cm-score-warning",
        `Line ${String(warning.line)}, column ${String(warning.column)}: ${warning.message}`,
      );
      link.type = "button";
      link.addEventListener("click", () => {
        const block = scoreBlocks(editor.state).find((b) => b.to === editor.posAtDOM(wrap));
        if (!block) return;
        const { doc } = editor.state;
        const first = doc.lineAt(block.textFrom).number;
        const lineNumber = Math.min(first + warning.line - 1, doc.lines);
        const line = doc.line(lineNumber);
        editAt(editor, Math.min(line.from + warning.column - 1, line.to, block.textTo));
      });
      item.append(link);
      list.append(item);
    }
    wrap.append(list);
    return wrap;
  }

  override ignoreEvent(): boolean {
    return true;
  }
}

// --- Decorations ---

function scoreDecorations(state: EditorState, hasFocus: boolean): DecorationSet {
  const found: Range<Decoration>[] = [];
  for (const block of scoreBlocks(state)) {
    if (loadError !== null) {
      const widget = new WarningsWidget([], loadError);
      found.push(Decoration.widget({ widget, block: true, side: 1 }).range(block.to));
      continue;
    }
    if (!abcjs) continue; // loading; the source shows meanwhile
    const source = state.sliceDoc(block.textFrom, block.textTo);
    const edited =
      hasFocus && state.selection.ranges.some((r) => r.from <= block.to && r.to >= block.from);
    // `nosource` keeps a read-only view engraved, even with the cursor in it.
    if (edited && !(block.nosource && state.readOnly)) continue;
    const view = viewAt(state, block.from);
    const transpose = block.transpose + view.transpose;
    const { abc, tunes, warnings } = parse(abcjs, source, transpose);
    if (warnings.length > 0) {
      const widget = new WarningsWidget(warnings, null);
      found.push(Decoration.widget({ widget, block: true, side: 1 }).range(block.to));
      continue;
    }
    const plays =
      playing?.pos === block.from ? { tune: playing.tune, loading: playing.synth === null } : null;
    const widget = new ScoreWidget(abcjs, abc, transpose, tunes, view, plays);
    found.push(Decoration.replace({ widget, block: true }).range(block.from, block.to));
  }
  return Decoration.set(found);
}

// --- Playback ---

/** Where the piano sounds are, one file per note (`ui/public/sounds/`). */
const SOUNDS = "/sounds/";

/** The voice of a General MIDI drum kit, which the app has no sounds for. */
const PERCUSSION = 128;

/** The tune that plays. One plays at a time, in the whole window. */
interface Playing {
  editor: EditorView;
  /** The block, by the start of its opening fence. */
  pos: number;
  source: string;
  tune: number;
  tuneObject: TuneObject;
  voices: Voices;
  /** Semitones the tune is shown moved by, and so played. */
  transpose: number;
  /** `null` while the sounds load. */
  synth: MidiBuffer | null;
  /** The audio clock at the tune's start, and its length, in seconds. */
  started: number;
  duration: number;
}

let playing: Playing | null = null;
/** Counts starts and stops, so a start that finishes loading late knows it is stale. */
let generation = 0;
let context: AudioContext | null = null;

/** Tells the editor that what plays changed, so the bars show it. */
const playbackChanged = StateEffect.define();

function changed(editor: EditorView): void {
  // Not during an update of the editor, which may be what stopped the tune.
  queueMicrotask(() => {
    if (editor.dom.isConnected) editor.dispatch({ effects: playbackChanged.of(null) });
  });
}

function stopPlayback(): void {
  generation++;
  const was = playing;
  playing = null;
  if (!was) return;
  was.synth?.stop();
  changed(was.editor);
}

/** Where in the tune it plays, from 0 to 1. */
function position(was: Playing): number {
  if (!was.synth || !context || was.duration <= 0) return 0;
  return Math.min(Math.max((context.currentTime - was.started) / was.duration, 0), 1);
}

/**
 * The iPhone keeps Web Audio silent while its ring switch is off, unless the
 * page says it plays music (Audio Session API, iOS 16.4 and later).
 */
function playEvenWhenSilenced(): void {
  const session = (navigator as Navigator & { audioSession?: { type: string } }).audioSession;
  if (session) session.type = "playback";
}

/** The voices that do not play: muted ones, and with any alone, all others. */
function voicesOff(view: ScoreView, voices: Voices): number[] {
  return voices.names
    .map((_, index) => index)
    .filter(
      (index) => view.muted.includes(index) || (view.solo.length > 0 && !view.solo.includes(index)),
    );
}

/** Plays tune `tune` of the block at `pos` from `from` (0 to 1), stopping whatever plays. */
async function play(
  lib: Abcjs,
  editor: EditorView,
  pos: number,
  tune: number,
  tuneObject: TuneObject,
  voices: Voices | undefined,
  transpose: number,
  view: ScoreView,
  from = 0,
): Promise<void> {
  stopPlayback();
  const mine = generation;
  const block = scoreBlocks(editor.state).find((b) => b.from === pos);
  if (!block || !voices) return;
  if (!lib.synth.supportsAudio()) {
    console.error("This web view cannot play audio");
    return;
  }
  playEvenWhenSilenced();
  if (!context) {
    context = new AudioContext();
    lib.synth.registerAudioContext(context);
  }
  // Within the click, which is what lets the page make sound.
  void context.resume();
  const current: Playing = {
    editor,
    pos,
    source: editor.state.sliceDoc(block.textFrom, block.textTo),
    tune,
    tuneObject,
    voices,
    transpose,
    synth: null,
    started: 0,
    duration: 0,
  };
  playing = current;
  changed(editor);
  const tracks = tuneObject.setUpAudio({
    voicesOff: voicesOff(view, voices),
    chordsOff: view.chordsOff,
    // abcjs plays a shown transposition at concert pitch, as for a transposing
    // instrument; `transpose=` moves the sound too (format.md §5.6 rule 6).
    midiTranspose: transpose,
  });
  // Every voice on the piano the app brings along; no drums.
  tracks.tracks = tracks.tracks.map((track) =>
    track.filter((event) => event.cmd === "text" || event.instrument !== PERCUSSION),
  );
  for (const track of tracks.tracks) {
    for (const event of track) if (event.cmd !== "text") event.instrument = 0;
  }
  const synth = new lib.synth.CreateSynth();
  try {
    await synth.init({
      audioContext: context,
      // abcjs takes the tracks of `setUpAudio` here, though its types name another shape.
      sequence: tracks as unknown as import("abcjs").AudioSequence,
      millisecondsPerMeasure: (tuneObject.millisecondsPerMeasure() * 100) / view.tempo,
      options: {
        soundFontUrl: SOUNDS,
        onEnded: () => {
          if (playing?.synth === synth) {
            playing = null;
            changed(editor);
          }
        },
      },
    });
    const { duration } = await synth.prime();
    if (mine !== generation) return; // stopped or replaced while loading
    synth.start();
    if (from > 0) synth.seek(from, "percent");
    current.synth = synth;
    current.duration = duration;
    current.started = context.currentTime - from * duration;
    changed(editor);
  } catch (e) {
    console.error("The score could not be played", e);
    if (mine === generation) stopPlayback();
  }
}

/** New view settings for the block that plays: it goes on with them where it was. */
function replay(editor: EditorView, pos: number, view: ScoreView): void {
  const was = playing;
  if (was?.pos !== pos || !abcjs) return;
  void play(
    abcjs,
    editor,
    pos,
    was.tune,
    was.tuneObject,
    was.voices,
    was.transpose,
    view,
    position(was),
  );
}

/** Stops playing when the block changes or goes, or the note closes. */
const player = ViewPlugin.define(() => ({
  update(update) {
    const was = playing;
    if (!was || was.editor !== update.view || !update.docChanged) return;
    const pos = update.changes.mapPos(was.pos, 1, MapMode.TrackDel);
    const block = pos === null ? undefined : scoreBlocks(update.state).find((b) => b.from === pos);
    if (
      block &&
      update.state.sliceDoc(block.textFrom, block.textTo) === was.source &&
      pos !== null
    ) {
      was.pos = pos;
    } else {
      stopPlayback();
    }
  },
  destroy() {
    if (playing) {
      generation++;
      playing.synth?.stop();
      playing = null;
    }
  },
}));

/** Loads abcjs once a note with music shows, then has the blocks drawn again. */
const loader = ViewPlugin.define((view) => {
  let done = false;
  const check = (state: EditorState): void => {
    if (abcjs || loadError !== null || scoreBlocks(state).length === 0) return;
    void loadAbcjs().then(() => {
      if (!done) view.dispatch({ effects: musicLoaded.of(null) });
    });
  };
  check(view.state);
  return {
    update(update) {
      if (update.docChanged || syntaxTree(update.startState) !== syntaxTree(update.state)) {
        check(update.state);
      }
    },
    destroy() {
      done = true;
    },
  };
});

/** Scores in live preview; `focused` says whether the editor has focus. */
export function scorePreview(focused: StateField<boolean>): Extension {
  const hasFocus = (state: EditorState): boolean => state.field(focused, false) ?? false;
  const preview = StateField.define<DecorationSet>({
    create: (state) => scoreDecorations(state, hasFocus(state)),
    update(decorations, tr) {
      const changed =
        tr.docChanged ||
        tr.selection !== undefined ||
        tr.effects.length > 0 ||
        tr.startState.readOnly !== tr.state.readOnly ||
        hasFocus(tr.startState) !== hasFocus(tr.state) ||
        syntaxTree(tr.startState) !== syntaxTree(tr.state); // the parser caught up
      return changed ? scoreDecorations(tr.state, hasFocus(tr.state)) : decorations;
    },
    provide: (field) => EditorView.decorations.from(field),
  });
  return [scoreViews, preview, loader, player, scoreTheme];
}

const scoreTheme = EditorView.theme({
  ".cm-score": {
    margin: "6px 0",
    padding: "4px 8px",
    border: "1px solid var(--line)",
    borderRadius: "6px",
    cursor: "text",
    // abcjs draws in currentColor; its own white backgrounds stay out.
    color: "inherit",
  },
  ".cm-score-tune + .cm-score-tune": { marginTop: "8px" },
  ".cm-score-bar": {
    display: "flex",
    flexWrap: "wrap",
    alignItems: "center",
    gap: "6px 12px",
    marginBottom: "6px",
    fontFamily: "system-ui, sans-serif",
    fontSize: "0.85em",
    color: "var(--muted)",
    cursor: "default",
  },
  ".cm-score-group": { display: "inline-flex", alignItems: "center", gap: "4px" },
  ".cm-score-label": { marginRight: "2px" },
  ".cm-score-button": {
    font: "inherit",
    padding: "1px 8px",
    border: "1px solid var(--line)",
    borderRadius: "4px",
    background: "var(--panel)",
    color: "var(--fg)",
    cursor: "pointer",
  },
  ".cm-score-button[aria-pressed='true']": {
    background: "var(--accent)",
    borderColor: "var(--accent)",
    color: "var(--bg)",
  },
  ".cm-score-amount": {
    minWidth: "3.2em",
    textAlign: "center",
    fontVariantNumeric: "tabular-nums",
  },
  ".cm-score svg": { display: "block" },
  ".cm-score-warnings": {
    margin: "2px 0 8px",
    padding: "6px 10px",
    borderLeft: "3px solid var(--warn-fg)",
    fontFamily: "system-ui, sans-serif",
    fontSize: "0.85em",
  },
  ".cm-score-warning-title": { fontWeight: "600" },
  ".cm-score-warning-list": { margin: "4px 0 0", padding: "0", listStyle: "none" },
  ".cm-score-warning": {
    display: "block",
    margin: "2px 0",
    font: "inherit",
    color: "inherit",
    background: "none",
    border: "none",
    padding: "0",
    textAlign: "left",
    cursor: "pointer",
    textDecoration: "underline dotted",
  },
});

// --- Exports ---

/** An `abc` block of the notes to export, as `export_scores` lists it. */
export interface ExportScore {
  source: string;
  transpose: number;
}

/** The block engraved: one SVG per tune, none if it could not be engraved. */
export interface EngravedScore extends ExportScore {
  tunes: string[];
}

/** Width of the staves of an exported score: 15.9 cm, inside A4 and Letter (ADR-0033). */
const EXPORT_STAFF_WIDTH = 600;

/**
 * Engraves the `abc` blocks of an export with the editor's preparation, in
 * black, off screen, for `export_notes` to put into the documents (ADR-0033).
 * A block abcjs warns about, or every block if abcjs cannot be loaded, gets
 * no tunes and stays source in the export, which reports it. Yields to the
 * window between blocks; `progress` hears how many are done.
 */
export async function engraveForExport(
  blocks: ExportScore[],
  progress?: (done: number) => void,
): Promise<EngravedScore[]> {
  if (blocks.length === 0) return [];
  await loadAbcjs();
  const lib = abcjs;
  if (!lib) return blocks.map((block) => ({ ...block, tunes: [] }));
  // abcjs measures text, so the score must be in the document, though unseen.
  const host = document.createElement("div");
  host.setAttribute("aria-hidden", "true");
  host.style.cssText =
    "position: absolute; left: -10000px; top: 0; width: 800px; visibility: hidden; pointer-events: none";
  document.body.append(host);
  const serializer = new XMLSerializer();
  const engraved: EngravedScore[] = [];
  try {
    for (const [index, block] of blocks.entries()) {
      const { abc, tunes, warnings } = parse(lib, block.source, block.transpose);
      let svgs: string[] = [];
      if (warnings.length === 0 && tunes > 0) {
        const targets = Array.from({ length: tunes }, () =>
          host.appendChild(document.createElement("div")),
        );
        lib.renderAbc(targets as unknown as Element, abc, {
          visualTranspose: block.transpose,
          staffwidth: EXPORT_STAFF_WIDTH,
          paddingleft: 0,
          paddingright: 0,
          foregroundColor: "#000000",
        });
        svgs = targets.flatMap((target) => {
          const svg = target.querySelector("svg");
          return svg ? [serializer.serializeToString(svg)] : [];
        });
        // A tune missing would leave out part of the block: all or nothing.
        if (svgs.length !== tunes) svgs = [];
        host.replaceChildren();
      }
      engraved.push({ ...block, tunes: svgs });
      progress?.(index + 1);
      await new Promise((resolve) => setTimeout(resolve, 0));
    }
  } finally {
    host.remove();
  }
  return engraved;
}
