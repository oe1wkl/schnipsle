// SPDX-License-Identifier: Apache-2.0
//! Change notification (ADR-0015).
//!
//! Platform sources (the `notify` watcher here on desktop, the `NSFilePresenter`
//! in `schnipsle-apple` on Apple platforms) send raw [`Hint`]s into a dispatcher
//! thread. It waits until a burst of events is over, maps the paths to notes
//! and reports one [`VaultChanges`] per burst.
//!
//! Events are hints only: they tell the app to re-read. Whether a save is safe
//! is decided by the hash check in [`Vault::write_note`].

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, RecvTimeoutError, Sender};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

use crate::vault::{PathKind, Vault};

/// A raw change report from a platform source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Hint {
    /// Something happened at this absolute path.
    Path(PathBuf),
    /// Events were lost or cannot be attributed: re-read everything.
    Rescan,
}

/// One burst of changes, as sent to the UI.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultChanges {
    /// Vault-relative paths of notes that may have changed, appeared or gone.
    pub paths: Vec<String>,
    /// `true` if notes not listed in `paths` may have changed too.
    pub rescan: bool,
    /// `true` if the smart folders (format.md §9) may have changed.
    pub smart_folders: bool,
}

impl VaultChanges {
    /// `true` if there is nothing to report.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty() && !self.rescan && !self.smart_folders
    }
}

/// Turns raw hints into note changes: drops hidden, temporary and non-note
/// files, removes duplicates and sorts the paths.
#[must_use]
pub fn collect(vault: &Vault, hints: impl IntoIterator<Item = Hint>) -> VaultChanges {
    let mut paths = BTreeSet::new();
    let mut rescan = false;
    let mut smart_folders = false;
    for hint in hints {
        match hint {
            Hint::Rescan => rescan = true,
            Hint::Path(path) => match vault.classify(&path) {
                PathKind::Note(note) => {
                    paths.insert(note);
                }
                PathKind::Folder => rescan = true,
                PathKind::SmartFolders => smart_folders = true,
                PathKind::Ignored => {}
            },
        }
    }
    VaultChanges {
        paths: paths.into_iter().collect(),
        rescan,
        smart_folders,
    }
}

/// A burst ends after `quiet` without events, or after this long at the latest,
/// so a file that changes continuously is still reported.
const MAX_DELAY: Duration = Duration::from_secs(1);

/// Starts the dispatcher thread and returns the sender for platform sources.
/// The thread ends when every clone of the sender is dropped.
///
/// # Errors
/// If the thread cannot be started.
pub fn spawn_dispatcher(
    vault: Vault,
    quiet: Duration,
    emit: impl FnMut(VaultChanges) + Send + 'static,
) -> std::io::Result<Sender<Hint>> {
    let (hints, rx) = mpsc::channel();
    thread::Builder::new()
        .name("vault-changes".into())
        .spawn(move || run_dispatcher(&vault, &rx, quiet, emit))?;
    Ok(hints)
}

/// The dispatcher loop; returns when all senders are gone.
pub fn run_dispatcher(
    vault: &Vault,
    rx: &Receiver<Hint>,
    quiet: Duration,
    mut emit: impl FnMut(VaultChanges),
) {
    while let Ok(first) = rx.recv() {
        let mut burst = vec![first];
        let deadline = Instant::now() + MAX_DELAY;
        let mut senders_gone = false;
        loop {
            let wait = quiet.min(deadline.saturating_duration_since(Instant::now()));
            match rx.recv_timeout(wait) {
                Ok(hint) => burst.push(hint),
                Err(RecvTimeoutError::Timeout) => break,
                Err(RecvTimeoutError::Disconnected) => {
                    senders_gone = true;
                    break;
                }
            }
        }
        let changes = collect(vault, burst);
        if !changes.is_empty() {
            emit(changes);
        }
        if senders_gone {
            return;
        }
    }
}

#[cfg(not(any(target_os = "ios", target_os = "android")))]
pub use desktop::FsWatcher;

#[cfg(not(any(target_os = "ios", target_os = "android")))]
mod desktop {
    use std::io;
    use std::path::Path;
    use std::sync::mpsc::Sender;

    use notify::{EventKind, RecursiveMode, Watcher as _};

    use super::Hint;

    /// Watches a vault folder with the operating system's file events
    /// (inotify, `FSEvents`, `ReadDirectoryChangesW`). Stops when dropped.
    #[derive(Debug)]
    pub struct FsWatcher {
        _watcher: notify::RecommendedWatcher,
    }

    impl FsWatcher {
        /// Starts watching `root` recursively; hints go to `hints`.
        ///
        /// # Errors
        /// If the operating system refuses to watch the folder.
        pub fn start(root: &Path, hints: Sender<Hint>) -> io::Result<Self> {
            let mut watcher =
                notify::recommended_watcher(move |result: notify::Result<notify::Event>| {
                    // A send fails only when the dispatcher has stopped; nothing to do then.
                    let _ = match result {
                        Ok(event) if matches!(event.kind, EventKind::Access(_)) => Ok(()),
                        Ok(event) if event.need_rescan() => hints.send(Hint::Rescan),
                        Ok(event) => event
                            .paths
                            .into_iter()
                            .try_for_each(|path| hints.send(Hint::Path(path))),
                        Err(_) => hints.send(Hint::Rescan),
                    };
                })
                .map_err(io::Error::other)?;
            watcher
                .watch(root, RecursiveMode::Recursive)
                .map_err(io::Error::other)?;
            Ok(Self { _watcher: watcher })
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::path::Path;

    use super::*;

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    #[test]
    fn classifies_paths() {
        let (dir, vault) = vault();
        let root = &dir.path().join("notes");
        fs::create_dir(root.join("sub")).unwrap();
        let cases = [
            ("a.md", PathKind::Note("a.md".into())),
            ("sub/b.MD", PathKind::Note("sub/b.MD".into())),
            (".c.md.icloud", PathKind::Note("c.md".into())),
            ("sub/.d.md.icloud", PathKind::Note("sub/d.md".into())),
            (".a.md.tmp-12-0", PathKind::Ignored),
            (".DS_Store", PathKind::Ignored),
            (".notesapp/history/x.md", PathKind::Ignored),
            ("attachments/ab.png", PathKind::Ignored),
            ("sub", PathKind::Folder),
            ("deleted-folder", PathKind::Folder),
            ("", PathKind::Folder),
        ];
        for (rel, expected) in cases {
            assert_eq!(vault.classify(&root.join(rel)), expected, "{rel}");
        }
        assert_eq!(
            vault.classify(Path::new("/elsewhere/x.md")),
            PathKind::Ignored
        );
        // Only notes/ holds notes; one outside it makes the app check again.
        assert_eq!(vault.classify(&dir.path().join("x.md")), PathKind::Folder);
        assert_eq!(vault.classify(dir.path()), PathKind::Folder);
    }

    #[cfg(unix)]
    #[test]
    fn classifies_canonical_paths_of_a_symlinked_vault() {
        let dir = tempfile::tempdir().unwrap();
        let real = dir.path().join("real");
        fs::create_dir(&real).unwrap();
        Vault::create(&real).unwrap();
        let link = dir.path().join("link");
        std::os::unix::fs::symlink(&real, &link).unwrap();
        let vault = Vault::open(&link).unwrap();
        let reported = fs::canonicalize(&real).unwrap().join("notes/n.md");
        assert_eq!(vault.classify(&reported), PathKind::Note("n.md".into()));
    }

    /// The iPhone case: the vault is opened in canonical form
    /// (`/private/var/mobile/…`), the presenter reports `/var/mobile/…`.
    #[cfg(unix)]
    #[test]
    fn classifies_symlinked_paths_of_a_canonical_vault() {
        let dir = tempfile::tempdir().unwrap();
        let real = fs::canonicalize(dir.path()).unwrap().join("real");
        fs::create_dir_all(&real).unwrap();
        Vault::create(&real).unwrap();
        fs::create_dir_all(real.join("notes/sub")).unwrap();
        fs::write(real.join("notes/n.md"), "x").unwrap();
        let link = dir.path().join("link").join("notes");
        std::os::unix::fs::symlink(&real, dir.path().join("link")).unwrap();
        let vault = Vault::open(&real).unwrap();
        assert_eq!(
            vault.classify(&link.join("n.md")),
            PathKind::Note("n.md".into())
        );
        // Gone already when the event arrives, also with its folder.
        assert_eq!(
            vault.classify(&link.join("deleted.md")),
            PathKind::Note("deleted.md".into())
        );
        assert_eq!(
            vault.classify(&link.join("gone/also-gone.md")),
            PathKind::Note("gone/also-gone.md".into())
        );
        assert_eq!(vault.classify(&link.join("sub")), PathKind::Folder);
        assert_eq!(
            vault.classify(&link.join(".n.md.tmp-1-0")),
            PathKind::Ignored
        );
    }

    #[test]
    fn collect_dedups_and_sorts() {
        let (dir, vault) = vault();
        let at = |rel: &str| Hint::Path(dir.path().join("notes").join(rel));
        let changes = collect(
            &vault,
            [at("b.md"), at(".b.md.tmp-1-0"), at("a.md"), at("b.md")],
        );
        assert_eq!(
            changes,
            VaultChanges {
                paths: vec!["a.md".into(), "b.md".into()],
                rescan: false,
                smart_folders: false,
            }
        );
        assert!(collect(&vault, [at(".DS_Store")]).is_empty());
        assert!(collect(&vault, [Hint::Rescan]).rescan);
        assert!(collect(&vault, [at("some-folder")]).rescan);
        let smart = collect(
            &vault,
            [Hint::Path(
                dir.path().join(crate::layout::SMART_FOLDERS_FILE),
            )],
        );
        assert!(smart.smart_folders && !smart.rescan && smart.paths.is_empty());
    }

    #[test]
    fn dispatcher_reports_a_burst_once() {
        let (dir, vault) = vault();
        let (tx, rx) = mpsc::channel();
        for name in ["a.md", "b.md", "a.md"] {
            tx.send(Hint::Path(dir.path().join("notes").join(name)))
                .unwrap();
        }
        drop(tx);
        let mut emitted = Vec::new();
        run_dispatcher(&vault, &rx, Duration::from_millis(20), |c| emitted.push(c));
        assert_eq!(
            emitted,
            [VaultChanges {
                paths: vec!["a.md".into(), "b.md".into()],
                rescan: false,
                smart_folders: false,
            }]
        );
    }

    #[test]
    fn serializes_camel_case() {
        let json = serde_json::to_string(&VaultChanges {
            paths: vec!["a.md".into()],
            rescan: true,
            smart_folders: false,
        })
        .unwrap();
        assert_eq!(
            json,
            r#"{"paths":["a.md"],"rescan":true,"smartFolders":false}"#
        );
    }

    #[cfg(not(any(target_os = "ios", target_os = "android")))]
    #[test]
    fn fs_watcher_reports_a_saved_note() {
        let (dir, vault) = vault();
        let (seen_tx, seen_rx) = mpsc::channel();
        let hints = spawn_dispatcher(vault.clone(), Duration::from_millis(50), move |c| {
            let _ = seen_tx.send(c);
        })
        .unwrap();
        let _watcher = FsWatcher::start(dir.path(), hints).unwrap();
        // Give the OS a moment to arm the watch before the write.
        thread::sleep(Duration::from_millis(200));
        let hash = vault.create_note("watched.md", "x").unwrap().hash;
        vault.write_note("watched.md", "y", &hash).unwrap();

        let deadline = Instant::now() + Duration::from_secs(10);
        let mut seen = BTreeSet::new();
        while !seen.contains("watched.md") {
            let left = deadline.saturating_duration_since(Instant::now());
            let changes = seen_rx
                .recv_timeout(left)
                .expect("no change reported in time");
            seen.extend(changes.paths);
        }
        assert!(!seen.iter().any(|p| p.contains("tmp")), "{seen:?}");
    }
}
