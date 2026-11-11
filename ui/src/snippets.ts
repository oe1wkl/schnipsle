// SPDX-License-Identifier: Apache-2.0
// The music menu: snippets for ABC scores (format.md §5.6, ADR-0013).
//
// ABC has to be learnt. The menu writes what people look up most: a new
// score from a template, the key and metre of a tune, another voice, a line
// of lyrics, a chord symbol and another tune in the block. Each choice is one
// ordinary edit of the note, undone like typing; the rest of the block stays
// as written (rule 2).

import { syntaxTree } from "@codemirror/language";
import type { EditorState, TransactionSpec } from "@codemirror/state";
import { frontMatterLength } from "./editor";
import { scoreBlockAt } from "./score";

/** The ABC standard the format follows (format.md §5.6). */
export const ABC_STANDARD = "https://abcnotation.com/wiki/abc:standard:v2.1";

// --- Reading a block ---

interface Line {
  from: number;
  to: number;
  text: string;
}

/** A header field, such as `K:G`, or a `%%score` directive. */
interface Field {
  /** The value, after the field name and its spaces, before a `%` comment. */
  from: number;
  to: number;
  value: string;
}

interface Tune {
  lines: Line[];
  /** Index of the first line after the header (`lines.length` if none). */
  body: number;
  /** The number of the `X:` line, if it has one. */
  number: number | null;
  key: Field | null;
  /** Index of the `K:` line. */
  keyLine: number;
  metre: Field | null;
  /** The unit note length, `L:`. */
  unit: Field | null;
  /** `%%score` or `%%staves`, which list the voices that are shown. */
  score: Field | null;
  /** Voice ids in the order they first appear (`V:` lines and `[V:…]`). */
  voices: string[];
}

/** Where the cursor is, for the menu. */
export interface MusicPlace {
  /** In an `abc` block: the tune at the cursor (the last before it), all tunes. */
  block: { tune: Tune | null; tunes: Tune[]; lines: Line[]; textFrom: number } | null;
  /** In the body of the tune, where notes are, rather than in its header. */
  inBody: boolean;
}

const FIELD = /^([A-Za-z+]):/;
const SCORE_DIRECTIVE = /^%%(?:score|staves)\b/;

function blockLines(state: EditorState, from: number, to: number): Line[] {
  const lines: Line[] = [];
  if (from >= to) return lines;
  for (let n = state.doc.lineAt(from).number; n <= state.doc.lines; n++) {
    const line = state.doc.line(n);
    if (line.from > to) break;
    lines.push({ from: line.from, to: Math.min(line.to, to), text: line.text });
    if (line.to >= to) break;
  }
  return lines;
}

/** The value of a field from `start` on: spaces skipped, a `%` comment left out. */
function field(line: Line, start: number): Field {
  const rest = line.text.slice(start);
  const comment = rest.search(/(?<!\\)%/);
  const raw = comment < 0 ? rest : rest.slice(0, comment);
  const lead = raw.length - raw.trimStart().length;
  const value = raw.trim();
  const from = line.from + start + lead;
  return { from, to: from + value.length, value };
}

function readTune(lines: Line[]): Tune {
  const tune: Tune = {
    lines,
    body: lines.length,
    number: null,
    key: null,
    keyLine: -1,
    metre: null,
    unit: null,
    score: null,
    voices: [],
  };
  let i = 0;
  for (; i < lines.length; i++) {
    const line = lines[i] as Line;
    const directive = SCORE_DIRECTIVE.exec(line.text);
    if (directive) {
      tune.score ??= field(line, directive[0].length);
      continue;
    }
    if (line.text.startsWith("%")) continue;
    const name = FIELD.exec(line.text)?.[1];
    if (name === undefined) break;
    if (name === "X") tune.number = Number.parseInt(field(line, 2).value, 10) || null;
    if (name === "M") tune.metre ??= field(line, 2);
    if (name === "L") tune.unit ??= field(line, 2);
    if (name === "K") {
      tune.key = field(line, 2);
      tune.keyLine = i;
      i++;
      break;
    }
  }
  tune.body = i;
  for (const line of lines) {
    const ids = [
      /^V:\s*([^\s\]%]+)/.exec(line.text)?.[1],
      ...Array.from(line.text.matchAll(/\[V:\s*([^\]\s]+)/g), (m) => m[1]),
    ];
    for (const id of ids) if (id !== undefined && !tune.voices.includes(id)) tune.voices.push(id);
  }
  return tune;
}

/** The tunes of a block: each starts with an `X:` line (rule 1: the first may not). */
function readTunes(lines: Line[]): Tune[] {
  const groups: Line[][] = [];
  let current: Line[] = [];
  const hasText = (group: Line[]): boolean => group.some((l) => l.text.trim() !== "");
  for (const line of lines) {
    if (line.text.startsWith("X:")) {
      if (hasText(current)) groups.push(current);
      current = [];
    }
    current.push(line);
  }
  if (hasText(current)) groups.push(current);
  // Empty lines at the end belong to the space between tunes.
  return groups.map((group) => {
    let end = group.length;
    while (end > 0 && (group[end - 1] as Line).text.trim() === "") end--;
    return readTune(group.slice(0, end));
  });
}

export function musicPlace(state: EditorState): MusicPlace {
  const pos = state.selection.main.head;
  const found = scoreBlockAt(state, pos);
  if (!found) return { block: null, inBody: false };
  const lines = blockLines(state, found.textFrom, found.textTo);
  const tunes = readTunes(lines);
  const started = tunes.filter((t) => (t.lines[0]?.from ?? Infinity) <= pos);
  const tune = started.at(-1) ?? tunes[0] ?? null;
  // On a line of the tune, below its header; not on a fence or between tunes.
  const at = tune?.lines.findIndex((l) => l.from <= pos && pos <= l.to) ?? -1;
  const inBody = tune !== null && at >= 0 && at >= tune.body;
  return { block: { tune, tunes, lines, textFrom: found.textFrom }, inBody };
}

// --- Keys and metres ---

/** A key as the menu writes it: tonic, then `m` or a mode's first three letters. */
const KEY = /^([A-G][#b]?)(?:\s*(maj|ion|min|aeo|mix|dor|phr|lyd|loc|m)[a-z]*)?(?![A-Za-z#=])/i;

/** The key of a `K:` value in the menu's spelling (`Am`, `Ddor`), or `null`. */
export function keyOf(value: string): string | null {
  const m = KEY.exec(value);
  if (!m?.[1] || !/^[A-G]/.test(m[1])) return null;
  const mode = m[2]?.toLowerCase();
  const suffix =
    mode === undefined || mode === "maj" || mode === "ion"
      ? ""
      : mode === "m" || mode === "min" || mode === "aeo"
        ? "m"
        : mode;
  return m[1] + suffix;
}

export const KEYS: { label: string; keys: { value: string; label: string }[] }[] = [
  {
    label: "Major keys",
    keys: ["C", "G", "D", "A", "E", "F", "Bb", "Eb", "Ab"].map((k) => ({ value: k, label: k })),
  },
  {
    label: "Minor keys",
    keys: ["Am", "Em", "Bm", "F#m", "Dm", "Gm", "Cm"].map((k) => ({ value: k, label: k })),
  },
  {
    label: "Modes",
    keys: [
      { value: "Ddor", label: "D Dorian" },
      { value: "Edor", label: "E Dorian" },
      { value: "Ador", label: "A Dorian" },
      { value: "Gmix", label: "G Mixolydian" },
      { value: "Dmix", label: "D Mixolydian" },
      { value: "Amix", label: "A Mixolydian" },
    ],
  },
];

export const METRES = ["4/4", "3/4", "2/4", "2/2", "3/8", "6/8", "9/8", "12/8", "5/4", "C", "C|"];

export const CLEFS: { value: string; label: string }[] = [
  { value: "treble", label: "Treble" },
  { value: "treble-8", label: "Tenor (treble 8vb)" },
  { value: "alto", label: "Alto" },
  { value: "bass", label: "Bass" },
];

/** Shown with ♯ and ♭, written with `#` and `b` as ABC wants them. */
export const pretty = (key: string): string =>
  key.replace(/^([A-G])#/, "$1♯").replace(/^([A-G])b/, "$1♭");

// --- Edits ---

/** One menu choice as an edit, and whether the editor should take the focus after it. */
export interface Snippet {
  spec: TransactionSpec;
  focus: boolean;
}

interface Insert {
  from: number;
  to?: number;
  insert: string;
}

/** Puts `text` as a line of its own before line `index` of the tune (or after its end). */
function lineBefore(tune: Tune, index: number, text: string): Insert {
  const line = tune.lines[index];
  if (line) return { from: line.from, insert: `${text}\n` };
  const last = tune.lines.at(-1) as Line;
  return { from: last.to, insert: `\n${text}` };
}

/** Sets the tune's key (`K:`), keeping a clef or anything else the field says. */
export function setKey(tune: Tune, key: string): Snippet {
  if (!tune.key) {
    return { spec: { changes: lineBefore(tune, tune.body, `K:${key}`) }, focus: false };
  }
  const old = KEY.exec(tune.key.value);
  const length = old && /^[A-G]/.test(old[0]) ? old[0].length : 0;
  const rest = tune.key.value.slice(length);
  const space = length === 0 && rest !== "" ? " " : "";
  return {
    spec: { changes: { from: tune.key.from, to: tune.key.from + length, insert: key + space } },
    focus: false,
  };
}

/** Sets the tune's metre (`M:`); a new one goes before `K:`, which ends the header. */
export function setMetre(tune: Tune, metre: string): Snippet {
  if (tune.metre) {
    return {
      spec: { changes: { from: tune.metre.from, to: tune.metre.to, insert: metre } },
      focus: false,
    };
  }
  const at = tune.key ? tune.keyLine : tune.body;
  return { spec: { changes: lineBefore(tune, at, `M:${metre}`) }, focus: false };
}

/** A fraction such as `3/4` or `2+3/8`, or `null`. */
function fraction(value: string): number | null {
  const m = /^(\d+(?:\+\d+)*)\/(\d+)$/.exec(value.replace(/\s/g, ""));
  if (!m?.[1] || !m[2]) return null;
  const top = m[1].split("+").reduce((sum, n) => sum + Number(n), 0);
  return Number(m[2]) > 0 ? top / Number(m[2]) : null;
}

/** A rest as long as one bar of the tune (`z4`), or `Z` if that cannot be worked out. */
function barRest(tune: Tune): string {
  const value = tune.metre?.value;
  // C is 4/4 and C| is 2/2: one whole note either way.
  const metre = value === undefined ? null : value === "C" || value === "C|" ? 1 : fraction(value);
  if (metre === null) return "Z";
  // ABC's default unit is 1/16 below a metre of 3/4, otherwise 1/8.
  const unit = tune.unit ? fraction(tune.unit.value) : metre < 0.75 ? 1 / 16 : 1 / 8;
  if (unit === null) return "Z";
  const n = metre / unit;
  if (Math.abs(n - Math.round(n)) > 1e-9 || n < 1) return "Z";
  return n === 1 ? "z" : `z${String(Math.round(n))}`;
}

/**
 * Adds a voice at the end of the tune, with one bar's rest to start from.
 * Music before the first `V:` is voice 1, so a tune without voices gets a
 * `V:1` before its notes; a `%%score` line gets the new voice, or it would
 * not be shown.
 */
export function addVoice(tune: Tune, clef: string): Snippet {
  const ids = tune.voices;
  const numbers = ids.map(Number);
  let next = ids.length === 0 ? 2 : ids.length + 1;
  if (ids.length > 0 && numbers.every((n) => Number.isInteger(n))) next = Math.max(...numbers) + 1;
  while (ids.includes(String(next))) next++;
  const id = String(next);
  const changes: Insert[] = [];
  if (ids.length === 0 && tune.body < tune.lines.length) {
    changes.push(lineBefore(tune, tune.body, "V:1"));
  }
  if (tune.score) changes.push({ from: tune.score.to, insert: ` ${id}` });
  const last = tune.lines.at(-1) as Line;
  const rest = barRest(tune);
  changes.push({ from: last.to, insert: `\nV:${id} clef=${clef} name="Voice ${id}"\n${rest} |` });
  return { spec: { changes }, focus: false };
}

/** A `w:` line under the line with the cursor, to type the syllables in. */
export function addLyrics(state: EditorState): Snippet {
  const line = state.doc.lineAt(state.selection.main.head);
  const insert = "\nw:";
  return {
    spec: { changes: { from: line.to, insert }, selection: { anchor: line.to + insert.length } },
    focus: true,
  };
}

/** A chord symbol at the cursor, `"C"`, with the name selected to type over. */
export function addChord(state: EditorState): Snippet {
  const { from, to } = state.selection.main;
  return {
    spec: { changes: { from, to, insert: '"C"' }, selection: { anchor: from + 1, head: from + 2 } },
    focus: true,
  };
}

// --- Templates ---

export const TEMPLATES: { id: string; label: string; abc: string }[] = [
  {
    id: "melody",
    label: "Melody with chords",
    abc: `X:1
T:Title
M:4/4
L:1/8
Q:1/4=100
K:C
"C" C2 E2 G2 E2 | "F" F2 A2 "G" G2 B2 | "C" c8 |]`,
  },
  {
    id: "two-voices",
    label: "Two voices",
    abc: `X:1
T:Title
M:4/4
L:1/4
Q:1/4=90
V:1 name="Voice 1" clef=treble
V:2 name="Voice 2" clef=bass
K:C
V:1
E F G2 | c B c2 |]
V:2
C, D, E,2 | E, G, C,2 |]`,
  },
  {
    id: "choir",
    label: "Choir (SATB)",
    abc: `X:1
T:Title
M:4/4
L:1/4
Q:1/4=80
%%score [S A T B]
V:S name="Soprano" clef=treble
V:A name="Alto" clef=treble
V:T name="Tenor" clef=treble-8
V:B name="Bass" clef=bass
K:G
V:S
B A G2 |]
w:La la la
V:A
G F D2 |]
w:La la la
V:T
d d B2 |]
w:La la la
V:B
G, D G,2 |]
w:La la la`,
  },
];

/** Selects the placeholder title, to type over. */
function titleSelection(at: number, abc: string): { anchor: number; head: number } {
  const title = abc.indexOf("T:Title") + 2;
  return { anchor: at + title, head: at + title + "Title".length };
}

/**
 * A new score after the paragraph or block at the cursor, with an empty line
 * before and after it.
 */
export function newScore(state: EditorState, abc: string): Snippet {
  const { doc } = state;
  let pos = Math.max(state.selection.main.head, frontMatterLength(doc.toString()));
  // Not inside another code block: after it.
  for (let node = syntaxTree(state).resolveInner(pos, 0); ;) {
    if (node.name === "FencedCode") pos = node.to;
    const parent = node.parent;
    if (!parent) break;
    node = parent;
  }
  let line = doc.lineAt(Math.min(pos, doc.length));
  const block = `\`\`\`abc\n${abc}\n\`\`\``;
  const filled = (n: number): boolean => n >= 1 && n <= doc.lines && doc.line(n).text.trim() !== "";
  let from: number;
  let before: string;
  if (line.text.trim() === "") {
    from = line.from;
    before = filled(line.number - 1) ? "\n" : "";
  } else {
    // At the end of the paragraph, not in its middle.
    while (filled(line.number + 1)) line = doc.line(line.number + 1);
    from = line.to;
    before = "\n\n";
  }
  const after = filled(line.number + 1) ? "\n" : "";
  const insert = before + block + after;
  const start = from + before.length + "```abc\n".length;
  return {
    spec: { changes: { from, insert }, selection: titleSelection(start, abc) },
    focus: true,
  };
}

/** Another tune at the end of the block, numbered after the others. */
export function newTune(place: NonNullable<MusicPlace["block"]>): Snippet | null {
  const last = place.lines.at(-1);
  if (!last) return null;
  const numbers = place.tunes.map((t) => t.number ?? 0);
  const number = Math.max(place.tunes.length, ...numbers) + 1;
  const abc = (TEMPLATES[0] as { abc: string }).abc.replace("X:1", `X:${String(number)}`);
  const insert = `\n\n${abc}`;
  return {
    spec: { changes: { from: last.to, insert }, selection: titleSelection(last.to + 2, abc) },
    focus: true,
  };
}

// --- The menu ---

export interface MusicMenuHost {
  /** The editor's state, read when the menu opens. */
  state(): EditorState;
  /** Applies a choice as one user edit. */
  edit(snippet: Snippet): void;
  /** Opens a web page in the browser. */
  openLink(url: string): void;
}

function button(label: string, onClick: () => void, pressed = false): HTMLButtonElement {
  const b = document.createElement("button");
  b.type = "button";
  b.className = "small";
  b.textContent = label;
  if (pressed) b.setAttribute("aria-pressed", "true");
  b.addEventListener("click", onClick);
  return b;
}

function group(menu: HTMLElement, title: string, buttons: HTMLElement[]): void {
  const section = document.createElement("div");
  section.className = "music-group";
  const heading = document.createElement("div");
  heading.className = "music-heading";
  heading.textContent = title;
  const row = document.createElement("div");
  row.className = "music-row";
  row.append(...buttons);
  section.append(heading, row);
  menu.append(section);
}

/** Wires `opener` to a menu of snippets for the note in `host`. */
export function musicMenu(opener: HTMLButtonElement, host: MusicMenuHost): void {
  let menu: HTMLElement | null = null;

  const close = (): void => {
    menu?.remove();
    menu = null;
    opener.setAttribute("aria-expanded", "false");
    document.removeEventListener("pointerdown", outside, true);
    document.removeEventListener("keydown", escape, true);
    window.removeEventListener("resize", close);
  };
  const outside = (event: PointerEvent): void => {
    const target = event.target as Node;
    if (!menu?.contains(target) && !opener.contains(target)) close();
  };
  const escape = (event: KeyboardEvent): void => {
    if (event.key === "Escape") {
      event.preventDefault();
      close();
      opener.focus();
    }
  };
  const choose =
    (make: () => Snippet | null): (() => void) =>
    () => {
      const snippet = make();
      close();
      if (snippet) host.edit(snippet);
    };

  const build = (): HTMLElement => {
    const state = host.state();
    const place = musicPlace(state);
    const el = document.createElement("div");
    el.className = "music-menu";
    el.setAttribute("role", "dialog");
    el.setAttribute("aria-label", "Music");
    const tune = place.block?.tune ?? null;
    if (!place.block) {
      group(
        el,
        "New score",
        TEMPLATES.map((t) =>
          button(
            t.label,
            choose(() => newScore(host.state(), t.abc)),
          ),
        ),
      );
    } else if (tune) {
      const current = tune.key ? keyOf(tune.key.value) : null;
      for (const keys of KEYS) {
        group(
          el,
          keys.label,
          keys.keys.map((k) =>
            button(
              pretty(k.label),
              choose(() => setKey(tune, k.value)),
              current === k.value,
            ),
          ),
        );
      }
      const metre = tune.metre?.value ?? null;
      group(
        el,
        "Metre",
        METRES.map((m) =>
          button(
            m,
            choose(() => setMetre(tune, m)),
            metre === m,
          ),
        ),
      );
      group(
        el,
        "Add a voice",
        CLEFS.map((c) =>
          button(
            c.label,
            choose(() => addVoice(tune, c.value)),
          ),
        ),
      );
      const lyrics = button(
        "Lyrics line",
        choose(() => addLyrics(host.state())),
      );
      const chord = button(
        "Chord symbol",
        choose(() => addChord(host.state())),
      );
      if (!place.inBody) {
        for (const b of [lyrics, chord]) {
          b.disabled = true;
          b.title = "Put the cursor in the notes first";
        }
      }
      const block = place.block;
      group(el, "Insert", [
        lyrics,
        chord,
        button(
          "Another tune",
          choose(() => newTune(block)),
        ),
      ]);
    } else {
      // An empty block: a template inside it.
      const at = place.block.textFrom;
      group(
        el,
        "New score",
        TEMPLATES.map((t) =>
          button(
            t.label,
            choose(() => ({
              spec: {
                changes: { from: at, insert: `${t.abc}\n` },
                selection: titleSelection(at, t.abc),
              },
              focus: true,
            })),
          ),
        ),
      );
    }
    const help = document.createElement("a");
    help.href = ABC_STANDARD;
    help.className = "music-help";
    help.textContent = "ABC notation 2.1 ↗";
    help.addEventListener("click", (event) => {
      event.preventDefault();
      close();
      host.openLink(ABC_STANDARD);
    });
    el.append(help);
    return el;
  };

  opener.setAttribute("aria-haspopup", "dialog");
  opener.setAttribute("aria-expanded", "false");
  opener.addEventListener("click", () => {
    if (menu) {
      close();
      return;
    }
    menu = build();
    document.body.append(menu);
    const rect = opener.getBoundingClientRect();
    menu.style.bottom = `${String(window.innerHeight - rect.top + 6)}px`;
    // Right-aligned with the button, but inside the window.
    const width = menu.offsetWidth;
    const left = Math.max(12, Math.min(rect.right - width, window.innerWidth - 12 - width));
    menu.style.left = `${String(left)}px`;
    opener.setAttribute("aria-expanded", "true");
    document.addEventListener("pointerdown", outside, true);
    document.addEventListener("keydown", escape, true);
    window.addEventListener("resize", close);
    menu.querySelector<HTMLButtonElement>("button:not(:disabled)")?.focus({ preventScroll: true });
  });
}
