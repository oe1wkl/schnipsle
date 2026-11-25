# ADR-0048: A default vault, with the manual as notes

- Status: Accepted
- Date: 2026-11-25
- Deciders: Willi Kraml
- Related: N-02, F-07, F-14, ADR-0004, ADR-0006, ADR-0036, ADR-0039, ADR-0042, ADR-0046, ADR-0049

## Context

At its first start Schnipsle shows an empty window and asks for a vault: a folder to pick, or an empty one to make a vault of. Someone trying the app has no notes yet and does not know what a vault is. The roadmap planned the manual as Markdown pages in `docs/manual/` (keyboard, music and the query language exist), opened from the Help menu.

The owner asked on 2026-11-25 for:

- a default vault, made when the app is installed, in a folder that is empty, or else in a new folder with a number added (`Schnipsle 1`, `Schnipsle 2`, …);
- the manual as notes in that vault, which describe and show every feature with examples, each carrying a tag such as `#schnipsle_manual`;
- the manual notes exported as one PDF (or another format) and printed, with a title page, a table of contents and the notes in an order of their own (ADR-0049).

Neither a Mac app from a DMG nor an iPhone app runs anything when it is installed, so "at installation" is the first start, when the app has no vault remembered.

The same vault should reach the Mac and the iPhone without the user picking folders on both. An iPhone app can make folders in iCloud Drive only in its own iCloud container, whose `Documents` folder iCloud Drive shows under the app's name. A Mac app finds the same folder only if it uses the same container, which needs the iCloud Documents entitlement and a provisioning profile on the Mac as well, also for a Developer ID build (ADR-0036).

On a second device, iCloud may not yet have downloaded the vault the first device made, nor even listed it. A device that decides from what is on its disk alone could make a second vault with the same name, and iCloud would merge the two folders.

## Decision

**At its first start, with no vault remembered, Schnipsle makes or opens a vault named "Schnipsle" in its own iCloud folder, or on the device if iCloud Drive is off, and adds the manual to a new vault. The manual is written as a vault in the repository and shipped with the app.**

1. **When.** At a start with no vault remembered, before the empty window. A vault the user picked later is never replaced. If the remembered vault cannot be reached, the app asks as today and does not make a default vault.
2. **Where.**
   - With iCloud Drive: a folder in `Documents` of the app's iCloud container `iCloud.cc.kraml.schnipsle`, shown in iCloud Drive as "Schnipsle". The Mac and the iPhone use the same container.
   - Without iCloud Drive (turned off, not signed in, or the container not available): `~/Documents` on the Mac, the app's `Documents` folder on the iPhone (shown in Files under "On My iPhone").
   - The vault folder is named "Schnipsle". The app's folder can then hold other vaults next to it, and the numbered names below stay possible.
3. **Make or open.**
   - The folder is not there or is empty: the app makes a vault in it (format.md §1–2) and adds the manual (rule 6).
   - The folder holds a vault (`.notesapp/vault.yaml`, also as an iCloud placeholder that is not downloaded yet): the app opens it, and adds the manual notes that are missing (rule 6).
   - The folder holds something else: the same with "Schnipsle 1", "Schnipsle 2", … , the first that is not there, empty or a vault.
4. **Ask iCloud first.** In the iCloud folder the app decides only after iCloud has listed the folder's contents, including what is not downloaded yet (`NSMetadataQuery`, first results). Until then it says it is waiting for iCloud. Offline, it offers "Use a folder on this device" (the folder of rule 2 without iCloud) and "Choose a folder…"; it never makes a vault in iCloud it could not list.
5. **Remembered** as a picked vault is (`vault_location.rs`). Open Vault… works as before. The app makes a default vault only once per device: one the user has deleted is not made again.
6. **The manual is notes.**
   - `docs/manual/` becomes a vault in the repository (`notes/`, `attachments/`, `.notesapp/vault.yaml`), and its notes follow format.md: each has a fixed `id`, `created` and `modified`, links to other manual notes as `[[Title]]` (§5.4), and the final tag line `#schnipsle_manual`.
   - The manual describes every feature and shows it with examples: notes, pictures, tags and tag folders, search and smart folders, music and song sheets, import, export, history and trash, sync, settings, keyboard. Example notes carry the tag too, so the tag folder holds the whole manual; tags an example needs to show something, such as a tag combination, are nested under it (`#schnipsle_manual/recipes`).
   - A book note (ADR-0049), "Schnipsle manual", links to the notes in the order they are read in and is the title page when the manual is exported. It also carries `#book`, the one tag of the manual not under `#schnipsle_manual`, to show how books can be gathered in a tag folder.
   - The notes are copied into the app (Tauri resources) when it is built. They go into the vault's `notes/schnipsle-manual/` folder, their pictures into `attachments/` (§6).
   - A manual note is missing if no note in the vault has its `id`, and neither its file nor its iCloud placeholder is in that folder. A manual note the user moved or edited is never added again, and one the user deleted is in the trash and not added again by the first start.
7. **Help › Manual** opens the book note. In a vault without the manual, it offers to add the manual to this vault.
8. **Help › Update the Manual** (after an update of the app that changes the manual; the app says so once):
   - A manual note whose text is that of an earlier version of the manual is replaced, its old text kept in the history (§7) first.
   - A manual note the user has changed is kept. The new text is written next to it as a copy with the same `id`, which the app shows as another version to choose from, as for a copy made by iCloud (ADR-0042, ADR-0046).
   - Missing notes are added. A report says what was replaced, kept and added.
   - The app ships the hashes of the earlier versions of each manual note to tell them apart from changed ones.
9. **The manual stays readable on GitHub.** The notes are Markdown with front matter; links between them are `[[Title]]`, which GitHub shows as text. `docs/manual/README.md` lists them in reading order.

No change to the vault format: the manual notes are ordinary notes.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| A: the default vault in the app's iCloud container, with a local fallback (chosen) | The Mac and the iPhone share it without picking folders | iCloud entitlement and provisioning profile on the Mac too; waiting for iCloud's listing |
| B: in the local `Documents` folder | No iCloud setup; nothing to wait for | No sync until the user moves the notes to another vault |
| C: a choice at first start with the iCloud folder preselected | The user sees where the notes go | One more screen before the first note; most people take the default |
| Vault folder inside the app's folder (chosen) | Numbered names and further vaults next to it | iCloud Drive › Schnipsle › Schnipsle |
| The app's folder itself as the vault | One folder fewer | No room for a second vault; numbered names impossible |
| Open an existing vault in the folder (chosen) | A second device and a reinstall find the notes | Rule 4 needed so that a vault not yet listed is not made twice |
| A new numbered folder whenever the folder is not empty | Never touches what is there | A second device or a reinstall makes a vault of its own beside the shared one |
| Manual as notes, the repository folder as a vault (chosen) | Searchable and exportable like any note; shows the features with real examples; one source | The manual's notes in the user's lists and tag tree; updates need rule 8 |
| Manual as pages shown in a help window | Never mixes with the user's notes | Cannot show tags, search or export on real notes; not printable as a book |
| Manual notes updated at every start | Always current | Replaces text the user may be using; a copy for every changed note at once |

## Consequences

- Positive: the first start shows notes at once, and they explain the app with examples the user can try and change. The manual prints as a book (ADR-0049). On a second device the same vault opens.
- Negative:
  - The Mac build needs the iCloud Documents entitlement and a provisioning profile, in the release job as well (ADR-0036); the iPhone build needs the container and `NSUbiquitousContainers` in `Info.plist`.
  - Manual notes count in lists, search and the tag tree; the user can trash them like any note.
  - Each manual change needs its earlier hashes kept, and an update of a changed note shows up as another version to choose.
- Follow-up work:
  - A spike on the Mac (Developer ID with the entitlement) and the iPhone: the container's `Documents` shown as "Schnipsle", `NSMetadataQuery` on a second device before the vault is downloaded, and the fallbacks.
  - Moving `docs/manual/` into a vault and writing the missing pages (getting started, vaults, tags and smart folders, import, export, history and trash, sync, settings); the links to its pages (format.md §9, comments in the code) follow the move.
  - Core: making the default vault and adding or updating the manual (`schnipsle-core`, with tests); the app shell: the container and the first start; the Help menu on the Mac and its place on the iPhone.
  - `docs/dev-setup.md`: the iCloud container and the provisioning profiles.
