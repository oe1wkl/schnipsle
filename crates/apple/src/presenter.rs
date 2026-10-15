// SPDX-License-Identifier: Apache-2.0
//! `NSFilePresenter` on the vault folder: reports changes that iCloud Drive and
//! other coordinating apps make to notes (ADR-0015).

use std::io;
use std::path::Path;
use std::ptr;
use std::sync::mpsc::Sender;

use block2::DynBlock;
use objc2::rc::Retained;
use objc2::runtime::{NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AnyThread, DefinedClass, define_class, msg_send};
use objc2_foundation::{
    NSError, NSFileCoordinator, NSFilePresenter, NSOperationQueue, NSString, NSURL,
};
use schnipsle_core::Hint;

struct Ivars {
    folder: Retained<NSURL>,
    queue: Retained<NSOperationQueue>,
    hints: Sender<Hint>,
}

impl Ivars {
    fn report(&self, url: &NSURL) {
        let hint = url.to_file_path().map_or(Hint::Rescan, Hint::Path);
        // A send fails only once the dispatcher has stopped; nothing to do then.
        let _ = self.hints.send(hint);
    }

    fn rescan(&self) {
        let _ = self.hints.send(Hint::Rescan);
    }
}

define_class!(
    // SAFETY:
    // - NSObject has no subclassing requirements.
    // - `Presenter` does not implement `Drop`.
    #[unsafe(super(NSObject))]
    #[name = "SchnipsleVaultPresenter"]
    #[ivars = Ivars]
    struct Presenter;

    unsafe impl NSObjectProtocol for Presenter {}

    // Callbacks arrive on `queue`, one at a time; they only send hints and
    // never block, so coordinated writers elsewhere are not held up.
    unsafe impl NSFilePresenter for Presenter {
        #[unsafe(method_id(presentedItemURL))]
        fn presented_item_url(&self) -> Option<Retained<NSURL>> {
            Some(self.ivars().folder.clone())
        }

        #[unsafe(method_id(presentedItemOperationQueue))]
        fn presented_item_operation_queue(&self) -> Retained<NSOperationQueue> {
            self.ivars().queue.clone()
        }

        #[unsafe(method(presentedSubitemDidChangeAtURL:))]
        fn subitem_did_change(&self, url: &NSURL) {
            self.ivars().report(url);
        }

        #[unsafe(method(presentedSubitemDidAppearAtURL:))]
        fn subitem_did_appear(&self, url: &NSURL) {
            self.ivars().report(url);
        }

        #[unsafe(method(presentedSubitemAtURL:didMoveToURL:))]
        fn subitem_did_move(&self, old_url: &NSURL, new_url: &NSURL) {
            self.ivars().report(old_url);
            self.ivars().report(new_url);
        }

        #[unsafe(method(accommodatePresentedSubitemDeletionAtURL:completionHandler:))]
        fn accommodate_subitem_deletion(&self, url: &NSURL, done: &DynBlock<dyn Fn(*mut NSError)>) {
            self.ivars().report(url);
            done.call((ptr::null_mut(),));
        }

        #[unsafe(method(presentedItemDidChange))]
        fn item_did_change(&self) {
            self.ivars().rescan();
        }

        #[unsafe(method(presentedItemDidMoveToURL:))]
        fn item_did_move(&self, _new_url: &NSURL) {
            self.ivars().rescan();
        }
    }
);

/// Presents the vault folder to the system while it lives: other coordinating
/// processes then tell us about changes to notes inside it. Unregisters when
/// dropped.
pub struct VaultPresenter {
    presenter: Retained<Presenter>,
}

impl std::fmt::Debug for VaultPresenter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultPresenter")
            .field("folder", &self.presenter.ivars().folder.to_file_path())
            .finish()
    }
}

impl VaultPresenter {
    /// Registers a presenter for the folder `root`; hints go to `hints`.
    ///
    /// # Errors
    /// If `root` cannot be expressed as a file URL.
    pub fn start(root: &Path, hints: Sender<Hint>) -> io::Result<Self> {
        let folder = NSURL::from_directory_path(root).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("not a usable folder path: {}", root.display()),
            )
        })?;
        let queue = NSOperationQueue::new();
        queue.setMaxConcurrentOperationCount(1);
        queue.setName(Some(&NSString::from_str("schnipsle.vault-presenter")));

        let presenter = Presenter::alloc().set_ivars(Ivars {
            folder,
            queue,
            hints,
        });
        // SAFETY: `presenter` is a freshly allocated `Presenter` with its ivars
        // set; NSObject's `init` takes no arguments and returns the same object.
        let presenter: Retained<Presenter> = unsafe { msg_send![super(presenter), init] };
        NSFileCoordinator::addFilePresenter(ProtocolObject::from_ref(&*presenter));
        Ok(Self { presenter })
    }
}

impl Drop for VaultPresenter {
    fn drop(&mut self) {
        NSFileCoordinator::removeFilePresenter(ProtocolObject::from_ref(&*self.presenter));
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::sync::Arc;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    use schnipsle_core::{Vault, watch};

    use super::*;
    use crate::FileCoordinator;

    /// Waits until a hint for `note` arrives (or fails after 10 s).
    fn wait_for(vault: &Vault, rx: &mpsc::Receiver<Hint>, note: &str) {
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let hint = rx
                .recv_timeout(left)
                .expect("no presenter callback in time");
            if watch::collect(vault, [hint])
                .paths
                .iter()
                .any(|p| p == note)
            {
                return;
            }
        }
    }

    #[test]
    fn reports_coordinated_writes_by_others() {
        let dir = tempfile::tempdir().unwrap();
        // "Another app": its writes are coordinated and must reach our presenter.
        let other = Vault::open_with(dir.path(), Arc::new(FileCoordinator)).unwrap();
        let (tx, rx) = mpsc::channel();
        let _presenter = VaultPresenter::start(dir.path(), tx).unwrap();

        let hash = other.create_note("from-icloud.md", "one").unwrap();
        wait_for(&other, &rx, "from-icloud.md");
        other.write_note("from-icloud.md", "two", &hash).unwrap();
        wait_for(&other, &rx, "from-icloud.md");
    }

    #[test]
    fn stops_reporting_after_drop() {
        let dir = tempfile::tempdir().unwrap();
        let other = Vault::open_with(dir.path(), Arc::new(FileCoordinator)).unwrap();
        let (tx, rx) = mpsc::channel();
        drop(VaultPresenter::start(dir.path(), tx).unwrap());
        other.create_note("later.md", "x").unwrap();
        assert!(matches!(
            rx.recv_timeout(Duration::from_millis(500)),
            Err(mpsc::RecvTimeoutError::Disconnected | mpsc::RecvTimeoutError::Timeout)
        ));
        fs::remove_file(dir.path().join("later.md")).unwrap();
    }
}
