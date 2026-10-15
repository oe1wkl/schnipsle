// SPDX-License-Identifier: Apache-2.0
//! File coordination (ADR-0015, format.md §10).
//!
//! On Apple platforms every read and write of vault files must be coordinated
//! with iCloud Drive and other apps through `NSFileCoordinator`. The core does not
//! know about Apple APIs: it runs its file operations inside a [`Coordinator`],
//! and the app shell passes in the platform implementation from `schnipsle-apple`.
//! Everywhere else, and in tests, [`Uncoordinated`] runs them directly.

use std::fmt;
use std::io;
use std::path::Path;

/// Runs file operations while holding coordinated access to a path.
///
/// Implementations call `f` at most once, synchronously, before returning. `f`
/// receives the path to use, which may differ from the one passed in if the file
/// was moved while waiting for access. If access cannot be obtained,
/// implementations return an error without calling `f`.
///
/// Both methods may block until other processes finish with the file, so callers
/// must not run them on a UI thread.
pub trait Coordinator: Send + Sync + fmt::Debug {
    /// Runs `f` with shared read access to `path` (a file or a folder).
    ///
    /// # Errors
    /// The error from `f`, or an error if access could not be obtained.
    fn read(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()>;

    /// Runs `f` with exclusive access for replacing `path`.
    ///
    /// # Errors
    /// The error from `f`, or an error if access could not be obtained.
    fn write(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()>;
}

/// No coordination: runs every operation directly. Used off Apple platforms.
#[derive(Debug, Default, Clone, Copy)]
pub struct Uncoordinated;

impl Coordinator for Uncoordinated {
    fn read(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        f(path)
    }

    fn write(&self, path: &Path, f: &mut dyn FnMut(&Path) -> io::Result<()>) -> io::Result<()> {
        f(path)
    }
}
