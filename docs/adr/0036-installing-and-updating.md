# ADR-0036: Installing and updating

- Status: Proposed
- Date: 2026-11-13
- Deciders: Willi Kraml
- Related: N-02, N-05, N-07, ADR-0003, ADR-0005, ADR-0010, ADR-0019, ADR-0022, ADR-0025

## Context

1.0 is for people to install and trust on the Mac and the iPhone (roadmap, M7). An iPhone app can only be installed from the App Store, or from TestFlight while in beta. A Mac app has two ways:

- **Developer ID:** signed with the developer's certificate, notarized by Apple, and handed out as a download.
- **The Mac App Store:** the app must run in the App Sandbox.

The sandbox conflicts with two desktop features:

- DOCX and EPUB call Pandoc as an outside program the user installed (ADR-0010, ADR-0025). A sandboxed app may only run programs inside its own bundle, and Pandoc may not be bundled (ADR-0005).
- The Apple Notes import drives Notes with Apple Events (ADR-0019). The sandbox allows that only with a temporary-exception entitlement, which App Review weighs case by case.

PDF through the print path (ADR-0022) has also not been checked under a sandbox.

A download from GitHub has no store to bring updates. The app works offline and needs no account (N-02), so an update check must not get in the way of that.

## Decision

1. **The Mac:** Developer ID only.
   - A DMG, signed with the Developer ID certificate, with the hardened runtime and the Apple Events entitlement for the Notes import.
   - Notarized and stapled by a release job in CI.
   - Attached to the GitHub release.
   - No Mac App Store build in 1.0.
2. **The iPhone and iPad:** the App Store, with TestFlight for the beta.
3. **Updates on the Mac:** Tauri's updater plugin (`tauri-plugin-updater`).
   - Each release has an update manifest (`latest.json`) next to the DMG on GitHub. The update archive is signed with the project's update key, besides the Apple signature. The public key is in the app.
   - The app checks at most once a day, after start, in the background.
   - When a newer version exists, the app says so and shows its notes. It installs only when the user agrees, and it restarts only when the user agrees.
   - The check fails quietly when the Mac is offline or GitHub cannot be reached.
   - The check is the app's only request to the internet. It sends nothing but what any download sends: the version and the platform in the URL, and the IP address. It can be turned off in the settings view, and the manual says it exists.
   - The private update key is kept as a CI secret and by the owner, never in the repository.
4. **No updater on iOS:** the App Store updates the app there.

## Options considered

| Option | Pros | Cons |
| --- | --- | --- |
| Developer ID only (chosen) | Pandoc export and the Notes import work as they do now; no App Review for the Mac; releases as often as wanted | People must trust a download; updates are the app's job |
| Mac App Store only | Found and updated through the store; Apple's review as a sign of trust | Sandbox: DOCX/EPUB and the Notes import would be reworked or dropped; PDF to check again; every release reviewed |
| Both | Each audience gets its way | Two builds to sign, test and explain; the store build has fewer features |
| Updater: Tauri's plugin (chosen) | Part of Tauri; signed updates; no server of our own (GitHub hosts the files) | A request to the internet; a key to look after; a manifest per release |
| Updater: none | No network at all; nothing to look after | People miss fixes, data-safety fixes included |
| Updater: Sparkle | Mature, the usual way on the Mac | Not part of Tauri; a framework and a bridge to add |

## Consequences

- Positive:
  - Every desktop feature stays as it is.
  - Fixes reach Mac users without a store.
  - The release is one CI job: build, sign, notarize, staple, sign the update, upload.
- Negative:
  - An Apple Developer Program membership is needed for both platforms; it is needed for the iPhone anyway.
  - A network request in an app that otherwise makes none, which must be said and can be turned off.
  - The update key cannot be replaced without one more update signed by the old key; losing it strands installed copies.
- Follow-up work:
  - Release job in CI with the Apple certificate, the notarization credentials and the update key as secrets.
  - The settings view gets "Check for updates" (on by default) and a "Check now" menu item.
  - `tauri-plugin-updater` and its dependencies pass cargo-deny and the npm licence check.
  - A Mac App Store build can be weighed after 1.0; ADR-0022's sandbox check goes with it.
  - Reproducible builds (N-07): what the signatures change is written down in `docs/dev-setup.md`.
