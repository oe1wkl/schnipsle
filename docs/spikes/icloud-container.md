# Spike: the app's iCloud folder for the default vault

- Branch: `spike/icloud-container`
- Started: 2026-11-25
- Decides: where ADR-0048 puts the default vault, and what the Mac build needs for it (M7, default vault)
- Status: **done** (2026-11-26; Developer ID still open)

## Question

ADR-0048 puts the default vault into the `Documents` folder of the app's iCloud container, so the Mac and the iPhone share it without the user picking folders. Before building it, five things must be known:

1. **iPhone, the folder.** With the iCloud Documents entitlement and `NSUbiquitousContainers` in `Info.plist`, does `FileManager.url(forUbiquityContainerIdentifier:)` give the container, does iCloud Drive (Files app, and Finder on the Mac) show its `Documents` as "Schnipsle" with the app's icon, and do the vault's reads and coordinated writes work there as in a picked folder?
2. **iPhone, a second device.** On a device that has not downloaded the vault yet, does `NSMetadataQuery` (`NSMetadataQueryUbiquitousDocumentsScope`) list `.notesapp/vault.yaml` and the notes before they are downloaded, how long do its first results take, and what does it give offline or with iCloud Drive turned off?
3. **Mac without the entitlement.** Once the iPhone has used the container, can the Developer ID app, which is not sandboxed, read and write `~/Library/Mobile Documents/iCloud~cc~kraml~schnipsle/Documents` by its path, with file coordination as for any iCloud Drive folder, and does a listing there show files not downloaded yet? If so, the Mac needs no entitlement as long as an iPhone made the container first.
4. **Mac with the entitlement.** If the Mac starts first, or rule 3 does not work: does a Mac build with the entitlement and a provisioning profile get the container from `url(forUbiquityContainerIdentifier:)`, also for Developer ID (ADR-0036)?
5. **The fallbacks.** Signed out of iCloud or iCloud Drive off: `ubiquityIdentityToken` and the container are `nil`, and the app's local `Documents` folder (iPhone: "On My iPhone" in Files, which needs `UIFileSharingEnabled` and `LSSupportsOpeningDocumentsInPlace`) is used instead.

## Prerequisite

The container `iCloud.cc.kraml.schnipsle` has to be registered for the App ID `cc.kraml.schnipsle` in the Apple Developer account, and the iCloud capability enabled. **A container identifier cannot be deleted or renamed afterwards**, so the name is chosen once. Xcode does both: target `schnipsle_iOS` › Signing & Capabilities › + Capability › iCloud › iCloud Documents › + container.

## Plan

- **Probe, iPhone:** a command `icloud_probe` in `tauri-plugin-vault-access` (Swift), run from Settings › Diagnostics on this branch only. It reports, with times: the identity token, the container URL (asked off the main thread), what is in `Documents`, and the `NSMetadataQuery` results with their download state. On request it makes a test folder `Schnipsle-Spike` with one note in `Documents`, written as the vault writes, and removes it again.
- **Probe, Mac:** the same report from `schnipsle-apple` (objc2): the container URL without the entitlement (expected `nil`), and the folder `~/Library/Mobile Documents/iCloud~cc~kraml~schnipsle/Documents` by its path: there or not, listing, a coordinated write and read of a test note.
- **Runs:**
  1. iPhone: probe, then make the test folder; look at Files and at Finder on the Mac.
  2. Mac: probe by path (question 3).
  3. iPhone: delete the app and install it again, so nothing is downloaded, then probe at once (question 2); again in flight mode.
  4. iPhone: iCloud Drive off for the app in Settings, probe (question 5).
  5. Mac with the entitlement (question 4): only if run 2 fails, with a Mac development profile; Developer ID waits for the certificate (M7 packaging).
- The probes are removed after the spike; the code is in commit `0640069` (`feat(ios): spike the app's iCloud container for the default vault`). The entitlements and the `Info.ios.plist` keys stay.

## Results

The container `iCloud.cc.kraml.schnipsle` was registered on 2026-11-26 in Xcode (iCloud › iCloud Documents; this Xcode calls the list "iCloud Identifiers"). Xcode wrote the three keys into `schnipsle_iOS.entitlements` and made a new team profile with them; the project file did not change.

### Run 1: iPhone 18 Pro, iOS 27.2.1, 2026-11-26

After Make test vault, then Probe:

| Step                                                          | Result                                                                                                            |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------------- |
| `ubiquityIdentityToken`                                       | set                                                                                                               |
| `url(forUbiquityContainerIdentifier:)`                        | 0.8 ms, `/private/var/mobile/Library/Mobile Documents/iCloud~cc~kraml~schnipsle`                                  |
| `Documents`                                                   | made by iOS, there at once                                                                                        |
| Test vault                                                    | `Vault::create_with` and `create_note` with the file coordinator worked as in a picked folder; the note read back |
| `NSMetadataQuery` (`NSMetadataQueryUbiquitousDocumentsScope`) | first full result after 50 ms, all 5 items (folders included), state `Current`, uploaded                          |

Files on the iPhone and Finder on the Mac show the container's `Documents` in iCloud Drive as "Schnipsle" with the app's icon. The Mac had the files about a minute later: `~/Library/Mobile Documents/iCloud~cc~kraml~schnipsle/Documents/Schnipsle-Spike/` with the note downloaded (not dataless), seen from a shell.

### Run 2: Mac, macOS 27.2.1, 2026-11-26

The release build (`pnpm tauri build --bundles app`), not sandboxed, signed with the Apple Development certificate, without the iCloud entitlement:

| Step                                                                         | Result                                                                      |
| ---------------------------------------------------------------------------- | --------------------------------------------------------------------------- |
| `ubiquityIdentityToken`                                                      | set                                                                         |
| `url(forUbiquityContainerIdentifier:)`                                       | `nil`, as expected without the entitlement                                  |
| `~/Library/Mobile Documents/iCloud~cc~kraml~schnipsle/Documents` by its path | there, listed in under 1 ms with the iPhone's vault and note                |
| Make test vault                                                              | opened the iPhone's vault and wrote a second note with the file coordinator |
| Prompts                                                                      | none from macOS                                                             |
| Back on the iPhone                                                           | the Mac's note arrived and is listed next to the first                      |

So a Mac app that is not sandboxed can use the container by its path once a device with the entitlement has made it (question 3). Still open: the same with the Developer ID build (ADR-0036), and a Mac that starts before any iPhone has made the container.

### Run 3: iPhone, the app deleted and installed again, 2026-11-26

Deleting the app (`devicectl device uninstall app`) does not remove its iCloud folder from the device: after the new install, `url(forUbiquityContainerIdentifier:)` gave the same folder in 1.0 ms, with the vault and both notes still downloaded, and `NSMetadataQuery` listed all 6 items as `Current` after 53 ms. A reinstall therefore finds the vault at once and is not a test of a second device; for that, the files have to be removed from the device first (Files › Remove Download, or a second device).

After Files › Remove Download on the "Schnipsle" folder, the app force-quit and opened again:

|                                              | Online                                                                                                                            | Flight mode             |
| -------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ----------------------- |
| Container from the system                    | 6.1 ms                                                                                                                            | 0.9 ms                  |
| Listing of `Documents` (`read_dir`, `lstat`) | all 6 entries with their real names and sizes; the files are _dataless_ (`SF_DATALESS`), there are no `.name.icloud` placeholders | the same                |
| `NSMetadataQuery`                            | 63 ms, 6 items; the files `NotDownloaded`, the folders `Current`, all uploaded                                                    | 56 ms, the same 6 items |

Findings:

- **Not downloaded means dataless here, not `.icloud`.** In the app's own container on iOS 27 a file not downloaded keeps its name and size and has the flag `SF_DATALESS`; the core knows only the older `.name.icloud` placeholders (`layout.rs`, `index_sync.rs`, `watch.rs`, `strays.rs`). `.notesapp/vault.yaml` is therefore seen as there, which is right for rule 3 of ADR-0048, but reading it or a note may block on a download or fail offline. The default vault needs the core to treat dataless files as not downloaded: the flag can come with the bulk listing (`ATTR_CMN_FLAGS`, ADR-0047) and from `st_flags` in `PlainLister`.
- **The query answers offline from what the device already knows.** In flight mode `NSMetadataQuery` finished gathering as fast as online and listed every item. Its first result therefore does not prove that iCloud was reached: a device that has never synced the container would, offline, most likely get an empty result that looks like an empty folder. Rule 4 of ADR-0048 has to check that the device is online (or that the container has been synced once) before it takes an empty result as empty.

### Run 4: iPhone, iCloud Drive off for Schnipsle, 2026-11-26

With Schnipsle switched off under the apps using iCloud Drive: `ubiquityIdentityToken` is `nil` and `url(forUbiquityContainerIdentifier:)` gives `nil` (4.2 ms). The app's local `Documents` is there, so the fallback of ADR-0048 rule 2 has a folder to use. The token alone tells the app whether to use iCloud; it need not ask for the container to know.

## Conclusion

- **iPhone:** the container works as ADR-0048 assumes. It is shown as "Schnipsle" with the app's icon in Files and in Finder, the vault is read and written there as in a picked folder, and `NSMetadataQuery` lists it, also before it is downloaded, in about 60 ms.
- **Mac:** a Mac app that is not sandboxed reads and writes the container by its path without the entitlement, with no prompt (run 2, Apple Development signature). The Mac build can therefore do without the entitlement and its provisioning profile, provided the Developer ID build behaves the same; this is checked when the Developer ID certificate is there (M7 packaging). A Mac that starts before any iPhone has made the container cannot make it itself; it then uses its local fallback (`~/Documents`) or waits for the folder to appear (to decide when the default vault is built).
- **Changes for the implementation:**
  - The core treats dataless files (`SF_DATALESS`) as not downloaded, like `.icloud` placeholders (run 3).
  - Rule 4 of ADR-0048 needs more than the query's first result: offline, the query answers from what the device knows (run 3).
  - `ubiquityIdentityToken` decides between iCloud and the local fallback (run 4).
- **Not tested:** a second device that has never seen the container (no second iPhone here); the Developer ID build; iCloud Drive turned off on the Mac.
