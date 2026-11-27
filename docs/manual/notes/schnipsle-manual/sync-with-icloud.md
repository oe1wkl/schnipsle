---
id: 01M8EAJZW027ZQ93MNEXYW1HE9
created: 2026-11-27T23:00:00+01:00
modified: 2026-11-27T23:00:00+01:00
---

# Sync with iCloud

Schnipsle has no server and no account of its own. To have your notes on the Mac and the iPhone, keep the vault in iCloud Drive: iCloud copies the files between your devices, and Schnipsle shows what arrives.

## The shared vault

At its first start, Schnipsle made the vault "Schnipsle" in its folder in iCloud Drive, or opened the one your other device had made there already ([Getting started](getting-started.md)). In Finder and in the Files app it is under **iCloud Drive › Schnipsle**. Nothing else needs setting up: a note written on the Mac is on the iPhone moments later, and the other way round.

A vault you picked yourself syncs the same way if its folder is in iCloud Drive. Open the same folder with **Open Vault…** on each device.

Your vault is on one device only if:

- iCloud Drive is off for Schnipsle, or you are not signed in to iCloud, on the iPhone: the vault is under **On My iPhone › Schnipsle**;
- the Mac started Schnipsle before any iPhone did: the vault is in **Documents** on the Mac, and Schnipsle said so when it made it.

To share such a vault later, move its folder into iCloud Drive in Finder (on the Mac) or Files (on the iPhone), and open it there with **Open Vault…** on each device.

## Changes from other devices

Notes changed on another device, or by another app, show up by themselves: in the list, in the tag folders and in the search, and in the open note if you are not typing in it. You do not need to reload anything.

Without a connection you go on writing as usual. iCloud sends your changes when the device is online again.

## Notes not downloaded yet

iCloud may keep a note only in the cloud until it is needed, for example on a new device or when space is short. Such a note is listed with a cloud. Pick it, and Schnipsle asks iCloud for it ("Downloading … from iCloud…") and opens it when it is here. Without a connection it cannot be opened until you are online.

## When two versions meet

If a note is changed in two places before iCloud has passed on either change, there are two versions. Schnipsle never picks one silently: it shows a banner above the note, and the version you do not choose is kept in the note's history, where you can read or restore it later ([History and trash](history-and-trash.md)).

**While you are writing.** If the note changes on another device while you edit it, the banner says "This note was changed on another device while you were editing.":

- **Keep mine** saves your text over the other one;
- **Use other version** shows the other text instead of yours.

If the note was deleted on the other device, **Use other version** puts your text into the trash, from where you can restore it.

**After syncing.** If iCloud finds that two devices saved different versions, the banner names the other device and when it saved:

- **Keep this version** keeps the text you see;
- **Use other version** takes the other device's text;
- **Show the other version** lets you read it first.

**Copies.** Sometimes iCloud keeps both versions as two files, such as `shopping-list.md` and `shopping-list 2.md`. Schnipsle sees that they are one note (they have the same `id`) and says that the second is a copy with different text. Besides keeping either version, you can **Keep both as separate notes**; the later one then gets an `id` of its own.

Until you choose, the note stays open, so nothing gets lost by going to another note. On the Mac, Schnipsle asks before it quits with a choice still open.

## Other ways to sync

The vault is a plain folder, so other services that sync folders can carry it too. Schnipsle is made and tried with iCloud Drive, and it recognises the copies iCloud makes; with another service, check after a conflict that both versions are there.

#schnipsle_manual
