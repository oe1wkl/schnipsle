// SPDX-License-Identifier: Apache-2.0
//! `NSFileCoordinator` behind the core's [`Coordinator`] trait.

use std::cell::{Cell, RefCell};
use std::io;
use std::path::Path;
use std::ptr::NonNull;

use block2::StackBlock;
use objc2::rc::Retained;
use objc2_foundation::{
    NSError, NSFileCoordinator, NSFileCoordinatorReadingOptions, NSFileCoordinatorWritingOptions,
    NSURL,
};
use schnipsle_core::Coordinator;

/// Coordinates vault access with `NSFileCoordinator`.
///
/// Each call creates a fresh `NSFileCoordinator`, as Apple recommends for
/// one-off operations, and blocks until access is granted. Call it from a
/// background thread only (ADR-0015).
#[derive(Debug, Default, Clone, Copy)]
pub struct FileCoordinator;

impl Coordinator for FileCoordinator {
    fn read(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        // Listing a folder must not start downloading a whole iCloud vault; it
        // only needs what is already on this device. Notes are read in full.
        let options = if path.is_dir() {
            NSFileCoordinatorReadingOptions::ImmediatelyAvailableMetadataOnly
        } else {
            NSFileCoordinatorReadingOptions::empty()
        };
        coordinate(path, f, Intent::Read(options))
    }

    fn write(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        coordinate(
            path,
            f,
            Intent::Write(NSFileCoordinatorWritingOptions::ForReplacing),
        )
    }

    fn delete(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        coordinate(
            path,
            f,
            Intent::Write(NSFileCoordinatorWritingOptions::ForDeleting),
        )
    }
}

#[derive(Clone, Copy)]
enum Intent {
    Read(NSFileCoordinatorReadingOptions),
    Write(NSFileCoordinatorWritingOptions),
}

fn coordinate(
    path: &Path,
    f: &mut dyn FnMut(&Path) -> io::Result<()>,
    intent: Intent,
) -> io::Result<()> {
    let url = file_url(path)?;
    let f = RefCell::new(f);
    let outcome: Cell<Option<io::Result<()>>> = Cell::new(None);

    // The accessor runs synchronously, on this thread, before the coordinate
    // call returns, so it may borrow from this stack frame. It must not panic:
    // it is called from Objective-C.
    let accessor = StackBlock::new(|granted: NonNull<NSURL>| {
        // SAFETY: NSFileCoordinator passes a valid, retained NSURL that stays
        // alive for the duration of the accessor call; we do not keep it.
        let granted = unsafe { granted.as_ref() };
        let result = match (granted.to_file_path(), f.try_borrow_mut()) {
            (Some(file), Ok(mut f)) => f(&file),
            (None, _) => Err(io::Error::other("coordinated URL is not a file path")),
            (_, Err(_)) => Err(io::Error::other(
                "file coordination re-entered the accessor",
            )),
        };
        outcome.set(Some(result));
    });

    let coordinator = NSFileCoordinator::new();
    let mut error: Option<Retained<NSError>> = None;
    match intent {
        Intent::Read(options) => coordinator.coordinateReadingItemAtURL_options_error_byAccessor(
            &url,
            options,
            Some(&mut error),
            &accessor,
        ),
        Intent::Write(options) => coordinator.coordinateWritingItemAtURL_options_error_byAccessor(
            &url,
            options,
            Some(&mut error),
            &accessor,
        ),
    }

    match (outcome.take(), error) {
        (Some(result), _) => result,
        (None, Some(error)) => Err(to_io_error(&error)),
        (None, None) => Err(io::Error::other(
            "file coordination returned without granting access",
        )),
    }
}

/// `path` as a file URL.
pub(crate) fn file_url(path: &Path) -> io::Result<Retained<NSURL>> {
    NSURL::from_file_path(path).ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("not a usable file path: {}", path.display()),
        )
    })
}

/// POSIX errors keep their error number, so `io::ErrorKind` stays meaningful.
fn to_io_error(error: &NSError) -> io::Error {
    let code = error.code();
    if error.domain().to_string() == "NSPOSIXErrorDomain"
        && let Ok(errno) = i32::try_from(code)
    {
        return io::Error::from_raw_os_error(errno);
    }
    io::Error::other(format!(
        "{} ({} {code})",
        error.localizedDescription(),
        error.domain()
    ))
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::thread;
    use std::time::{Duration, Instant};

    use schnipsle_core::{Vault, VaultError};

    use super::*;

    fn temp_vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::open_with(dir.path(), Arc::new(FileCoordinator)).unwrap();
        (dir, vault)
    }

    #[test]
    fn vault_roundtrip_through_nsfilecoordinator() {
        let (dir, vault) = temp_vault();
        let h1 = vault.create_note("a/n.md", "# One\n").unwrap().hash;
        let note = vault.read_note("a/n.md").unwrap();
        assert_eq!(
            (
                schnipsle_core::front_matter::body(&note.text),
                note.hash.as_str()
            ),
            ("# One\n", h1.as_str())
        );
        vault.write_note("a/n.md", "# Two\n", &h1).unwrap();
        let on_disk = fs::read_to_string(dir.path().join("a/n.md")).unwrap();
        assert_eq!(schnipsle_core::front_matter::body(&on_disk), "# Two\n");
        let titles: Vec<_> = vault
            .list_notes()
            .unwrap()
            .into_iter()
            .map(|n| n.title)
            .collect();
        assert_eq!(titles, ["Two"]);
    }

    #[test]
    fn conflict_is_still_detected() {
        let (dir, vault) = temp_vault();
        let h = vault.create_note("n.md", "one").unwrap().hash;
        fs::write(dir.path().join("n.md"), "changed elsewhere").unwrap();
        assert!(matches!(
            vault.write_note("n.md", "mine", &h),
            Err(VaultError::Conflict { .. })
        ));
    }

    #[test]
    fn accessor_errors_pass_through() {
        let dir = tempfile::tempdir().unwrap();
        let err = FileCoordinator
            .write(&dir.path().join("x.md"), &mut |_| {
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "nope"))
            })
            .unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn writers_wait_for_each_other() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("n.md");
        fs::write(&file, "x").unwrap();
        let (entered, wait_for_entry) = mpsc::channel();

        let holder = {
            let file = file.clone();
            thread::spawn(move || {
                let mut left = None;
                FileCoordinator
                    .write(&file, &mut |_| {
                        entered.send(()).unwrap();
                        thread::sleep(Duration::from_millis(300));
                        left = Some(Instant::now());
                        Ok(())
                    })
                    .unwrap();
                left.unwrap()
            })
        };
        wait_for_entry.recv().unwrap();
        let mut second_entered = None;
        FileCoordinator
            .write(&file, &mut |_| {
                second_entered = Some(Instant::now());
                Ok(())
            })
            .unwrap();
        let first_left = holder.join().unwrap();
        assert!(
            second_entered.unwrap() >= first_left,
            "the second writer must not run while the first holds access"
        );
    }
}
