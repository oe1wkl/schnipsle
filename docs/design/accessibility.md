# Accessibility check list

What must work with the keyboard on the Mac and with VoiceOver on the Mac and the iPhone (ADR-0041 rule 9). Go through it before a release and after changing a view; note the date, device and system next to each view.

How: on the Mac, turn on "Keyboard navigation" (System Settings › Keyboard) and use only the keyboard; then VoiceOver (⌘F5) with VO-arrows. On the iPhone, VoiceOver with swipes and double taps.

## Everywhere

- [ ] Tab reaches every control in the order it is seen; a focus ring shows where.
- [ ] Every button and field is announced by name and role; no "button" without a name.
- [ ] Toggles announce their state (Pin, Source, Trash, Compare, the score's Chords, M and S).
- [ ] The status line is read out when it changes (save, conflict, rename, export).
- [ ] No text in the app is missing from `ui/src/strings/en.ts` (`pnpm check` runs `scripts/check-strings.mjs` for the page).

## Bar and banners

- [ ] ‹ (Back), New, Open Vault…, ⚙ (iPhone) are named.
- [ ] The setup, read-only, outside-notes and conflict banners are announced when they appear, and their buttons are reached next.

## Sidebar

- [ ] Search: typing filters; Esc clears.
- [ ] Smart folders and tags are one Tab stop each; ↑ ↓ Home End move; → and ← open, close and move in and out; Return selects.
- [ ] ✎ (rename tag, edit smart folder) and ≡ (move) are reached with Tab from their row and announced with the folder's name.
- [ ] ≡ with the arrow keys moves the smart folder, and focus stays on it.

## Note list

- [ ] One Tab stop; ↑ ↓ Home End move; Return opens the note.
- [ ] The open note is announced as current.
- [ ] In the trash, Restore is reached with Tab from its row.

## Editor

- [ ] Tab leaves the editor; ⌘] and ⌘[ indent.
- [ ] Task checkboxes are announced as done or not done.
- [ ] A score is announced as a music score with its title; its bar's buttons (Play, Tempo, Transpose, voices) are named.
- [ ] A song sheet is read as text; its controls (transpose, chord names, lyrics only) are named.
- [ ] The Music menu opens from its button, its items are reachable, Esc closes it and focus returns to the button.

## History

- [ ] The list of versions is one Tab stop with ↑ ↓; Return shows a version.
- [ ] Compare, the choice of what to compare with, Restore, As new note and Close are reached with Tab.
- [ ] In a comparison, removed and added lines are announced as such, not only by colour.

## Dialogs

For each of Smart folder, Rename tag, Import, Imports, Export, Settings:

- [ ] Focus starts in the dialog and stays in it; Tab goes round its controls.
- [ ] Esc closes it (not while it is writing), and focus goes back to where it was.
- [ ] Reports and errors are announced when they appear.
