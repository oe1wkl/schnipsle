// SPDX-License-Identifier: Apache-2.0
//! Data safety (N-04): long random runs of what the app and the world outside
//! do to a vault, checked against what must never be lost.
//!
//! One actor is the app, used as the UI uses it: an editor with one open note
//! that types, autosaves, settles a conflict as the conflict banner does
//! (`Vault::keep_mine`, `Vault::use_other`, ADR-0042), takes history snapshots, renames a note after a
//! change of title, deletes, restores from the trash and from the history,
//! and settles same-`id` copies. The other actor is the world outside: other
//! devices and editors that change, delete, rename, copy and create note
//! files behind the app's back, without coordination.
//!
//! After every step of the app, every version of a note that was on disk
//! before must still be somewhere (a note, the history or the trash) unless
//! the user knowingly replaced it, i.e. saved over the version the editor had
//! loaded; the editor's text must be kept when the user picks the other
//! version; a snapshot once taken stays; and no temporary file is left.
//!
//! `cargo test -p schnipsle-core --test data_safety` runs eight short runs.
//! The long run (24 runs of 3000 steps, about 7 minutes) is ignored by
//! default; CI runs it in a job of its own with
//! `cargo test -p schnipsle-core --release --test data_safety -- --ignored --nocapture`.
//! `SCHNIPSLE_SEED=<n>` repeats the run with that seed, printed on failure.

mod support;

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use schnipsle_core::vault::sha256_hex;
use schnipsle_core::{NoteContent, Vault, VaultError, front_matter};
use support::generate::{Rng, generate};

/// The note open in the editor, as the UI keeps it.
#[allow(clippy::struct_field_names, reason = "named as in the UI")]
struct Open {
    path: String,
    /// The editor's text.
    text: String,
    /// Hash of the version the editor last loaded or saved.
    hash: String,
    title_at_open: String,
    /// The text has changes not yet saved.
    dirty: bool,
    /// A save found the note changed elsewhere: the hash on disk then, `None`
    /// if it was gone. The conflict banner is shown until it is settled.
    #[allow(
        clippy::option_option,
        reason = "as the UI: no conflict, or one with an optional hash"
    )]
    conflict: Option<Option<String>>,
}

impl Open {
    fn from(content: &NoteContent) -> Self {
        Self {
            path: content.path.clone(),
            text: content.text.clone(),
            hash: content.hash.clone(),
            title_at_open: content.title.clone(),
            dirty: false,
            conflict: None,
        }
    }
}

/// What one step of the app may lose and must keep.
#[derive(Default)]
struct Expect {
    /// Versions the user knowingly replaced: saved over in the editor.
    replaced: BTreeSet<String>,
    /// Texts that must be found afterwards, with what they were.
    kept: Vec<(String, String)>,
    /// Versions that may come back with a new `id`, `created` and
    /// `modified`: "Keep both" gives one of them a new `id` (format.md §4),
    /// keeping a version stamps it anew, and "Use other version" gives a
    /// note without `id` one.
    restamped: BTreeSet<String>,
}

/// The text without the front matter the app stamps: `id`, `created`, `modified`.
fn unstamped(text: &str) -> String {
    let stamped = |l: &&str| {
        ["id:", "created:", "modified:"]
            .iter()
            .any(|k| l.starts_with(k))
    };
    let block: Vec<&str> = front_matter::lines(text)
        .unwrap_or_default()
        .into_iter()
        .filter(|l| !stamped(l))
        .collect();
    format!("{}\n{}", block.join("\n"), front_matter::body(text))
}

/// Adds a line to the run's log, shown when a check fails.
macro_rules! say {
    ($run:expr, $($what:tt)*) => {
        $run.log.push(format!("{:5} {}", $run.step, format!($($what)*)))
    };
}

struct Run {
    _dir: tempfile::TempDir,
    vault: Vault,
    rng: Rng,
    seed: u64,
    step: usize,
    open: Option<Open>,
    /// Numbers the texts written, so every version differs.
    n: u64,
    /// Snapshot file → hash of its text; snapshots never change.
    snapshots: HashMap<PathBuf, String>,
    log: Vec<String>,
}

impl Run {
    fn new(seed: u64) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("vault");
        fs::create_dir(&root).unwrap();
        let vault = Vault::create(&root).unwrap();
        generate(vault.notes_dir(), 6, seed).unwrap();
        Self {
            _dir: dir,
            vault,
            rng: Rng::new(seed ^ 0x5EED),
            seed,
            step: 0,
            open: None,
            n: 0,
            snapshots: HashMap::new(),
            log: Vec::new(),
        }
    }

    fn next(&mut self) -> u64 {
        self.n += 1;
        self.n
    }

    fn chance(&mut self, percent: usize) -> bool {
        self.rng.below(100) < percent
    }

    #[track_caller]
    fn fail(&self, why: &str) -> ! {
        let tail = self.log[self.log.len().saturating_sub(40)..].join("\n");
        panic!(
            "seed {} (SCHNIPSLE_SEED={}), step {}: {why}\nlast steps:\n{tail}",
            self.seed, self.seed, self.step
        );
    }

    // ---- what is on disk ----------------------------------------------------

    /// Every note file below `notes/`: path relative to it → bytes.
    fn notes(&self) -> BTreeMap<String, Vec<u8>> {
        fn walk(dir: &Path, prefix: &str, out: &mut BTreeMap<String, Vec<u8>>) {
            let Ok(entries) = fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let path = format!("{prefix}{name}");
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    walk(&entry.path(), &format!("{path}/"), out);
                } else if Path::new(&name).extension().is_some_and(|e| e == "md")
                    && let Ok(bytes) = fs::read(entry.path())
                {
                    out.insert(path, bytes);
                }
            }
        }
        let mut out = BTreeMap::new();
        walk(self.vault.notes_dir(), "", &mut out);
        out
    }

    /// Hashes of the texts of all snapshots.
    fn history(&mut self) -> BTreeSet<String> {
        let dir = self.vault.root().join(".notesapp/history");
        let mut hashes = BTreeSet::new();
        for id in fs::read_dir(&dir).into_iter().flatten().flatten() {
            let id_name = id.file_name().to_string_lossy().into_owned();
            for snap in fs::read_dir(id.path()).into_iter().flatten().flatten() {
                let name = snap.file_name().to_string_lossy().into_owned();
                if name.starts_with('.') {
                    continue;
                }
                let file = snap.path();
                if !self.snapshots.contains_key(&file) {
                    let text = self
                        .vault
                        .read_snapshot(&id_name, &name)
                        .unwrap_or_else(|e| self.fail(&format!("snapshot {name} unreadable: {e}")));
                    self.snapshots
                        .insert(file.clone(), sha256_hex(text.as_bytes()));
                }
                hashes.insert(self.snapshots[&file].clone());
            }
        }
        hashes
    }

    /// Hashes of every version that can still be found: notes, history, trash.
    fn everywhere(&mut self) -> BTreeSet<String> {
        let mut all: BTreeSet<String> = self.notes().values().map(|b| sha256_hex(b)).collect();
        all.extend(self.history());
        let trash = self.vault.root().join(".notesapp/trash");
        for entry in fs::read_dir(trash).into_iter().flatten().flatten() {
            if entry.file_name().to_string_lossy().ends_with(".md")
                && let Ok(bytes) = fs::read(entry.path())
            {
                all.insert(sha256_hex(&bytes));
            }
        }
        all
    }

    /// Fails if a temporary file was left anywhere in the vault.
    fn check_no_temp_files(&self) {
        fn walk(dir: &Path, found: &mut Vec<PathBuf>) {
            for entry in fs::read_dir(dir).into_iter().flatten().flatten() {
                if entry.file_type().is_ok_and(|t| t.is_dir()) {
                    walk(&entry.path(), found);
                } else if entry.file_name().to_string_lossy().contains(".tmp-") {
                    found.push(entry.path());
                }
            }
        }
        let mut found = Vec::new();
        walk(self.vault.root(), &mut found);
        if !found.is_empty() {
            self.fail(&format!("temporary files left: {found:?}"));
        }
    }

    // ---- the run --------------------------------------------------------------

    fn run(&mut self, steps: usize) {
        let mut ever_kept = BTreeSet::new();
        for step in 0..steps {
            self.step = step;
            if self.chance(25) {
                self.outside();
                continue;
            }
            let before = self.notes();
            let mut expect = Expect::default();
            self.app(&mut expect);
            let all = self.everywhere();
            let restamped: BTreeSet<String> = if expect.restamped.is_empty() {
                BTreeSet::new()
            } else {
                self.notes()
                    .values()
                    .map(|b| unstamped(&String::from_utf8_lossy(b)))
                    .collect()
            };
            for (path, bytes) in &before {
                let hash = sha256_hex(bytes);
                let text = String::from_utf8_lossy(bytes);
                if expect.restamped.contains(&hash) && restamped.contains(&unstamped(&text)) {
                    continue;
                }
                if !all.contains(&hash) && !expect.replaced.contains(&hash) {
                    self.fail(&format!(
                        "the version of {path} on disk before this step is lost:\n{}",
                        String::from_utf8_lossy(bytes)
                    ));
                }
            }
            for (hash, what) in &expect.kept {
                if !all.contains(hash) {
                    self.fail(&format!("not kept: {what}"));
                }
            }
            let history = self.history();
            if let Some(gone) = ever_kept.difference(&history).next() {
                self.fail(&format!("a snapshot disappeared: {gone}"));
            }
            ever_kept = history;
            self.check_no_temp_files();
        }
    }

    // ---- the app --------------------------------------------------------------

    fn app(&mut self, expect: &mut Expect) {
        if self.open.as_ref().is_some_and(|o| o.conflict.is_some()) && self.chance(40) {
            self.settle_conflict(expect);
            return;
        }
        match self.rng.below(100) {
            0..40 => self.type_text(),
            40..58 => {
                self.save(expect);
            }
            58..64 => self.pause(expect),
            64..76 => self.open_another(expect),
            76..80 => self.new_note(expect),
            80..85 => self.delete(expect),
            85..89 => self.restore_trashed(),
            89..94 => self.restore_version(expect),
            _ => self.settle_copies(expect),
        }
    }

    fn type_text(&mut self) {
        let n = self.next();
        let retitle = self.chance(12);
        let Some(open) = self.open.as_mut() else {
            return;
        };
        if retitle && let Some(at) = open.text.find("\n# ") {
            let end = open.text[at + 1..]
                .find('\n')
                .map_or(open.text.len(), |e| at + 1 + e);
            open.text
                .replace_range(at + 1..end, &format!("# Title {n}"));
        } else {
            let _ = write!(open.text, "\nline {n}\n");
        }
        open.dirty = true;
        let path = open.path.clone();
        say!(
            self,
            "type in {path}{}",
            if retitle { " (title)" } else { "" }
        );
    }

    /// Autosave. Returns whether nothing is left unsaved.
    fn save(&mut self, expect: &mut Expect) -> bool {
        let Some(open) = self.open.as_mut() else {
            return true;
        };
        if open.conflict.is_some() {
            return false;
        }
        if !open.dirty {
            return true;
        }
        let path = open.path.clone();
        match self.vault.write_note(&path, &open.text, &open.hash) {
            Ok(saved) => {
                expect.replaced.insert(open.hash.clone());
                open.text = saved.text;
                open.hash = saved.hash;
                open.dirty = false;
                let on_disk = fs::read(self.vault.notes_dir().join(&path)).unwrap_or_default();
                if sha256_hex(&on_disk) != open.hash {
                    self.fail(&format!("saved {path}, but the file has other text"));
                }
                say!(self, "save {path}");
                true
            }
            Err(VaultError::Conflict { current_hash, .. }) => {
                say!(self, "save {path}: conflict, {current_hash:?}");
                open.conflict = Some(current_hash);
                false
            }
            Err(e) => self.fail(&format!("save {path}: {e}")),
        }
    }

    /// The conflict banner: "Keep mine" or "Use other version" (ADR-0042).
    fn settle_conflict(&mut self, expect: &mut Expect) {
        let mine = self.chance(50);
        let Some(open) = self.open.as_mut() else {
            return;
        };
        let Some(on_disk) = open.conflict.clone() else {
            return;
        };
        let path = open.path.clone();
        if mine {
            // The other version goes into the history before it is replaced.
            match self.vault.keep_mine(&path, &open.text, on_disk.as_deref()) {
                Ok(saved) => {
                    open.text = saved.text;
                    open.hash = saved.hash;
                    open.dirty = false;
                    open.conflict = None;
                    say!(self, "keep mine in {path} (other: {on_disk:?})");
                }
                Err(VaultError::Conflict { current_hash, .. }) => {
                    open.conflict = Some(current_hash);
                    say!(self, "keep mine in {path}: changed again");
                }
                Err(e) => self.fail(&format!("keep mine in {path}: {e}")),
            }
            return;
        }
        // The editor's text goes into the history (or the trash), and the
        // note may get an id; then the editor shows the note as it is.
        expect.kept.push((
            sha256_hex(open.text.as_bytes()),
            format!("the editor's text of {path} after \"Use other version\""),
        ));
        if let Ok(bytes) = fs::read(self.vault.notes_dir().join(&path)) {
            expect.restamped.insert(sha256_hex(&bytes));
        }
        match self.vault.use_other(&path, &open.text) {
            Ok(used) => {
                say!(
                    self,
                    "use other version of {path} (trashed: {:?})",
                    used.trashed.map(|t| t.key)
                );
                self.open = used.note.as_ref().map(Open::from);
            }
            Err(VaultError::Conflict { current_hash, .. }) => {
                open.conflict = Some(current_hash);
                say!(self, "use other version of {path}: changed again");
            }
            Err(e) => self.fail(&format!("use other version of {path}: {e}")),
        }
    }

    /// A typing pause: saved, then stored in the history.
    fn pause(&mut self, expect: &mut Expect) {
        if !self.save(expect) {
            return;
        }
        let Some(open) = &self.open else {
            return;
        };
        let path = open.path.clone();
        let text = open.text.clone();
        let unchanged =
            fs::read(self.vault.notes_dir().join(&path)).is_ok_and(|b| sha256_hex(&b) == open.hash);
        match self.vault.snapshot_note(&path, None) {
            Ok(_) => {
                say!(self, "snapshot {path}");
                // What was snapshotted is the file; it is the editor's text
                // unless it changed elsewhere meanwhile (not within one step).
                if unchanged && text.contains("\nid: ") {
                    expect
                        .kept
                        .push((sha256_hex(text.as_bytes()), format!("snapshot of {path}")));
                }
            }
            Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => self.fail(&format!("snapshot {path}: {e}")),
        }
    }

    /// Leaves the open note as the UI does: saved, renamed after its title.
    /// Returns `false` if it cannot be left, as text is still unsaved.
    fn leave(&mut self, expect: &mut Expect) -> bool {
        if !self.save(expect) {
            return false;
        }
        if let Some(open) = self.open.take() {
            match self
                .vault
                .rename_after_edit(&open.path, &open.title_at_open)
            {
                Ok(path) if path != open.path => say!(self, "rename {} to {path}", open.path),
                Ok(_) => {}
                Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {}
                Err(e) => self.fail(&format!("rename {}: {e}", open.path)),
            }
        }
        true
    }

    fn open_another(&mut self, expect: &mut Expect) {
        if !self.leave(expect) {
            return;
        }
        let notes: Vec<String> = self.notes().into_keys().collect();
        if notes.is_empty() {
            return;
        }
        let path = notes[self.rng.below(notes.len())].clone();
        match self.vault.read_note(&path) {
            Ok(content) => {
                say!(self, "open {path}");
                self.open = Some(Open::from(&content));
            }
            Err(e) => self.fail(&format!("open {path}: {e}")),
        }
    }

    fn new_note(&mut self, expect: &mut Expect) {
        if !self.leave(expect) {
            return;
        }
        let n = self.next();
        match self
            .vault
            .create_titled("", &format!("# Note {n}\n\nbody {n}\n"))
        {
            Ok(new) => {
                say!(self, "new note {}", new.path);
                self.open = Some(Open {
                    path: new.path,
                    title_at_open: format!("Note {n}"),
                    hash: new.saved.hash,
                    text: new.saved.text,
                    dirty: false,
                    conflict: None,
                });
            }
            Err(e) => self.fail(&format!("new note: {e}")),
        }
    }

    /// Delete: refused while text is unsaved, as in the UI.
    fn delete(&mut self, expect: &mut Expect) {
        if !self.save(expect) {
            return;
        }
        let Some(open) = self.open.take() else {
            return;
        };
        match self.vault.trash_note(&open.path) {
            Ok(trashed) => say!(self, "delete {} as {}", open.path, trashed.key),
            Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => self.fail(&format!("delete {}: {e}", open.path)),
        }
    }

    fn restore_trashed(&mut self) {
        let trash = self
            .vault
            .list_trash()
            .unwrap_or_else(|e| self.fail(&format!("trash: {e}")));
        if trash.is_empty() {
            return;
        }
        let key = trash[self.rng.below(trash.len())].key.clone();
        match self.vault.restore_note(&key) {
            Ok(path) => say!(self, "restore {key} to {path}"),
            Err(e) => self.fail(&format!("restore {key}: {e}")),
        }
    }

    /// Restore a version: refused while text is unsaved, as in the UI.
    fn restore_version(&mut self, expect: &mut Expect) {
        if !self.save(expect) {
            return;
        }
        let Some(open) = &self.open else {
            return;
        };
        let path = open.path.clone();
        let hash = open.hash.clone();
        let Ok(versions) = self.vault.versions(&path) else {
            return;
        };
        if versions.is_empty() {
            return;
        }
        let name = versions[self.rng.below(versions.len())].name.clone();
        match self.vault.restore_version(&path, &name, &hash) {
            Ok(saved) => {
                say!(self, "restore {path} to {name}");
                let open = self.open.as_mut().unwrap();
                open.text = saved.text;
                open.hash = saved.hash;
            }
            Err(VaultError::Conflict { .. }) => say!(self, "restore {path}: changed"),
            Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => self.fail(&format!("restore {path} to {name}: {e}")),
        }
    }

    /// A same-`id` copy beside the open note: keep both, or keep one version.
    fn settle_copies(&mut self, expect: &mut Expect) {
        if !self.save(expect) {
            return;
        }
        let both = self.chance(50);
        let theirs = self.chance(50);
        let Some(open) = &self.open else {
            return;
        };
        let path = open.path.clone();
        let Ok(content) = self.vault.read_note(&path) else {
            return;
        };
        if content.hash != open.hash {
            return; // changed elsewhere: the app reloads first
        }
        let Some(other) = content.conflicts.first() else {
            return;
        };
        let result = if let (Some(copy), true) = (&other.copy_path, both) {
            say!(self, "keep {path} and {copy} both");
            expect.restamped.insert(content.hash.clone());
            expect.restamped.insert(other.hash.clone());
            self.vault.keep_both(&path, copy)
        } else {
            let text = if theirs {
                other.text.clone()
            } else {
                open.text.clone()
            };
            say!(
                self,
                "keep {} version of {path}",
                if theirs { "the other" } else { "this" }
            );
            expect.restamped.insert(sha256_hex(text.as_bytes()));
            self.vault.resolve_conflict(&path, &text, &open.hash)
        };
        match result {
            Ok(content) => self.open = Some(Open::from(&content)),
            Err(VaultError::Conflict { .. }) => say!(self, "changed meanwhile"),
            Err(e) => self.fail(&format!("settle copies of {path}: {e}")),
        }
    }

    // ---- the world outside ----------------------------------------------------

    fn outside(&mut self) {
        let n = self.next();
        let notes = self.notes();
        let kind = self.rng.below(100);
        let pick = (!notes.is_empty()).then(|| {
            let at = self.rng.below(notes.len());
            notes
                .iter()
                .nth(at)
                .map(|(p, b)| (p.clone(), b.clone()))
                .unwrap()
        });
        let dir = self.vault.notes_dir().to_owned();
        match (kind, pick) {
            (0..15, _) | (_, None) => {
                let path = format!("plain {n}.md");
                fs::write(dir.join(&path), format!("# Plain {n}\n\ntext {n}\n")).unwrap();
                say!(self, "outside: new {path}, no front matter");
            }
            (15..55, Some((path, bytes))) => {
                let mut text = String::from_utf8_lossy(&bytes).into_owned();
                let strip = self.chance(15);
                if strip
                    && let Some(rest) = text.strip_prefix("---\n")
                    && let Some(end) = rest.find("\n---\n")
                {
                    let block = "---\n".len() + end + "\n---\n".len();
                    text.drain(..block);
                }
                let _ = write!(text, "\nelsewhere {n}\n");
                fs::write(dir.join(&path), text).unwrap();
                say!(
                    self,
                    "outside: edit {path}{}",
                    if strip { ", front matter gone" } else { "" }
                );
            }
            (55..65, Some((path, _))) => {
                fs::remove_file(dir.join(&path)).unwrap();
                say!(self, "outside: delete {path}");
            }
            (65..80, Some((path, _))) => {
                let to = match path.rsplit_once('/') {
                    Some((folder, _)) => format!("{folder}/moved {n}.md"),
                    None => format!("moved {n}.md"),
                };
                fs::rename(dir.join(&path), dir.join(&to)).unwrap();
                say!(self, "outside: rename {path} to {to}");
            }
            (_, Some((path, bytes))) => {
                let copy = format!("{} 2.md", path.trim_end_matches(".md"));
                if notes.contains_key(&copy) {
                    return;
                }
                let mut text = String::from_utf8_lossy(&bytes).into_owned();
                let _ = write!(text, "\nother device {n}\n");
                fs::write(dir.join(&copy), text).unwrap();
                say!(self, "outside: copy {path} as {copy}");
            }
        }
    }
}

fn seeds(default: std::ops::Range<u64>) -> Vec<u64> {
    match std::env::var("SCHNIPSLE_SEED") {
        Ok(seed) => vec![seed.parse().expect("SCHNIPSLE_SEED is a number")],
        Err(_) => default.collect(),
    }
}

#[test]
fn random_runs() {
    for seed in seeds(1..9) {
        Run::new(seed).run(300);
    }
}

#[test]
#[ignore = "long; CI runs it with --release"]
fn long_random_runs() {
    let seeds = seeds(1000..1024);
    for seed in &seeds {
        Run::new(*seed).run(3000);
    }
    println!("{} runs of 3000 steps, nothing lost", seeds.len());
}
