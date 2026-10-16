// SPDX-License-Identifier: Apache-2.0
//! iCloud conflict versions (`NSFileVersion`) behind the core's
//! [`SyncConflicts`] trait (ADR-0006).

use std::fs;
use std::io;
use std::path::Path;

use objc2_foundation::{
    NSArray, NSFileVersion, NSNumber, NSString, NSURL, NSURLIsUbiquitousItemKey,
    NSURLUbiquitousItemDownloadingStatusKey, NSURLUbiquitousItemHasUnresolvedConflictsKey,
};
use schnipsle_core::{RawVersion, SyncConflicts};

use crate::coordinator::file_url;

/// Lists and resolves the conflict versions iCloud Drive keeps for a note.
///
/// Resolved versions are only marked resolved; the system deletes them later.
/// `removeOtherVersionsOfItemAtURL:` is never called, because it would also
/// delete a conflict version that arrived after the user looked.
#[derive(Debug, Default, Clone, Copy)]
pub struct ICloudVersions;

impl SyncConflicts for ICloudVersions {
    fn versions(&self, file: &Path) -> io::Result<Vec<RawVersion>> {
        let url = file_url(file)?;
        let versions = NSFileVersion::unresolvedConflictVersionsOfItemAtURL(&url);
        let listed = versions.as_ref().map_or(0, |v| v.count());
        let flags = ICloudFlags::of(&url);
        if cfg!(debug_assertions) && flags.ubiquitous == Some(true) {
            eprintln!(
                "[schnipsle] read {}: downloadingStatus={}, hasUnresolvedConflicts={:?}, conflict versions listed={listed}",
                file.display(),
                flags.downloading_status.as_deref().unwrap_or("?"),
                flags.unresolved_conflicts,
            );
        }
        // Never stay silent about a conflict we cannot show (spike test C7).
        if flags.unresolved_conflicts == Some(true) && listed == 0 {
            return Err(io::Error::other(
                "iCloud reports a conflict for this note but does not list its versions",
            ));
        }
        let Some(versions) = versions else {
            return Ok(Vec::new());
        };
        versions
            .iter()
            .map(|version| {
                Ok(RawVersion {
                    bytes: read(&version)?,
                    modified_ms: version
                        .modificationDate()
                        .map_or(0, |date| seconds_to_ms(date.timeIntervalSince1970())),
                    device: version
                        .localizedNameOfSavingComputer()
                        .map(|name| name.to_string()),
                })
            })
            .collect()
    }

    fn mark_resolved(
        &self,
        file: &Path,
        resolve: &mut dyn FnMut(&[u8]) -> bool,
    ) -> io::Result<usize> {
        let url = file_url(file)?;
        let Some(versions) = NSFileVersion::unresolvedConflictVersionsOfItemAtURL(&url) else {
            return Ok(0);
        };
        let mut resolved = 0;
        for version in &versions {
            if resolve(&read(&version)?) {
                version.setResolved(true);
                resolved += 1;
            }
        }
        Ok(resolved)
    }
}

/// What iCloud says about a file; `None` where it does not say.
struct ICloudFlags {
    ubiquitous: Option<bool>,
    unresolved_conflicts: Option<bool>,
    /// `…Current` when this device has the newest version, `…Downloaded` when a
    /// newer one exists in iCloud, `…NotDownloaded` for a placeholder.
    downloading_status: Option<String>,
}

impl ICloudFlags {
    fn of(url: &NSURL) -> Self {
        // SAFETY: a constant exported by Foundation, valid for the whole process.
        let ubiquitous_key = unsafe { NSURLIsUbiquitousItemKey };
        // SAFETY: as above.
        let conflicts_key = unsafe { NSURLUbiquitousItemHasUnresolvedConflictsKey };
        // SAFETY: as above.
        let status_key = unsafe { NSURLUbiquitousItemDownloadingStatusKey };
        let keys = NSArray::from_slice(&[ubiquitous_key, conflicts_key, status_key]);
        let Ok(values) = url.resourceValuesForKeys_error(&keys) else {
            return Self {
                ubiquitous: None,
                unresolved_conflicts: None,
                downloading_status: None,
            };
        };
        let flag = |key| {
            values
                .objectForKey(key)
                .and_then(|value| value.downcast::<NSNumber>().ok())
                .map(|number| number.boolValue())
        };
        Self {
            ubiquitous: flag(ubiquitous_key),
            unresolved_conflicts: flag(conflicts_key),
            downloading_status: values
                .objectForKey(status_key)
                .and_then(|value| value.downcast::<NSString>().ok())
                .map(|status| status.to_string()),
        }
    }
}

/// The content of a version, read from the system's version store.
fn read(version: &NSFileVersion) -> io::Result<Vec<u8>> {
    let path = version
        .URL()
        .to_file_path()
        .ok_or_else(|| io::Error::other("conflict version has no file path"))?;
    fs::read(path)
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "checked to be finite and positive; dates fit in u64 milliseconds"
)]
fn seconds_to_ms(seconds: f64) -> u64 {
    if seconds.is_finite() && seconds > 0.0 {
        (seconds * 1000.0).round() as u64
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_local_file_has_no_conflict_versions() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("n.md");
        fs::write(&file, "x").unwrap();
        assert!(ICloudVersions.versions(&file).unwrap().is_empty());
        assert_eq!(
            ICloudVersions.mark_resolved(&file, &mut |_| true).unwrap(),
            0
        );
    }

    #[test]
    fn converts_dates() {
        assert_eq!(seconds_to_ms(1_792_099_714.5), 1_792_099_714_500);
        assert_eq!(seconds_to_ms(-1.0), 0);
        assert_eq!(seconds_to_ms(f64::NAN), 0);
    }
}
