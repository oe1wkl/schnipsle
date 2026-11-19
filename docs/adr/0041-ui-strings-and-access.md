# ADR-0041: UI strings in one place, and access by keyboard and VoiceOver

- Status: Accepted
- Date: 2026-11-18
- Deciders: Willi Kraml
- Related: N-06, F-06, ADR-0003, ADR-0036, ADR-0039; roadmap M7 "Language: English only in 1.0, with the strings in one place ready for German"

## Context

N-06 asks for a UI that can be translated (English, German first) and used without a mouse. For 1.0 it was decided (2026-11-13) that the app ships in English only, with its strings in one place, so that German needs only a translation, not a search through the code.

Today the text is spread over four places:

- `ui/index.html`: labels, buttons, help text, `aria-label`s (about 540 lines);
- the UI modules: messages, reports and counts built with template strings (`main.ts`, `export.ts`, `import.ts`, …), with plurals written by hand;
- `src-tauri/src/lib.rs`: the labels of the Mac menu;
- Rust error messages and report reasons, shown as they come (`CommandError.message`).

Access is partly there: dialogs are native `<dialog>` elements opened with `showModal`, banners and errors have `role="alert"` or `role="status"`, toggles have `aria-pressed`, and the smart-folder tree can be reordered with the arrow keys. Missing are, among others, arrow keys in the note list and the tag tree (`role="tree"` without the keys a tree promises), a visible focus ring everywhere, names for some icon buttons, and focus going back where it came from after a dialog.

## Decision

**Every text the user reads in the app comes from one catalog per language; 1.0 has only English. Every view can be used with the keyboard on the Mac and with VoiceOver on the Mac and the iPhone, checked against a written list.**

### Strings

1. **The catalog.** `ui/src/strings/en.ts` holds all UI text as one object, grouped by view (`sidebar`, `editor`, `history`, `export`, …). A text with values is a function: `renamed: (n: number) => …`. Plurals use `Intl.PluralRules`, so German and other languages get their own forms; no hand-written `n === 1` outside the catalog.
2. **Picking the language.** `strings.ts` exports `t`, the catalog for the first of `navigator.languages` that exists, else English. A second language is a file with the type `Strings` (`typeof en`), so the compiler reports every missing or extra key. Dates and times keep the system's format (`toLocaleString()` without a language), since that follows the region the user set, which may differ from the language.
3. **The HTML.** `index.html` keeps its structure but no text. Elements name their text by key: `data-text="history.compare"`, and `data-label`, `data-title`, `data-placeholder` for the attributes. `applyStrings()` fills them at start, before the window is shown. A test fails if a key in the HTML is not in the catalog. The playgrounds call the same function.
4. **The Mac menu.** Its labels move into `src-tauri/src/strings.rs`, one table, English only in 1.0. A German menu is a second table, picked by the system language; the menu is built before the web view, so it cannot use the TypeScript catalog.
5. **Errors and reasons from Rust.** The UI words what it knows by `kind`, with the path or name the error carries in `CommandError.value`; for any other kind it shows `CommandError.message`, Rust's English text, which for unexpected failures is mostly the system's own. Errors and report reasons a user meets in normal use have a `kind` or a code:
   - the vault errors: no vault open, not a folder, not a vault yet, inside a vault, a conflict, not downloaded, a name taken, an invalid path or id;
   - an invalid tag name (empty, not a tag, into its own subtag);
   - an import's skipped notes (`SkipReason`);
   - the export report's warnings, which become a `Warning` enum: links not put back, links or page numbers that could not be added, an SVG kept as SVG only. What Pandoc says is shown as Pandoc says it.

   Stay English in 1.0, and get codes when German comes: why a vault is read-only, the errors of the smart-folder file and the import log, the problems listed per imported note, and the system's I/O errors.
6. **Not UI text.** What is written into the vault or an export follows the notes, not the app: front matter keys, file names, the ABC templates of the Music menu, the "Contents" heading and the "Chorus" label in exports. They stay English in 1.0; choosing their language is a later export option. The title a new note starts with ("New note") is the exception: the user replaces it at once, so it comes from the catalog.

### Access

7. **Keyboard on the Mac.**
   - Every control can be reached with Tab, in the order it is seen, and shows a visible focus ring (`:focus-visible`).
   - Lists and trees are one Tab stop each; the arrow keys move inside them, Return opens, and Home and End go to the first and last item (the note list, the tag tree, the smart-folder tree, the history list). The tag tree follows the WAI-ARIA tree pattern its `role="tree"` promises: Right opens a folder, Left closes it or goes to its parent.
   - Every dialog closes with Esc, and focus goes back to the control that opened it.
   - In the editor, Tab moves on to the next control, as it does now (CodeMirror without `indentWithTab`), so the keyboard never gets stuck in it; ⌘] and ⌘[ indent and outdent. The manual says so.
   - Menu commands have shortcuts where the Mac has a usual one; the list is in the manual.
8. **VoiceOver on the Mac and the iPhone.**
   - Every control has a name: buttons that show only a sign (‹ back, ✎ rename, ≡ drag handle) get an `aria-label` from the catalog.
   - States are said: `aria-pressed`, `aria-expanded`, `aria-current`, `aria-selected`.
   - Changes that happen without moving focus are announced by the status line (`aria-live="polite"`) or an alert, such as "Saved", a conflict, a finished export.
   - A rendered score is one image named by its tunes' titles, not read note by note; the source stays readable in the editor. A song sheet is text and is read as such, title first.
9. **Checked by a list.** `docs/design/accessibility.md` lists, view by view, what must work with the keyboard and with VoiceOver. It is gone through on the Mac and an iPhone before 1.0, and after changes to a view.

Not in 1.0: the German UI itself, Dynamic Type and larger text on the iPhone (the CSS sizes are in `px`), right-to-left languages.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Own catalog in TypeScript with `Intl.PluralRules` (chosen) | No dependency; the compiler checks keys and values of every language; plurals right for every language | Translators edit a TypeScript file, not a standard format |
| i18next or FormatJS with JSON or ICU messages | Standard formats and tools for translators | A dependency and a message parser at run time for a few hundred strings; keys checked only by extra tooling |
| Text stays in `index.html`, only script text moves | Fewer changes now | Two places, which is what the roadmap decision rules out |
| Rust sends codes for every error | Everything translatable | Changes every error type in core and import; most are unexpected failures the user rarely sees |
| Keep Rust's English messages for all errors | No Rust changes | Common errors stay English when German comes |

## Consequences

- Positive:
  - German is one new file plus the menu table, checked by the compiler.
  - Plurals and numbers are right in every language, not only in English.
  - Keyboard and VoiceOver users can reach everything; the list keeps it that way.
- Negative:
  - `index.html` is harder to read without its text; the keys stand in for it.
  - Every new piece of UI text needs a catalog entry.
  - Some unexpected Rust errors remain English.
- Follow-up work:
  - `ui/src/strings/en.ts`, `strings.ts` with `t` and `applyStrings`, and the key test.
  - Moving the text of `index.html` and every UI module into the catalog.
  - `src-tauri/src/strings.rs` for the menu.
  - Kinds or codes for the common errors and reasons (rule 5).
  - Arrow keys in the note list, the tag tree and the history list; focus rings; names for sign-only buttons; focus back after dialogs.
  - `docs/design/accessibility.md`, gone through on the Mac and the iPhone.
  - The manual's keyboard section.
