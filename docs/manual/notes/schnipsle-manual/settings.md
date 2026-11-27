---
id: 01M8EAJZW0JJPMEYKV1014546V
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Settings

**Settings…** (⌘,) in the Schnipsle menu on the Mac, or ⚙︎ at the top on the iPhone, opens the settings. Schnipsle needs few of them: most of what you see is decided by what you write.

## This vault

These settings are kept in the vault, so they apply on every device that opens it, and each vault has its own.

**Earlier versions of a note.** How long the [history](history-and-trash.md) keeps versions: every version for 7 days, then one a day until 90 days, then one a week. Change the two numbers to keep more or fewer. If the new numbers would remove versions, Schnipsle says how many, of how many notes, and asks: **Remove them** removes them at once, on every device; **Keep them** leaves the numbers as they were.

**Chord names in song sheets.** **English: B and B♭** or **German: H and B**, for the chord names in [song sheets](music.md). A song sheet can still say otherwise for itself.

A vault that is open read-only shows its settings but cannot change them.

## This Mac

These settings are saved on this Mac only.

**Pandoc, for Word and EPUB.** Shows the Pandoc that [Export](export.md) uses for Word and EPUB, and its version, or that none was found. **Choose Program…** picks another one, for example a newer version you installed elsewhere; **Find It Automatically** goes back to looking for it. A Pandoc that does not run is named, and Word and EPUB are then not offered.

**Check for updates once a day.** When this is on, Schnipsle will ask GitHub once a day whether a newer version exists, and send nothing else. The check itself comes with a later version; the setting is kept until then.

## Manual

**Open the manual** opens the title page of this manual, [Schnipsle manual](schnipsle-manual.md), as **Help › Manual** does on the Mac. In a vault without the manual, Schnipsle offers to add it: its notes go into the folder `schnipsle-manual`, and your own notes stay as they are. Notes of the manual you deleted are in the trash and are not added again.

## Diagnostics

How long Schnipsle's steps took since it started, such as opening the vault, a search or opening a note, and how much memory it uses. It helps when Schnipsle seems slow. The numbers are kept in memory only: nothing is saved or sent. **Copy** copies them as text, for example to send them with a question about Schnipsle.

## In the vault's file

The vault's settings are in `.notesapp/settings.yaml`, a text file you can also edit by hand:

```yaml
history_keep_all_days: 7
history_keep_daily_days: 90
chord_notation: german
```

Schnipsle changes only the lines it sets, so comments and lines it does not know stay. A value it cannot read counts as the default, and Settings says so.

#schnipsle_manual
