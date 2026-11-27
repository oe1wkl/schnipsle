# ADR-0050: The default vault on a second device and on the Mac

- Status: Accepted
- Date: 2026-11-26
- Deciders: Willi Kraml
- Related: N-02, F-07, ADR-0006, ADR-0015, ADR-0036, ADR-0046, ADR-0047, ADR-0048

## Context

ADR-0048 puts the default vault into the `Documents` folder of the app's iCloud container `iCloud.cc.kraml.schnipsle`. Its rule 4 lets the app decide only after `NSMetadataQuery` has given its first full result, and its consequences expect the Mac build to need the iCloud entitlement and a provisioning profile.

The spike of 2026-11-26 (`docs/spikes/icloud-container.md`) found three things those rules did not foresee:

1. **Not downloaded is dataless.** In the container on iOS 27, a file that is not downloaded keeps its name and size and carries the flag `SF_DATALESS`; there is no `.name.icloud` placeholder. The core knows only those placeholders (`layout.rs`, `index_sync.rs`, `watch.rs`, `strays.rs`), so it takes a dataless file for a downloaded one. A coordinated read of it then waits for the download, or fails offline.
2. **The query answers offline.** In flight mode, `NSMetadataQuery` finished gathering as fast as online (56 ms) and listed what the device already knew. Its first result does not prove that iCloud was reached. A device that has never synced the container would, offline, most likely get an empty result that looks like an empty folder; rule 4 would then make a second vault.
3. **The Mac reaches the container by its path.** The Mac app is not sandboxed. Without the entitlement it got no container from `url(forUbiquityContainerIdentifier:)`, but it listed, read and wrote `~/Library/Mobile Documents/iCloud~cc~kraml~schnipsle/Documents` with file coordination, with no prompt, and the iPhone got its note. That folder exists once any device of the account has used the container. The build was signed for development; the Developer ID build (ADR-0036) has not been tried.

Also found: deleting the app on the iPhone keeps the container's files on the device, so a reinstall finds the vault at once; and with iCloud Drive off for the app, `ubiquityIdentityToken` is `nil`.

## Decision

**A file is not downloaded if it is a placeholder or dataless. The app takes an empty iCloud folder as empty only when it knows the folder has been synced. The Mac uses the container by its path, without the entitlement, as long as the Developer ID build allows it.**

1. **Dataless files are not downloaded.** On macOS and iOS, a file whose flags include `SF_DATALESS` counts everywhere as a `.name.icloud` placeholder does today: listed with its name, not read until downloaded, its download requested when it is opened (ADR-0046), and `.notesapp/vault.yaml` that is dataless counts as a vault (ADR-0048 rule 3). The flag comes with the listing: `BulkLister` asks for `ATTR_CMN_FLAGS` (ADR-0047), `PlainLister` reads `st_flags` on Apple platforms; elsewhere no file is dataless.
2. **iCloud Drive on or off** (amends ADR-0048 rule 2). On the iPhone, `ubiquityIdentityToken` decides: `nil` means the local fallback at once, without asking for the container or waiting.
3. **When an empty result counts** (amends ADR-0048 rule 4). The iPhone decides from the query's first full result when that result holds the vault folder (any download state). When the result holds no vault folder, the app takes it as true only if one of these holds:
   - the device is online (the network path is satisfied) and a further 5 seconds of query updates bring no item into the vault folder; or
   - the app has seen this container synced before on this device (it remembers that once a query returned an uploaded item there).

   Otherwise it shows the choices of rule 4 ("Use a folder on this device", "Choose a folder…") and keeps the query running; if the vault appears meanwhile, it offers to open it. The 5 seconds are a first value, to be measured with a second device that has never seen the container.
4. **The Mac by path** (amends ADR-0048 rule 2 and its consequences). The Mac app uses `~/Library/Mobile Documents/iCloud~cc~kraml~schnipsle/Documents` by its path and has no iCloud entitlement or provisioning profile. It decides from the listing of that folder (with rule 1), as the Mac's iCloud Drive keeps the folder's entries on disk:
   - the folder holds a vault: it opens it (ADR-0048 rule 3);
   - the folder is there without a vault: it makes one, as ADR-0048 rule 3 says;
   - the folder is not there (no device has used the container yet, or iCloud Drive is off on the Mac): the local fallback `~/Documents/Schnipsle`, and the app says that the iPhone app will not see this vault until it is moved.
5. **If Developer ID does not allow it.** If the Developer ID build cannot use the folder by its path (checked when the certificate is there, M7 packaging), the Mac takes ADR-0048's way: the entitlement and a profile, and the query as on the iPhone. Rules 1 to 3 stay.

No change to the vault format.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Dataless files as placeholders, flag from the listing (chosen) | One rule for both kinds; no extra call per file | `ATTR_CMN_FLAGS` added to the bulk listing and its tests |
| Ask each file with `URLResourceValues` (`ubiquitousItemDownloadingStatus`) | The status iCloud itself reports | A call per file, which ADR-0047 removed |
| Read the file and let the coordinator download it | Nothing to change | Blocks the list on downloads; fails offline |
| Empty only when online plus a wait, or synced before (chosen) | No second vault from an offline first start; a device that knows the container decides at once | A wait of a few seconds on a new device; a value to measure |
| The query's first result as ADR-0048 says | Fast | Offline it makes a second vault |
| Never make a vault in an empty iCloud folder without asking | Safe | One more question at every new install, the case the default vault is for |
| Mac by path, no entitlement (chosen) | No profile in the release job; the notarised build stays as it is (ADR-0036) | Relies on a path and on the app not being sandboxed; a Mac that starts first cannot make the container |
| Mac with entitlement and profile (ADR-0048) | Same as the iPhone; the Mac can start first | A provisioning profile for Developer ID, in the release job as well |

## Consequences

- Positive: the Mac release build needs no change to its signing. A first start offline on a new device cannot make a second vault. Files not downloaded are handled the same in a picked folder and in the container.
- Negative: a Mac that starts before any iPhone makes a local vault, which the user moves later if they want it on the iPhone. A new iPhone waits up to about 5 seconds online before it makes a vault. The Mac has no query: a container folder whose entries are not on disk yet looks empty to it; as the folder exists only once an iPhone has used the container, and that iPhone has then made its vault, this is rare, and a second vault shows up as `Schnipsle 1`, not as a merge. The Mac rule depends on the Developer ID check.
- Rule 1 implemented on 2026-11-26: `FolderEntry::dataless` from `ATTR_CMN_FLAGS` (`crates/apple/src/listing.rs`) and `st_flags` (`crates/core/src/listing.rs`); used by the list of notes, the index sync (a dataless note stays indexed while its time and size match), the search for copies, reading and "Use other version", the vault check and the stray notes. The Mac now asks for a download as the iPhone does, with `startDownloadingUbiquitousItemAtURL`, which works without the entitlement; `evictUbiquitousItemAtURL` does not (Cocoa error 512), so the test that evicts a real file in iCloud Drive uses `brctl evict` (`crates/apple/src/download.rs`, run by hand with `--ignored`).
- Follow-up work:
  - The first start (ADR-0048 follow-up) with rules 2 to 4.
  - Measure the wait of rule 3 with a second device; check rule 4 with the Developer ID build.
