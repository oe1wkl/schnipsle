---
id: 01M8EAJZW03EE3QFV9HZ0DKC6T
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Notes and pictures

## Writing a note

**New** (⌘N on the Mac) starts a note with the heading "New note", and puts the cursor below it. Change the heading to the note's title and write on.

There is no Save button. Schnipsle saves a moment after you stop typing, and when you leave the note; the status line under the note says "Saved" with the time. If a note cannot be saved, for example because the vault's folder has gone, the note stays open and the status line says why, so nothing you typed is lost.

The note's title is its first heading, or else its first line. When you leave a note whose title has changed, Schnipsle renames its file to match: "Shopping list" is saved as `shopping-list.md`. A name that is taken gets a number (`shopping-list-2.md`); no file is ever replaced.

How to write headings, lists, tasks, tables and links is in [Markdown](markdown.md).

## The list of notes

The list shows the notes of the vault, or those of the tag, smart folder or search you picked; the line above it says which, and **Show all** goes back to all notes. The menu above the list sorts them by **Recently changed** or by **Title**. On the iPhone, ‹ goes back from a note to the list.

- 📌 marks a pinned note. **Pin** in the bar under a note pins it, and pinned notes come first in every list. **Unpin** takes it back. Pinning writes `pinned: true` into the note's front matter and does not count as a change.
- A cloud marks a note that is in iCloud but not on this device yet. Pick it, and Schnipsle asks iCloud for it and opens it when it is here. See [Sync with iCloud](sync-with-icloud.md).

## The bar under a note

| Button  | What it does                                                                       |
| ------- | ---------------------------------------------------------------------------------- |
| Pin     | Lists the note first; Unpin takes it back                                          |
| History | Shows the earlier versions of the note ([History and trash](history-and-trash.md)) |
| Export  | On the iPhone: exports the note ([Export](export.md))                              |
| Image   | Adds pictures to the note                                                          |
| Music   | Adds a score, or changes the one at the cursor ([Music](music.md))                 |
| Delete  | Moves the note to the trash; Undo brings it back                                   |
| Source  | Shows the note as Markdown; Preview shows it formatted again                       |

The tags of the note are shown in the bar as well.

## Pictures

There are three ways to put a picture into a note:

- paste it (⌘V), for example a screenshot or a picture copied from another app;
- drop it onto the note, from Finder or another app;
- tap **Image** and pick one or more pictures; on the iPhone from your photos, the camera or the Files app.

The picture goes where the cursor is. Schnipsle shows it in the note, and on the line with the cursor you see its Markdown in front of it, which you can change: the text in square brackets describes the picture, for VoiceOver and for exports.

This is a picture made for this manual:

![The lake in the morning](../../attachments/82/826812b6f31983eb0222c945957536e05d68d44018d5f87701131ecb799afec4.png)

### Where pictures are kept

Schnipsle copies each picture into the folder `attachments` of the vault and names the copy after its content, such as `attachments/82/8268…ec4.png`; a file beside it notes the picture's original name. The same picture pasted twice, or into two notes, is kept once. The note links to the copy with a path from the note's own file (`../attachments/…`), so the note shows it in any Markdown editor that opens the vault.

Removing a picture from a note removes only the link. The picture stays in `attachments`, as an earlier version of the note in its history, or another note, may still show it.

Pictures that are next to a note in the folder `notes`, linked by a path from the note, are shown too, for example in notes imported from another app.

### Other files

Drop a PDF, a recording or any other file onto a note, and Schnipsle keeps it in `attachments` as well, with a link to it in the note: `[datasheet.pdf](../attachments/…)`. On the Mac, ⌘-click on the link opens the file in the app it belongs to.

Pictures in HEIC, the format of the iPhone's camera, are kept as they are. Exports turn them into JPEG, as most programs other than Apple's cannot show HEIC. See [Export](export.md).

## Trying it out

[Example: A walk by the lake](example-a-walk-by-the-lake.md) is a note with a picture and tags. Change it as you like: it is your copy of the manual.

#schnipsle_manual
