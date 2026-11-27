// SPDX-License-Identifier: Apache-2.0
//! Asking iCloud to download a file that is not on this device (ADR-0050).
//!
//! A note iCloud has not downloaded is a dataless file or a `.Name.icloud`
//! placeholder; the core lists it but does not read it, since a read would
//! wait for the download. [`download`] asks for it without waiting; the
//! file presenter or the watcher reports it once it is there.

use std::io;
use std::path::Path;

use objc2_foundation::NSFileManager;

use crate::coordinator::{file_url, to_io_error};

/// Asks iCloud to download the file at `path`, by its real name, and returns
/// at once. A file already downloaded, or not in iCloud, is left as it is.
///
/// # Errors
/// If the system refuses, e.g. because `path` is not in iCloud Drive.
pub fn download(path: &Path) -> io::Result<()> {
    let url = file_url(path)?;
    NSFileManager::defaultManager()
        .startDownloadingUbiquitousItemAtURL_error(&url)
        .map_err(|e| to_io_error(&e))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{Duration, Instant};

    use std::sync::Arc;

    use schnipsle_core::listing::is_dataless;
    use schnipsle_core::{FolderLister, Index, PlainLister, Vault, VaultError};

    use crate::BulkLister;

    /// Removes the local copy of `path`, which stays in iCloud. With
    /// `brctl`: `evictUbiquitousItemAtURL` needs the iCloud entitlement.
    fn evict(path: &Path) -> io::Result<()> {
        let out = std::process::Command::new("brctl")
            .arg("evict")
            .arg(path)
            .output()?;
        if out.status.success() {
            Ok(())
        } else {
            Err(io::Error::other(
                String::from_utf8_lossy(&out.stderr).into_owned(),
            ))
        }
    }

    fn dataless(path: &Path) -> bool {
        fs::symlink_metadata(path).is_ok_and(|m| is_dataless(&m))
    }

    /// Calls `done` until it gives `Ok(true)`; panics after three minutes
    /// with the last error.
    fn wait_for(what: &str, done: impl Fn() -> io::Result<bool>) {
        let start = Instant::now();
        loop {
            let last = done();
            if matches!(last, Ok(true)) {
                return;
            }
            assert!(
                start.elapsed() < Duration::from_secs(180),
                "timed out: {what}: {last:?}"
            );
            std::thread::sleep(Duration::from_millis(500));
        }
    }

    #[test]
    fn a_file_outside_icloud_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("a.md");
        fs::write(&file, "x").unwrap();
        assert!(download(&file).is_err());
    }

    /// Needs iCloud Drive and a folder in it to make a test vault in, in
    /// `SCHNIPSLE_ICLOUD_TEST_DIR`; the vault is removed afterwards. Run with
    /// `cargo test -p schnipsle-apple -- --ignored evicted`.
    #[test]
    #[ignore = "needs iCloud Drive"]
    fn an_evicted_note_is_listed_but_not_read_until_downloaded() {
        let dir = std::env::var_os("SCHNIPSLE_ICLOUD_TEST_DIR")
            .expect("SCHNIPSLE_ICLOUD_TEST_DIR names a folder in iCloud Drive");
        let root = Path::new(&dir).join("dataless-test");
        fs::create_dir_all(&root).unwrap();
        let cleanup = scopeguard(&root);
        let vault = Vault::create(&root)
            .unwrap()
            .with_folder_lister(Arc::new(BulkLister));
        let saved = vault
            .create_note("Evicted.md", "# Evicted\n\nInvented test text.\n")
            .unwrap();
        let file = vault.notes_dir().join("Evicted.md");
        let mut index = Index::in_memory().unwrap();
        assert_eq!(vault.sync_index(&mut index).unwrap().indexed, 1);

        // iCloud evicts a file only once it is uploaded.
        wait_for("eviction", || evict(&file).map(|()| dataless(&file)));
        let size = u64::try_from(saved.text.len()).unwrap();
        for lister in [&BulkLister as &dyn FolderLister, &PlainLister] {
            let entries = lister.list(vault.notes_dir()).unwrap();
            let entry = entries.iter().find(|e| e.name == "Evicted.md").unwrap();
            assert!(entry.dataless, "{lister:?}");
            assert_eq!(
                entry.stat.unwrap().size,
                size,
                "a dataless file keeps its size"
            );
        }
        let listed = vault.list_notes().unwrap();
        assert_eq!(listed.len(), 1);
        assert!(!listed[0].available);
        assert!(matches!(
            vault.read_note_text("Evicted.md"),
            Err(VaultError::NotDownloaded(_))
        ));
        // Same time and size: it stays in the index, unread.
        let sync = vault.sync_index(&mut index).unwrap();
        assert_eq!((sync.indexed, sync.removed, sync.failed), (0, 0, 0));
        vault.index_file(&mut index, "notes/Evicted.md").unwrap();
        assert!(index.stamp("notes/Evicted.md").unwrap().is_some());
        assert!(dataless(&file), "nothing above read it");

        download(&file).unwrap();
        wait_for("download", || Ok(!dataless(&file)));
        assert_eq!(vault.read_note_text("Evicted.md").unwrap().text, saved.text);
        assert!(vault.list_notes().unwrap()[0].available);
        drop(cleanup);
    }

    /// Removes `dir` when dropped, also when the test fails.
    fn scopeguard(dir: &Path) -> impl Drop + '_ {
        struct Remove<'a>(&'a Path);
        impl Drop for Remove<'_> {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(self.0);
            }
        }
        Remove(dir)
    }
}
