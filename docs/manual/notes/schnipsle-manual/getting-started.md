---
id: 01M8EAJZW075NMA896ANR8RF8C
created: 2026-11-27T12:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Getting started

Schnipsle keeps every note as a Markdown file: plain text that any other editor can open, today and in many years. The notes live in a folder called a **vault**. Instead of sorting notes into folders, you give them tags and find them by searching.

## Your vault

At its first start Schnipsle made a vault named "Schnipsle" for you, and put this manual in it:

- With iCloud Drive, the vault is in **iCloud Drive › Schnipsle**. The Mac and the iPhone use the same vault, so a note written on one shows up on the other.
- Without iCloud Drive, it is in **Documents** on the Mac, or under **On My iPhone › Schnipsle** in the Files app.

The vault's name is shown at the top of the window. **Open Vault…** (⌘O on the Mac) opens another folder as a vault. A folder that is not a vault yet becomes one after you agree: Schnipsle adds the folders `notes` and `.notesapp` to it and changes nothing else.

Inside a vault, the notes are in `notes`; `.notesapp` holds what Schnipsle keeps beside them, such as the history of each note and the trash. [Vaults and files](vaults-and-files.md) tells more, and [Sync with iCloud](sync-with-icloud.md) how the vault gets to your other devices.

## Notes

**New** (⌘N on the Mac) starts a note. Just type: Schnipsle saves as you go, and the list of notes shows the most recently changed first.

The first line is the note's title. A heading works best:

```markdown
# Shopping list

- bread
- tomatoes
```

The text is Markdown: `# Heading`, `**bold**`, `*italic*`, `- lists`, `- [ ] tasks`, links, tables and pictures, all explained in [Markdown](markdown.md). While you write, Schnipsle shows the text formatted; the line with the cursor shows its Markdown.

To follow a link, such as the ones to the other parts of this manual, tap it on the iPhone, or ⌘-click it on the Mac. To change a link on the iPhone, tap beside it first: in the line with the cursor, a tap on a link edits it.

## Tags

A word with `#` in front is a tag: `#recipes`, `#travel`. A `/` puts a tag inside another: `#travel/italy` is found under `#travel` too. Tags can go anywhere in the text; many people put them on the last line, as this manual does.

The tags of the vault are listed as folders. A tag folder holds every note with that tag, and its subfolders hold the notes that also have a second tag. The tag `#schnipsle_manual` gathers this manual. More in [Tags and smart folders](tags-and-smart-folders.md).

## Search

Type into the search field to find notes by their words, tags, dates and more. A search you need again can be kept as a **smart folder**. Both are described in [Query language](query-language.md).

## Where to go next

The manual's other parts are listed in [Schnipsle manual](schnipsle-manual.md).

#schnipsle_manual
