---
id: 01M8EAJZW0N8XSTV3YEPT2TDT1
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Import

**File › Import…** on the Mac brings notes from other apps into the open vault: from Apple Notes, Simplenote, web pages saved as HTML, and Markdown files from other editors. The iPhone has no import; notes imported on the Mac reach it through iCloud.

An import never changes the source, never replaces a note you changed in Schnipsle, and can be undone.

## How an import goes

1. Choose the app under **From**, and read the steps shown for it.
2. Optionally, write tags into **Add tags**, such as `#import/2026-11`. Every imported note gets them, so you find the whole batch in one tag folder and can sort it out from there.
3. Choose the folder or file to import (**Choose folder…**, **Choose file…** or **Choose zip file…**).
4. Schnipsle reads it and lists every note with what it would do: **New**, **Update** or **Skipped**, and where the note would go, or why not. Nothing is written yet.
5. **Import** writes the notes, with a progress bar. Afterwards the list shows each note, and a click opens it. Imported notes can be searched at once.

Each note keeps the dates it had in the other app, where the app exports them. The notes are written into a folder of their own inside `notes`, named after the app, such as `notes/Simplenote`. The folders a note was in become tags, on a line at the end of the note: a note from the folder "Radio" in "Projects" gets `#Projects/Radio`. Tags from **Add tags** and tags the other app keeps go on the same line. A tag the text has already is not added again.

A line in an imported note that would turn into a tag by mistake, such as `#define LED 13` in a piece of code that is not marked as code, is pointed out in the list.

Choosing a folder inside the open vault, or one that holds it, is refused.

## Importing again

Import the same export again later, and Schnipsle brings only what is new:

- notes it imported before are updated if they are newer in the export and you have not changed them here;
- notes you changed here since are kept as they are, and the list says so;
- notes you deleted here since are not brought back.

After an import was undone, importing the same export again brings its notes back.

## From Apple Notes

Apple Notes exports notes as Markdown, but leaves out when they were written and which folder they were in. So the import takes two steps:

1. In Notes, open a folder, select its notes (⌘A) and choose **File › Export as › Markdown**. Save them into a folder of their own inside one folder for the import, such as "Schnipsle-Import". Do this for each folder you want.
2. In Schnipsle, choose that folder. With **Read dates and folders from Notes**, Schnipsle reads them from Notes; macOS asks once whether it may. Nothing in Notes is changed. Reading takes about half a minute for a few hundred notes.

Without the dates and folders, every note gets the time of the export, and a later import with them would add the notes a second time. So let Schnipsle read them.

Locked notes are not exported by Notes. Nor are notes with the same title as another note in their folder; the list names them, so you can rename them in Notes and export again.

## From Simplenote

In Simplenote, choose **File › Export Notes** (in the web app: **Settings › Tools › Export Notes**). It saves a zip file; choose it as it is, without unpacking it.

Notes keep their dates and pins. Simplenote's tags become tags at the end of each note, unless the text has them already. Notes in Simplenote's trash go into the trash here.

## From HTML files

Choose a folder of web pages (`.html`, `.htm`), such as pages saved from a browser, or a single page. Each page becomes a note, named after its first heading or its title, and its subfolders become tags. Pictures saved with a page or held in it are copied into the vault, as are files it links to in the folder; pictures and links on the web stay links.

## From Markdown files

Choose a folder of Markdown files (`.md`), such as notes from another editor, or a single file. Each file becomes a note as it is, its front matter included, and its subfolders become tags. A `title` in the front matter becomes the note's heading. Hidden folders, such as `.obsidian`, are left out. Pictures and files the notes link to in the folder are copied into the vault.

Songs in ChordPro (`.cho`, `.crd`, `.chopro`, `.chordpro`) become notes with their title and the song as a song sheet ([Music](music.md)).

To use a folder of Markdown notes as it is, without copying it, open it as a vault instead: see [Vaults and files](vaults-and-files.md).

## Undoing an import

**File › Imports…** lists the imports into the open vault, newest first, each with the tags it added and what it brought in. **Undo…** first shows what undoing would do:

- notes the import added go to the trash;
- notes it updated get their version from before the import back, from the history;
- notes you have changed here since stay as they are.

**Undo import** then does that, and marks the import as undone. Nothing is deleted for good: the notes are in the trash ([History and trash](history-and-trash.md)).

Each import is recorded in the vault, in `.notesapp/import-log`.

#schnipsle_manual
