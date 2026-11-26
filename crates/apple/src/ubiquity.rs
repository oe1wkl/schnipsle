// SPDX-License-Identifier: Apache-2.0
//! The app's iCloud container (docs/spikes/icloud-container.md, spike only).

use std::path::PathBuf;

use objc2_foundation::{NSFileManager, NSString};

/// Whether the device is signed in to iCloud with iCloud Drive on for this
/// app: `NSFileManager.ubiquityIdentityToken` is set.
#[must_use]
pub fn has_icloud_account() -> bool {
    NSFileManager::defaultManager()
        .ubiquityIdentityToken()
        .is_some()
}

/// The folder of the iCloud container `id`, `None` without iCloud Drive or
/// without the entitlement. Blocks while iCloud sets the container up: never
/// call it on the main thread.
#[must_use]
pub fn ubiquity_container(id: &str) -> Option<PathBuf> {
    let id = NSString::from_str(id);
    NSFileManager::defaultManager()
        .URLForUbiquityContainerIdentifier(Some(&id))
        .and_then(|url| url.to_file_path())
}
