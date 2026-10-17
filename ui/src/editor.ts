// SPDX-License-Identifier: Apache-2.0
// Note editor: CodeMirror 6 with inline live preview and a plain-source mode (ADR-0007).
//
// The editor never rewrites Markdown on its own. Live preview only hides markers
// and draws widgets; the document stays exactly what the user typed. The only
// programmatic edit, `replaceFrontMatter`, takes in front matter the core wrote
// on save, and it is kept out of the undo history.

import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { yamlFrontmatter } from "@codemirror/lang-yaml";
import { HighlightStyle, syntaxHighlighting, syntaxTree } from "@codemirror/language";
import {
  Annotation,
  Compartment,
  EditorState,
  type Extension,
  Facet,
  type Range,
  Transaction,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  drawSelection,
  EditorView,
  keymap,
  placeholder,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import { tags as t } from "@lezer/highlight";

export type EditorMode = "live" | "source";

export interface NoteEditor {
  /** The complete note text. */
  text(): string;
  /** Shows a note; starts a fresh undo history. */
  load(text: string): void;
  setEnabled(enabled: boolean): void;
  setMode(mode: EditorMode): void;
  focus(): void;
  cursorToEnd(): void;
  /** Inserts `text` as a user edit at `pos`, or in place of the selection if `null`. */
  insert(text: string, pos: number | null): void;
  /**
   * Replaces the front matter block `oldBlock` at the start of the note with
   * `newBlock`, outside the undo history; the selection moves with the text.
   * Returns false (and changes nothing) if the note no longer starts with
   * `oldBlock`, i.e. the user edited the block meanwhile.
   */
  replaceFrontMatter(oldBlock: string, newBlock: string): boolean;
}

/** Length of a closed front matter block at the start of `text`, 0 if none (format.md §4). */
export function frontMatterLength(text: string): number {
  const match = /^\uFEFF?---\r?\n(?:.*\r?\n)*?---[ \t]*(?:\r?\n|$)/.exec(text);
  return match ? match[0].length : 0;
}

/** Marks transactions made by the app rather than the user. */
const programmatic = Annotation.define<boolean>();

// --- Language: Markdown (GFM), with YAML front matter only when it is closed ---

// No Markdown keymap: its Enter renumbers ordered lists, which rewrites text the
// user did not touch (ADR-0007).
const markdownSupport = markdown({ base: markdownLanguage, addKeymap: false });
const withFrontMatter = yamlFrontmatter({ content: markdownSupport });
const language = new Compartment();

// yamlFrontmatter would take a lone `---` at the start (a horizontal rule) as an
// unclosed front matter swallowing the whole note; the core treats that as body.
const FRONT_MATTER_PROBE = 8192;
const startsWithFrontMatter = (start: string): boolean =>
  frontMatterLength(start.slice(0, FRONT_MATTER_PROBE)) > 0;

const switchLanguage = EditorState.transactionExtender.of((tr) => {
  if (!tr.docChanged) return null;
  const before = startsWithFrontMatter(tr.startState.doc.sliceString(0, FRONT_MATTER_PROBE));
  const after = startsWithFrontMatter(tr.newDoc.sliceString(0, FRONT_MATTER_PROBE));
  return before === after
    ? null
    : { effects: language.reconfigure(after ? withFrontMatter : markdownSupport) };
});

// --- Live preview ---

class CheckboxWidget extends WidgetType {
  constructor(private readonly checked: boolean) {
    super();
  }
  override eq(other: CheckboxWidget): boolean {
    return other.checked === this.checked;
  }
  override toDOM(): HTMLElement {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.checked = this.checked;
    box.className = "cm-task";
    box.setAttribute("aria-label", this.checked ? "Done" : "Not done");
    return box;
  }
  override ignoreEvent(): boolean {
    return false;
  }
}

/** Turns an image link as written in the note into a URL the page can show. */
export type ImageLoader = (url: string) => Promise<string>;
const imageLoader = Facet.define<ImageLoader, ImageLoader | null>({
  combine: (loaders) => loaders[0] ?? null,
});

/** An image in live preview; shows its description if it cannot be loaded. */
class ImageWidget extends WidgetType {
  constructor(
    private readonly url: string,
    private readonly alt: string,
    private readonly load: ImageLoader,
  ) {
    super();
  }
  override eq(other: ImageWidget): boolean {
    return other.url === this.url && other.alt === this.alt;
  }
  override toDOM(view: EditorView): HTMLElement {
    const wrap = document.createElement("span");
    wrap.className = "cm-image";
    const img = document.createElement("img");
    img.alt = this.alt;
    img.addEventListener("load", () => {
      view.requestMeasure();
    });
    wrap.append(img);
    this.load(this.url).then(
      (src) => {
        img.src = src;
      },
      () => {
        wrap.className = "cm-image-missing";
        wrap.textContent = `Image not available: ${this.alt || this.url}`;
      },
    );
    return wrap;
  }
  override ignoreEvent(): boolean {
    return false; // a click puts the cursor there, which shows the Markdown
  }
}

const hide = Decoration.replace({});
const muted = Decoration.mark({ class: "cm-md-mark" });

/** Lines the user is editing: those touched by a selection, while focused. */
function editedLines(view: EditorView): (from: number, to: number) => boolean {
  if (!view.hasFocus) return () => false;
  const { doc } = view.state;
  const spans = view.state.selection.ranges.map((r) => [
    doc.lineAt(r.from).from,
    doc.lineAt(r.to).to,
  ]);
  return (from, to) => spans.some(([start = 0, end = 0]) => from <= end && to >= start);
}

function previewDecorations(view: EditorView): DecorationSet {
  const { state } = view;
  const edited = editedLines(view);
  const load = state.facet(imageLoader);
  const found: Range<Decoration>[] = [];
  for (const { from, to } of view.visibleRanges) {
    syntaxTree(state).iterate({
      from,
      to,
      enter: (node) => {
        const isEdited = edited(node.from, node.to);
        if (node.name === "Image") {
          // `![alt](url)` shows the image in place of the Markdown. On the line
          // being edited the Markdown shows too, with the image right after it,
          // so the image does not vanish under the cursor. A plugin may not
          // replace line breaks, so an image spread over lines only gets that.
          const url = node.node.getChild("URL");
          const altEnd = node.node.getChildren("LinkMark")[1]?.from;
          if (load === null || !url || altEnd === undefined) return;
          const widget = new ImageWidget(
            state.doc.sliceString(url.from, url.to),
            state.doc.sliceString(node.from + 2, altEnd),
            load,
          );
          const oneLine = state.doc.lineAt(node.from).to >= node.to;
          found.push(
            isEdited || !oneLine
              ? Decoration.widget({ widget, side: 1 }).range(node.to)
              : Decoration.replace({ widget }).range(node.from, node.to),
          );
          return false;
        }
        switch (node.name) {
          case "HeaderMark": {
            if (isEdited) break;
            const after = state.doc.sliceString(node.to, node.to + 1);
            found.push(hide.range(node.from, after === " " ? node.to + 1 : node.to));
            break;
          }
          case "EmphasisMark":
          case "StrikethroughMark":
            if (!isEdited) found.push(hide.range(node.from, node.to));
            break;
          case "CodeMark":
            if (!isEdited && node.node.parent?.name === "InlineCode") {
              found.push(hide.range(node.from, node.to));
            }
            break;
          case "Link": {
            // `[text](url "title")` shows as `text`: hide `[` and everything from `]`.
            if (isEdited) break;
            const marks = node.node.getChildren("LinkMark");
            const [open, close] = marks;
            if (open && close) {
              found.push(hide.range(open.from, open.to));
              found.push(hide.range(close.from, node.to));
            }
            break;
          }
          case "TaskMarker":
            if (!isEdited) {
              const checked = /x/i.test(state.doc.sliceString(node.from + 1, node.to - 1));
              found.push(
                Decoration.replace({ widget: new CheckboxWidget(checked) }).range(
                  node.from,
                  node.to,
                ),
              );
            }
            break;
          case "ListMark":
          case "QuoteMark":
            found.push(muted.range(node.from, node.to));
            break;
        }
      },
    });
  }
  return Decoration.set(found, true);
}

/** Toggles the task at the checkbox the user clicked: `[ ]` ↔ `[x]`. */
function toggleTask(view: EditorView, target: HTMLElement): boolean {
  const pos = view.posAtDOM(target);
  const marker = view.state.doc.sliceString(pos, pos + 3);
  if (!/^\[[ xX]\]$/.test(marker)) return false;
  view.dispatch({
    changes: { from: pos + 1, to: pos + 2, insert: marker[1] === " " ? "x" : " " },
    userEvent: "input.toggle",
  });
  return true;
}

const livePreview = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = previewDecorations(view);
    }
    update(update: ViewUpdate): void {
      if (
        update.docChanged ||
        update.viewportChanged ||
        update.selectionSet ||
        update.focusChanged
      ) {
        this.decorations = previewDecorations(update.view);
      } else if (syntaxTree(update.startState) !== syntaxTree(update.state)) {
        this.decorations = previewDecorations(update.view); // the parser caught up
      }
    }
  },
  {
    decorations: (plugin) => plugin.decorations,
    eventHandlers: {
      mousedown(event, view) {
        const target = event.target;
        if (target instanceof HTMLElement && target.classList.contains("cm-task")) {
          event.preventDefault();
          return toggleTask(view, target);
        }
        return false;
      },
    },
  },
);

/** Front matter lines in muted colour, in both modes: it is metadata, not the note. */
const frontMatterLine = Decoration.line({ class: "cm-frontmatter" });
function frontMatterDecorations(state: EditorState): DecorationSet {
  const end = frontMatterLength(state.doc.sliceString(0, FRONT_MATTER_PROBE));
  if (end === 0) return Decoration.none;
  const lines: Range<Decoration>[] = [];
  for (let pos = 0; pos < end;) {
    const line = state.doc.lineAt(pos);
    lines.push(frontMatterLine.range(line.from));
    pos = line.to + 1;
  }
  return Decoration.set(lines);
}
const mutedFrontMatter = ViewPlugin.fromClass(
  class {
    decorations: DecorationSet;
    constructor(view: EditorView) {
      this.decorations = frontMatterDecorations(view.state);
    }
    update(update: ViewUpdate): void {
      if (update.docChanged) this.decorations = frontMatterDecorations(update.state);
    }
  },
  { decorations: (plugin) => plugin.decorations },
);

// --- Styles ---

const MONO = 'ui-monospace, "SF Mono", Menlo, monospace';

const shared = [
  { tag: t.emphasis, fontStyle: "italic" },
  { tag: t.strong, fontWeight: "700" },
  { tag: t.strikethrough, textDecoration: "line-through" },
  { tag: t.link, color: "var(--accent)" },
  { tag: t.url, color: "var(--muted)" },
  { tag: t.quote, color: "var(--muted)" },
  { tag: [t.processingInstruction, t.contentSeparator, t.meta], color: "var(--muted)" },
  // YAML front matter
  {
    tag: [t.propertyName, t.definition(t.propertyName), t.string, t.number, t.bool],
    color: "var(--muted)",
  },
];

const liveStyle = HighlightStyle.define([
  ...shared,
  { tag: t.heading1, fontSize: "1.6em", fontWeight: "700" },
  { tag: t.heading2, fontSize: "1.35em", fontWeight: "700" },
  { tag: t.heading3, fontSize: "1.15em", fontWeight: "700" },
  { tag: [t.heading4, t.heading5, t.heading6], fontWeight: "700" },
  { tag: t.link, color: "var(--accent)", textDecoration: "underline" },
  { tag: t.monospace, fontFamily: MONO, fontSize: "0.9em" },
]);

const sourceStyle = HighlightStyle.define([
  ...shared,
  {
    tag: [t.heading1, t.heading2, t.heading3, t.heading4, t.heading5, t.heading6],
    fontWeight: "700",
  },
]);

const theme = EditorView.theme({
  "&": { flex: "1", minHeight: "0", color: "inherit", backgroundColor: "transparent" },
  "&.cm-focused": { outline: "none" },
  ".cm-scroller": { fontFamily: "inherit", lineHeight: "1.5", padding: "16px 0" },
  ".cm-content": { padding: "0 16px", caretColor: "var(--fg)", maxWidth: "48rem" },
  ".cm-cursor": { borderLeftColor: "var(--fg)" },
  ".cm-placeholder": { color: "var(--muted)" },
  ".cm-task": { margin: "0 0.35em 0 0", verticalAlign: "-0.1em", cursor: "pointer" },
  ".cm-md-mark": { color: "var(--muted)" },
  ".cm-frontmatter, .cm-frontmatter *": { color: "var(--muted)", fontSize: "0.9em" },
  ".cm-image img": { maxWidth: "100%", maxHeight: "24rem", verticalAlign: "bottom" },
  ".cm-image-missing": { color: "var(--muted)", fontStyle: "italic" },
});

const sourceTheme = EditorView.theme({
  ".cm-scroller": { fontFamily: MONO, fontSize: "15px" },
});

const modes: Record<EditorMode, Extension> = {
  live: [livePreview, syntaxHighlighting(liveStyle)],
  source: [syntaxHighlighting(sourceStyle), sourceTheme],
};

// --- The editor ---

export function createEditor(
  parent: HTMLElement,
  events: {
    onChange: () => void;
    onBlur: () => void;
    /** Files pasted or dropped into the note; `pos` is where they were dropped. */
    onFiles: (files: File[], pos: number | null) => void;
    loadImage: ImageLoader;
  },
): NoteEditor {
  const mode = new Compartment();
  const enabled = new Compartment();
  let currentMode: EditorMode = "live";
  let isEnabled = false;
  const enabledExtension = (on: boolean): Extension => [
    EditorView.editable.of(on),
    EditorState.readOnly.of(!on),
  ];

  const makeState = (doc: string): EditorState =>
    EditorState.create({
      doc,
      extensions: [
        history(),
        drawSelection(),
        EditorView.lineWrapping,
        keymap.of([...defaultKeymap, ...historyKeymap]),
        language.of(startsWithFrontMatter(doc) ? withFrontMatter : markdownSupport),
        switchLanguage,
        mutedFrontMatter,
        imageLoader.of(events.loadImage),
        mode.of(modes[currentMode]),
        enabled.of(enabledExtension(isEnabled)),
        placeholder("Select a note"),
        theme,
        EditorView.contentAttributes.of({
          spellcheck: "true",
          autocorrect: "on",
          autocapitalize: "sentences",
          "aria-label": "Note text",
        }),
        EditorView.updateListener.of((update) => {
          const byUser = update.transactions.some(
            (tr) => tr.docChanged && tr.annotation(programmatic) !== true,
          );
          if (byUser) events.onChange();
        }),
        EditorView.domEventHandlers({
          blur: () => {
            events.onBlur();
          },
          paste: (event) => {
            const files = Array.from(event.clipboardData?.files ?? []);
            if (files.length === 0) return false;
            event.preventDefault();
            events.onFiles(files, null);
            return true;
          },
          drop: (event, view) => {
            const files = Array.from(event.dataTransfer?.files ?? []);
            if (files.length === 0) return false;
            event.preventDefault();
            events.onFiles(files, view.posAtCoords({ x: event.clientX, y: event.clientY }));
            return true;
          },
        }),
      ],
    });

  const view = new EditorView({ state: makeState(""), parent });

  return {
    text: () => view.state.doc.toString(),
    load(text) {
      view.setState(makeState(text));
    },
    setEnabled(on) {
      isEnabled = on;
      view.dispatch({ effects: enabled.reconfigure(enabledExtension(on)) });
    },
    setMode(next) {
      currentMode = next;
      view.dispatch({ effects: mode.reconfigure(modes[next]) });
    },
    focus() {
      view.focus();
    },
    cursorToEnd() {
      view.dispatch({ selection: { anchor: view.state.doc.length }, scrollIntoView: true });
    },
    insert(text, pos) {
      const spec =
        pos === null
          ? view.state.replaceSelection(text)
          : { changes: { from: pos, insert: text }, selection: { anchor: pos + text.length } };
      view.dispatch(spec, { userEvent: "input.paste", scrollIntoView: true });
      view.focus();
    },
    replaceFrontMatter(oldBlock, newBlock) {
      if (view.state.doc.sliceString(0, oldBlock.length) !== oldBlock) return false;
      view.dispatch({
        changes: { from: 0, to: oldBlock.length, insert: newBlock },
        annotations: [Transaction.addToHistory.of(false), programmatic.of(true)],
      });
      return true;
    },
  };
}
