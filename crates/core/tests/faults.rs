// SPDX-License-Identifier: Apache-2.0
//! Data safety (N-04) when things go wrong underneath: the app killed while
//! it writes, a full disk, a vault it may not write to. A note must then be
//! either as it was or as it was written, never cut short or gone, and the
//! version before stays readable.
//!
//! The full-disk test needs a small disk image (`hdiutil`, macOS) and takes
//! about 15 s, so it is ignored by default:
//! `cargo test -p schnipsle-core --test faults -- --ignored --nocapture`.

#[allow(dead_code, reason = "only the random numbers are used here")]
mod support;

use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use schnipsle_core::{Vault, VaultError};
use support::generate::Rng;

/// Body number `k` the child writes: large enough that a write takes a while.
fn body(k: u64) -> String {
    let mut text = format!("# Written {k}\n\n");
    for line in 0..4000 {
        let _ = writeln!(text, "line {line} of version {k}");
    }
    let _ = writeln!(text, "END {k}");
    text
}

/// The number `k` of a complete [`body`] that `text` ends with, after front matter.
fn complete(text: &str) -> Option<u64> {
    let rest = text.strip_prefix("---\n")?;
    let body_start = rest.find("\n---\n")? + 5;
    let found = &rest[body_start..];
    let k = found.lines().last()?.strip_prefix("END ")?.parse().ok()?;
    (found == body(k)).then_some(k)
}

const CHILD: &str = "SCHNIPSLE_FAULTS_CHILD_VAULT";

/// Run by [`killed_while_saving`] in a child process: saves, snapshots,
/// creates, deletes and restores without end, until it is killed.
#[test]
#[ignore = "started by killed_while_saving"]
fn child_writer() {
    let Ok(root) = std::env::var(CHILD) else {
        return;
    };
    let vault = Vault::open(&root).unwrap();
    let log = Path::new(&root).join("created.log");
    let mut k = std::env::var("SCHNIPSLE_FAULTS_FROM")
        .unwrap()
        .parse::<u64>()
        .unwrap();
    loop {
        k += 1;
        let note = vault.read_note("main.md").unwrap();
        vault.write_note("main.md", &body(k), &note.hash).unwrap();
        if k % 3 == 0 {
            vault.snapshot_note("main.md", None).unwrap();
        }
        if k % 4 == 0 {
            let new = vault
                .create_titled("", &format!("# Extra {k}\n\nextra {k}\n"))
                .unwrap();
            let mut lines = fs::read_to_string(&log).unwrap_or_default();
            let _ = writeln!(lines, "{k}");
            fs::write(&log, lines).unwrap();
            let trashed = vault.trash_note(&new.path).unwrap();
            if k % 8 == 0 {
                vault.restore_note(&trashed.key).unwrap();
            }
        }
    }
}

/// Every file below `dir` whose name contains `part`.
fn files_with(dir: &Path, part: &str) -> Vec<PathBuf> {
    let mut found = Vec::new();
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            found.extend(files_with(&entry.path(), part));
        } else if entry.file_name().to_string_lossy().contains(part) {
            found.push(entry.path());
        }
    }
    found
}

#[test]
fn killed_while_saving() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("vault");
    fs::create_dir(&root).unwrap();
    let vault = Vault::create(&root).unwrap();
    vault.create_note("main.md", &body(0)).unwrap();
    let mut rng = Rng::new(7);
    let mut last = 0;
    let mut temp_files = 0;
    for round in 0..25 {
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "child_writer",
                "--exact",
                "--ignored",
                "--nocapture",
                "--quiet",
            ])
            .env(CHILD, &root)
            .env("SCHNIPSLE_FAULTS_FROM", ((round + 1) * 100_000).to_string())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        std::thread::sleep(Duration::from_millis(20 + rng.next() % 200));
        if let Ok(Some(status)) = child.try_wait() {
            let mut err = String::new();
            io::Read::read_to_string(&mut child.stderr.take().unwrap(), &mut err).unwrap();
            panic!("the child stopped by itself in round {round}: {status}\n{err}");
        }
        child.kill().unwrap(); // SIGKILL: no clean-up of any kind
        child.wait().unwrap();

        // The note is a complete version, never cut short or missing.
        let text = fs::read_to_string(root.join("notes/main.md"))
            .unwrap_or_else(|e| panic!("round {round}: main.md unreadable: {e}"));
        let k = complete(&text)
            .unwrap_or_else(|| panic!("round {round}: main.md is not a complete version"));
        assert!(
            k >= last,
            "round {round}: main.md went back from {last} to {k}"
        );
        last = k;

        // Every snapshot can be read.
        let vault = Vault::open(&root).unwrap();
        let id = schnipsle_core::front_matter::get(&text, "id").unwrap();
        for name in vault.snapshots(&id).unwrap() {
            let snap = vault.read_snapshot(&id, &name).unwrap_or_else(|e| {
                panic!("round {round}: snapshot {name} unreadable: {e}");
            });
            assert!(
                complete(&snap).is_some(),
                "round {round}: snapshot {name} is cut short"
            );
        }

        // Every extra note that was created is somewhere: a note or the trash.
        let created = fs::read_to_string(root.join("created.log")).unwrap_or_default();
        let mut everything = String::new();
        for file in files_with(&root, ".md") {
            everything.push_str(&fs::read_to_string(file).unwrap_or_default());
        }
        for k in created.lines() {
            assert!(
                everything.contains(&format!("extra {k}\n")),
                "round {round}: extra {k} lost"
            );
        }

        // A temporary file may be left, never a damaged note; the app
        // passes it over (it starts with a dot).
        temp_files = files_with(&root, ".tmp-").len();
    }
    println!(
        "25 kills: main.md always complete, at version {last}; {temp_files} temporary files left"
    );
}

/// `dir` and every folder below it.
fn folders(dir: &Path) -> Vec<PathBuf> {
    let mut found = vec![dir.to_owned()];
    for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
        if entry.file_type().is_ok_and(|t| t.is_dir()) {
            found.extend(folders(&entry.path()));
        }
    }
    found
}

/// Detaches a disk image when dropped, also when the test fails.
#[cfg(target_os = "macos")]
struct Detach(PathBuf);

#[cfg(target_os = "macos")]
impl Drop for Detach {
    fn drop(&mut self) {
        let _ = Command::new("hdiutil")
            .args(["detach", "-force", "-quiet"])
            .arg(&self.0)
            .status();
    }
}

/// The vault itself may not be written to: every write fails and changes nothing.
#[cfg(unix)]
#[test]
fn read_only_vault() {
    use std::os::unix::fs::PermissionsExt as _;
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("vault");
    fs::create_dir(&root).unwrap();
    let vault = Vault::create(&root).unwrap();
    let saved = vault.create_note("n.md", "# Note\n\ntext\n").unwrap();
    vault.snapshot_note("n.md", None).unwrap();
    let before = fs::read(root.join("notes/n.md")).unwrap();
    let all = folders(&root);
    let set = |mode: u32| {
        for dir in &all {
            fs::set_permissions(dir, fs::Permissions::from_mode(mode)).unwrap();
        }
    };
    set(0o555);
    if fs::write(root.join("notes/probe"), "x").is_ok() {
        set(0o755);
        eprintln!("skipped: permissions are not enforced here (root?)");
        return;
    }

    let vault = Vault::open(&root).unwrap();
    let denied = |r: Result<(), VaultError>, what: &str| match r {
        Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::PermissionDenied => {}
        other => panic!("{what}: expected permission denied, got {other:?}"),
    };
    denied(
        vault
            .write_note("n.md", "# Note\n\nmore\n", &saved.hash)
            .map(drop),
        "save",
    );
    denied(vault.create_note("m.md", "# New\n").map(drop), "create");
    denied(vault.trash_note("n.md").map(drop), "delete");
    denied(
        vault.snapshot("01HF7YAT04EXCA2ARJ0J9F8K0P", "x").map(drop),
        "snapshot",
    );
    assert_eq!(fs::read(root.join("notes/n.md")).unwrap(), before);
    assert_eq!(vault.read_note("n.md").unwrap().hash, saved.hash);
    set(0o755);
    assert!(
        files_with(&root, ".tmp-").is_empty(),
        "temporary files left"
    );
    assert_eq!(vault.list_notes().unwrap().len(), 1);
}

/// A full disk: writes fail and leave the note, its history and the trash as
/// they were; once there is room again, everything works.
#[cfg(target_os = "macos")]
#[test]
#[ignore = "makes a disk image; about 15 s"]
fn full_disk() {
    let dir = tempfile::tempdir().unwrap();
    let image = dir.path().join("full.dmg");
    let mount = dir.path().join("disk");
    fs::create_dir(&mount).unwrap();
    let run = |args: &[&std::ffi::OsStr]| {
        let status = Command::new("hdiutil").args(args).status().unwrap();
        assert!(status.success(), "hdiutil {args:?}");
    };
    run(&[
        "create".as_ref(),
        "-size".as_ref(),
        "2m".as_ref(),
        "-fs".as_ref(),
        "HFS+".as_ref(),
        "-volname".as_ref(),
        "SchnipsleFull".as_ref(),
        "-quiet".as_ref(),
        image.as_os_str(),
    ]);
    run(&[
        "attach".as_ref(),
        image.as_os_str(),
        "-mountpoint".as_ref(),
        mount.as_os_str(),
        "-nobrowse".as_ref(),
        "-quiet".as_ref(),
    ]);
    let _detach = Detach(mount.clone());

    let root = mount.join("vault");
    fs::create_dir(&root).unwrap();
    let vault = Vault::create(&root).unwrap();
    let saved = vault.create_note("n.md", "# Note\n\ntext\n").unwrap();
    vault.snapshot_note("n.md", None).unwrap();
    let before = fs::read(root.join("notes/n.md")).unwrap();

    // Fill the disk.
    let filler = mount.join("filler");
    let mut file = fs::File::create(&filler).unwrap();
    let block = vec![0u8; 64 * 1024];
    loop {
        if let Err(e) = io::Write::write_all(&mut file, &block) {
            assert_eq!(e.raw_os_error(), Some(28), "expected ENOSPC, got {e}"); // ENOSPC
            break;
        }
    }
    drop(file);

    // Random hex, so the compressed snapshot does not fit either.
    let mut rng = Rng::new(3);
    let mut noise = String::new();
    for _ in 0..40_000 {
        let _ = write!(noise, "{:016x}", rng.next());
    }
    let big = format!("# Note\n\n{noise}\n");
    let full = |r: Result<(), VaultError>, what: &str| match r {
        Err(VaultError::Io(e)) if e.raw_os_error() == Some(28) => {}
        other => panic!("{what}: expected a full disk, got {other:?}"),
    };
    full(
        vault.write_note("n.md", &big, &saved.hash).map(drop),
        "save",
    );
    full(vault.create_note("m.md", &big).map(drop), "create");
    full(
        vault.snapshot("01HF7YAT04EXCA2ARJ0J9F8K0P", &big).map(drop),
        "snapshot",
    );
    assert_eq!(fs::read(root.join("notes/n.md")).unwrap(), before);
    assert!(
        !root.join("notes/m.md").exists(),
        "a half-written note was left"
    );
    assert!(
        files_with(&root, ".tmp-").is_empty(),
        "temporary files left"
    );

    // Room again: the same save goes through.
    fs::remove_file(&filler).unwrap();
    vault.write_note("n.md", &big, &saved.hash).unwrap();
    assert!(vault.read_note("n.md").unwrap().text.ends_with(&big));
}
