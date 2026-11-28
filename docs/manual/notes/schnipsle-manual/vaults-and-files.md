---
id: 01M8EAJZW0N3TYXJHDAAF908K8
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Vaults and files

Your notes belong to you, not to Schnipsle. They are ordinary files in an ordinary folder, the **vault**, which you can open in Finder or the Files app, copy, back up, and edit with any other program. If you stop using Schnipsle one day, your notes stay as they are.

## What is in a vault

```text
Schnipsle/
  notes/                 your notes, one Markdown file each
  attachments/           pictures and other files in your notes
  .notesapp/
    vault.yaml           the vault's name and identity
    settings.yaml        settings for this vault
    smart-folders.yaml   your smart folders
    history/             earlier versions of each note
    trash/               deleted notes
    import-log/          a record of each import
```

- **notes** holds the notes as `.md` files. It may have folders of its own, such as the folder `schnipsle-manual` this manual is in; Schnipsle lists the notes of all of them together. Files and folders whose names start with `.` are left out.
- **attachments** holds pictures and files, each kept once, under a name made from its content ([Notes and pictures](notes-and-pictures.md)).
- **.notesapp** holds what Schnipsle keeps beside the notes. Its name starts with a dot, so Finder and Files hide it unless you ask them to show hidden files (⇧⌘. in Finder).

Everything in the vault is shared by the devices that open it, the settings and smart folders included. What only one device needs, such as the search index, is kept outside the vault, in the app's own folder on that device; Schnipsle builds it again by itself when it is missing.

Schnipsle never deletes files in a vault that it does not know. Other apps can keep their own files in it.

## A note's file

A note is a text file in UTF-8, named after its title: `shopping-list.md`. It starts with a few lines between `---`, its front matter, followed by the text in Markdown ([Markdown](markdown.md)).

```markdown
---
id: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T
created: 2026-11-20T08:15:00+01:00
modified: 2026-11-27T19:42:10+01:00
---

# Shopping list

- bread
```

The `id` belongs to the note for good. Schnipsle knows a note by it, not by its file name, so you can rename or move the file and the note keeps its history. A Markdown file without front matter, such as one written by another app, is a note too; Schnipsle adds the front matter the first time it saves the note, never just by opening it. Keys that other apps add to the front matter are kept.

## Working with other apps

You can edit notes with another editor, add files in Finder, or let a script write notes, while Schnipsle is open. Schnipsle sees the change at once, updates the list and the search, and shows the new text of an open note. If you were typing in that note at the same moment, it asks which version to keep ([Sync with iCloud](sync-with-icloud.md)); neither is lost.

If another app removes a note's front matter, Schnipsle writes back the `id` and `created` the note had, so it keeps its history.

You can sort notes into folders inside `notes` with Finder, and move them between folders. Schnipsle does not mind where a note is. Links from a moved note to pictures and other notes are paths from its file, though, so they may need changing; Schnipsle changes them only when it moves a note itself, such as when it restores a note from the trash into another folder.

## Opening another vault

You can have several vaults, for example one for work and one for home, and open one at a time. The name of the open vault is shown at the top of the window.

**Open Vault…** (⌘O on the Mac; the button at the top on the iPhone) opens another folder. Schnipsle remembers it and opens it at the next start.

- **A vault** opens as it is.
- **An empty folder** becomes a vault after you agree to **Make it a vault**: Schnipsle adds the folders `notes` and `.notesapp`.
- **A folder holding files**, such as Markdown notes from another app, becomes a vault after you agree to **Move into “notes” and open**. Schnipsle moves everything in it into a new folder `notes` inside it, and **What will be moved** shows what that is. Nothing is changed or deleted, and links between the files keep working. If some files are still in iCloud only, Schnipsle has them downloaded and asks you to try again once they are here.
- **A folder inside a vault** is not opened; open the vault itself.

To bring notes into a vault without moving them, use import instead: see [Import](import.md).

On the iPhone, pick the folder in iCloud Drive or under On My iPhone.

## A vault Schnipsle may not change

A vault made by a newer version of Schnipsle, whose files this version may not understand, opens read-only, and Schnipsle says so. So does a vault whose `vault.yaml` is damaged. You can read and search the notes, but not change them, until you update Schnipsle or repair the file.

## Notes outside `notes`

Notes that turn up beside the folder `notes`, at the top of the vault, are not listed. An older version of Schnipsle on another device may have saved them there. Schnipsle then offers to **Move into “notes”**. A note whose name is taken gets a number; nothing is replaced.

## Backups

A vault is a folder of files, so Time Machine and other backup programs keep it like any other folder. iCloud Drive is not a backup: a note deleted on one device is deleted on all. Within the vault, Schnipsle keeps earlier versions of each note and a trash ([History and trash](history-and-trash.md)).

#schnipsle_manual
