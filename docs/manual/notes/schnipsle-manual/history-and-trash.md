---
id: 01M8EAJZW0ZPWBSW14Z8ECP8W7
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# History and trash

Schnipsle does not throw text away. Earlier versions of every note are kept in its history, and a deleted note goes to the trash, from where it comes back as it was.

## Versions of a note

Schnipsle keeps a version of a note:

- when you have paused typing for 30 seconds;
- when you leave the note;
- before the note is deleted;
- before its text is replaced: by a change from another device, by the version you did not keep in a conflict, by restoring an earlier version, or by Schnipsle itself, such as when it renames a tag, rewrites chords or updates an imported note.

A version that is the same as the last one is not kept again.

The versions are kept in the vault (`.notesapp/history`), so every device that opens it has them, compressed and filed by the note's `id`. They stay with the note when you rename it, move it or restore it from the trash.

## Looking at the history

**History**, in the bar under a note, shows its earlier versions, newest first, in place of the note; **Close history** goes back. Pick a version to read it. It is shown like the note, formatted or as Markdown as you chose with **Source**.

**Compare** shows what changed between the version you picked and the note as it is now. **Compare with** chooses another version instead. Schnipsle always compares the older with the newer:

- lines removed and lines added are shown one under the other, as Markdown, with the words that changed inside a line marked;
- a count says how many lines were added and removed;
- changes in the front matter are listed by name, such as a different `modified` time;
- unchanged lines far from any change are folded: "… 12 unchanged lines"; **Show them** opens them.

## Getting a version back

- **Restore this version** makes the version you picked the note's text. The text it replaces is kept in the history first, so a restore can be undone by restoring that one. If the note has changes not saved yet, or a conflict, save or resolve it first; if the note was changed on another device meanwhile, the restore stops.
- **As new note** copies the version into a new note, with an `id` of its own, and leaves the note as it is. Use it to take back a paragraph without losing what you wrote since.

## How long versions are kept

Every version is kept for 7 days. After that, the last version of each day is kept until the version is 90 days old, and after that the last of each week.

Both numbers can be changed in [Settings](settings.md), for each vault. If a change would remove versions, Settings says how many and asks first.

## The trash

**Delete**, in the bar under a note, moves it to the trash, unchanged. **Undo** right after it brings it back.

**Trash**, above the list of notes, shows the deleted notes, with when each was deleted and where it was. Pick one to read it: it opens read-only, pictures included. **Restore** puts it back where it was, or into `notes` if that place is taken, and opens it for editing. A restored note keeps its `id` and its history.

The search leaves out the trash. To search it as well, add `in:trash` to the search ([Query language](query-language.md)); notes found there are marked, and can be restored from the Trash view.

The trash is in the vault (`.notesapp/trash`), shared by all devices. Schnipsle never empties it by itself. Each deleted note is a file there under its `id`, with a small file beside it recording when it was deleted and where it was; to remove a note for good, delete both files in Finder or Files.

## When a note changes elsewhere

If a note you are writing changes on another device at the same moment, or is deleted there, Schnipsle asks which version to keep and puts the other one into the history, or your text into the trash. See [Sync with iCloud](sync-with-icloud.md).

#schnipsle_manual
