---
id: 01M8EAJZW03VZTVJVN7359P9RX
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Tags and smart folders

Schnipsle has no folders to sort notes into. A note gets tags instead, as many as it needs, and is found under each of them. Smart folders keep searches you need again.

## Writing tags

A tag is a `#` followed directly by a word: `#recipes`, `#travel`, `#Österreich`. Write it anywhere in the note; many put the tags on the last line, as this manual does.

- A tag can hold letters of any language, digits, `_` and `-`: `#cw_training`, `#2026-trip`. It needs at least one letter, so `#1` or `#2026` is not a tag.
- `/` puts a tag inside another: `#travel/italy` is a tag of its own, and the note is also found under `#travel`. Tags can go as deep as you like: `#projects/house/garden`.
- Capitals do not matter: `#Recipes` and `#recipes` are the same tag. Schnipsle shows it in the spelling used most often.
- A tag ends at a space or a sign such as `,` or `.`: in `Notes on #radio, #antennas.` the tags are `#radio` and `#antennas`.

Some things look like tags but are not:

| Written                                  | Why it is not a tag            |
| ---------------------------------------- | ------------------------------ |
| `# Heading`                              | `#` and a space make a heading |
| `#00ff00`, `#f80`                        | a colour code                  |
| `#123`                                   | no letter                      |
| `C#` and `F#`                            | nothing follows the `#`        |
| `page#section` in a link's address       | part of the address            |
| anything in code, scores and song sheets | code is never read for tags    |
| `\#literal`                              | the backslash keeps it a word  |

## Tag folders

The tags of the vault are listed under **Tags**: in the sidebar on the Mac, and behind the **Tags** button above the list on the iPhone. Each tag is a folder holding the notes that have it, with their number beside it. Pick a folder, and the list shows its notes; the line above the list says which folder it is, and **Show all** goes back to all notes. Notes in the trash are not counted.

Inside a tag's folder are two kinds of folders:

- **Its subtags.** `#travel` holds `travel/italy` and `travel/norway`.
- **The tags it occurs with.** If some notes tagged `#recipes` are also tagged `#quick`, the folder `recipes` holds a folder `quick`: the notes with both tags. Inside that you find the tags that occur with both, down to three tags.

So a note tagged `#recipes #quick #vegetarian` is found under `recipes`, under `quick` and under `vegetarian`, and under each of their combinations. You never decide which folder a note goes into.

**Untagged notes** holds the notes without any tag.

This manual has one example: every part of it has the tag `#schnipsle_manual`, and its title page [Schnipsle manual](schnipsle-manual.md) also has `#book`. So the folder `book` holds the folder `schnipsle_manual`, the book notes that are part of the manual. The examples have tags such as `#schnipsle_manual/recipes`, so they are subtags under `schnipsle_manual`.

## Renaming and merging tags

✎ next to a tag renames it in every note that has it. Its subtags move along: renaming `#projects` to `#work` turns `#projects/house` into `#work/house`. Smart folders that search for it are changed too.

Rename a tag to one that exists already, and the two are merged: the notes of both have the one tag afterwards.

Before anything is written, the dialog shows how many notes and smart folders change, and which. Then:

- Only tags change. A `#projects` in code, in a link's address or after `\` stays as it is.
- Each note keeps its version from before in its history.
- A note you changed after the dialog showed it, or on another device meanwhile, is left as it is, and the report names it.
- **Undo** puts the notes back, as long as Schnipsle runs.

To move a tag into another, rename it with the new parent in front: `#italy` renamed to `#travel/italy`. A tag cannot be moved into one of its own subtags.

## Smart folders

A smart folder is a search with a name. It is listed under **Smart folders**, above the tags, and always shows the notes that match now: a note you tag later turns up by itself.

To make one, search for something and tap **Save** next to the search field; or tap **New smart folder** and write the search. For example:

| Search                      | Finds                              |
| --------------------------- | ---------------------------------- |
| `#recipes -#tried`          | recipes not tried yet              |
| `has:task -#done`           | notes with tasks, not done         |
| `modified:7d`               | notes changed in the last week     |
| `#schnipsle_manual/recipes` | the example recipes of this manual |

What a search can say is in [Query language](query-language.md), as is how to change, sort, nest and delete smart folders. Deleting a smart folder never deletes notes.

Smart folders are kept in the vault (`.notesapp/smart-folders.yaml`), so all your devices have the same ones.

## Trying it out

The examples of this manual are in the tag folder `schnipsle_manual`, under `recipes` and `travel`:

- [Example: Pancakes](example-pancakes.md)
- [Example: Tomato soup](example-tomato-soup.md)
- [Example: Packing list](example-packing-list.md)

Add a tag such as `#breakfast` to the pancakes, and see where the note turns up. Remove it again, or keep it: the manual notes are yours.

#schnipsle_manual
