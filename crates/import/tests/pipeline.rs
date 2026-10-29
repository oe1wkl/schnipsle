// SPDX-License-Identifier: Apache-2.0
//! Golden-file tests of the import pipeline (format.md §12): each case in
//! `tests/fixtures/import/pipeline/` imports a batch into an empty vault and
//! compares the dry-run report, the notes, the stored attachments and the
//! import log with `expected/`. Run with `SCHNIPSLE_BLESS=1` to write
//! `expected/` from the current output after a deliberate change.

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};

use schnipsle_core::stamp::ulid;
use schnipsle_core::{Vault, VaultError};
use schnipsle_import::{
    AttachmentData, Batch, Env, Outcome, PlannedAction, SkipReason, apply, plan,
};

/// A fixed clock and ids counting up, so the output is the same every run.
struct TestEnv {
    now_ms: Cell<u64>,
    next: Cell<u64>,
}

impl TestEnv {
    fn new() -> Self {
        Self {
            now_ms: Cell::new(1_793_268_000_000), // 2026-10-29T10:00:00Z
            next: Cell::new(1),
        }
    }
}

impl Env for TestEnv {
    fn now_ms(&self) -> u64 {
        self.now_ms.get()
    }

    fn new_id(&self) -> std::io::Result<String> {
        let n = self.next.get();
        self.next.set(n + 1);
        let mut random = [0u8; 10];
        random[2..].copy_from_slice(&n.to_be_bytes());
        Ok(ulid(self.now_ms.get(), random))
    }
}

fn case(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/import/pipeline")
        .join(name)
}

/// A batch from JSON, with attachment files relative to the case folder.
fn batch(dir: &Path, file: &str) -> Batch {
    let mut batch: Batch =
        serde_json::from_str(&fs::read_to_string(dir.join(file)).unwrap()).unwrap();
    for note in &mut batch.notes {
        for attachment in &mut note.attachments {
            if let AttachmentData::File(path) = &mut attachment.data {
                *path = dir.join(&*path);
            }
        }
    }
    batch
}

/// Every file below `root`, by path relative to it.
fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(root: &Path, dir: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        for entry in entries {
            let path = entry.unwrap().path();
            if path.is_dir() {
                walk(root, &path, out);
            } else {
                let rel = path
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, fs::read(&path).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

/// What a run produced, as files: `plan.json`, `notes/…`, `attachments.txt`
/// (paths, without sidecars) and `import-log.yaml` (snapshot names, which
/// carry the real time, replaced by `<snapshot>`).
fn output(vault_dir: &Path, outcome: &Outcome) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let plan = serde_json::to_string_pretty(&outcome.plan).unwrap() + "\n";
    out.insert("plan.json".to_owned(), plan.into_bytes());
    for (path, bytes) in files(&vault_dir.join("notes")) {
        out.insert(format!("notes/{path}"), bytes);
    }
    let mut attachments = String::new();
    for path in files(&vault_dir.join("attachments")).keys() {
        if Path::new(path).extension().is_none_or(|e| e != "yaml") {
            let _ = writeln!(attachments, "attachments/{path}");
        }
    }
    out.insert("attachments.txt".to_owned(), attachments.into_bytes());
    if let Some(id) = &outcome.run_id {
        let file = vault_dir.join(format!(".notesapp/import-log/{id}.yaml"));
        let mut log = String::new();
        for line in fs::read_to_string(file).unwrap().lines() {
            match line.split_once("replaced: ") {
                Some((head, _)) => writeln!(log, "{head}replaced: <snapshot>"),
                None => writeln!(log, "{line}"),
            }
            .unwrap();
        }
        out.insert("import-log.yaml".to_owned(), log.into_bytes());
    }
    out
}

fn check(dir: &Path, actual: &BTreeMap<String, Vec<u8>>) {
    let expected_dir = dir.join("expected");
    if std::env::var_os("SCHNIPSLE_BLESS").is_some() {
        let _ = fs::remove_dir_all(&expected_dir);
        for (path, bytes) in actual {
            let file = expected_dir.join(path);
            fs::create_dir_all(file.parent().unwrap()).unwrap();
            fs::write(file, bytes).unwrap();
        }
        return;
    }
    let expected = files(&expected_dir);
    let names = |m: &BTreeMap<String, Vec<u8>>| m.keys().cloned().collect::<Vec<_>>();
    assert_eq!(
        names(actual),
        names(&expected),
        "files in {}",
        expected_dir.display()
    );
    for (path, bytes) in actual {
        assert_eq!(
            String::from_utf8_lossy(bytes),
            String::from_utf8_lossy(&expected[path]),
            "{path}"
        );
    }
}

fn empty_vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path()).unwrap();
    (dir, vault)
}

#[test]
fn basic() {
    let dir = case("basic");
    let (vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let planned = plan(&vault, &batch(&dir, "batch.json"), &env).unwrap();
    // The dry run writes nothing.
    assert!(files(&vault_dir.path().join("notes")).is_empty());
    assert!(!vault_dir.path().join(".notesapp/import-log").exists());
    assert_eq!(planned.count(PlannedAction::Add), 8);
    assert_eq!(planned.items[5].reason, Some(SkipReason::Duplicate));

    let outcome = apply(&vault, planned, &env).unwrap();
    assert_eq!(outcome.written.len(), 8);
    check(&dir, &output(vault_dir.path(), &outcome));
}

#[test]
fn reimport() {
    let dir = case("reimport");
    let (vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let first = apply(
        &vault,
        plan(&vault, &batch(&dir, "first.json"), &env).unwrap(),
        &env,
    )
    .unwrap();
    assert_eq!(first.written.len(), 5);

    // B is edited here, F deleted.
    let path_of = |title: &str| {
        let item = first.plan.items.iter().find(|i| i.title == title).unwrap();
        item.path.clone().unwrap()
    };
    // (As is, so the golden file does not depend on the clock.)
    let b = vault.read_note(&path_of("Note B")).unwrap();
    vault
        .replace_note_as_is(&b.path, &format!("{}Edited here.\n", b.text), &b.hash)
        .unwrap();
    fs::remove_file(vault.note_file(&path_of("Note F")).unwrap()).unwrap();

    env.now_ms.set(env.now_ms.get() + 86_400_000);
    let second = plan(&vault, &batch(&dir, "second.json"), &env).unwrap();
    let reasons: Vec<_> = second.items.iter().map(|i| (i.action, i.reason)).collect();
    assert_eq!(
        reasons,
        [
            (PlannedAction::Update, None),
            (PlannedAction::Skip, Some(SkipReason::ChangedHere)),
            (PlannedAction::Skip, Some(SkipReason::NotNewer)),
            (PlannedAction::Skip, Some(SkipReason::NoModifiedTime)),
            (PlannedAction::Add, None),
            (PlannedAction::Skip, Some(SkipReason::Missing)),
        ]
    );
    let outcome = apply(&vault, second, &env).unwrap();

    // The version replaced is in the history.
    let a = vault.read_note(&path_of("Note A")).unwrap();
    assert!(a.text.contains("Second version from the source."));
    let id = schnipsle_core::front_matter::get(&a.text, "id").unwrap();
    let versions = vault.snapshots(&id).unwrap();
    assert_eq!(versions.len(), 1);
    assert!(
        vault
            .read_snapshot(&id, &versions[0])
            .unwrap()
            .contains("First version.")
    );
    // Its file name and `created` stay.
    assert_eq!(outcome.written[0].path, path_of("Note A"));
    check(&dir, &output(vault_dir.path(), &outcome));
}

#[test]
fn a_note_changed_after_the_dry_run_is_left_alone() {
    let dir = case("reimport");
    let (_vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let first = apply(
        &vault,
        plan(&vault, &batch(&dir, "first.json"), &env).unwrap(),
        &env,
    )
    .unwrap();
    let second = plan(&vault, &batch(&dir, "second.json"), &env).unwrap();
    let a = vault.read_note(first.written[0].path.as_str()).unwrap();
    vault
        .write_note(
            &a.path,
            &format!("{}Edited after the dry run.\n", a.text),
            &a.hash,
        )
        .unwrap();
    let outcome = apply(&vault, second, &env).unwrap();
    assert_eq!(
        outcome.plan.items[0].reason,
        Some(SkipReason::ChangedMeanwhile)
    );
    let a = vault.read_note(&a.path).unwrap();
    assert!(a.text.contains("Edited after the dry run."));
    // The others are written and logged; A is not.
    assert_eq!(outcome.written.len(), 3); // B, E and F
    assert!(outcome.written.iter().all(|w| w.path != a.path));
    let log = vault.import_logs().unwrap().pop().unwrap().1.unwrap();
    assert_eq!(log.entries().len(), 3);
    assert!(log.finished().is_some());
}

#[test]
fn nothing_to_write_writes_no_log() {
    let (_vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch: Batch = serde_json::from_str(r#"{"source":"html","input":"x","notes":[]}"#).unwrap();
    let outcome = apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();
    assert_eq!(outcome.run_id, None);
    assert!(vault.import_logs().unwrap().is_empty());
}

#[test]
fn a_read_only_vault_is_refused_before_anything_is_written() {
    let (vault_dir, vault) = empty_vault();
    fs::write(
        vault_dir.path().join(".notesapp/vault.yaml"),
        "format: 99\nid: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T\n",
    )
    .unwrap();
    let vault = Vault::open(vault.root()).unwrap();
    let env = TestEnv::new();
    let batch = batch(&case("reimport"), "first.json");
    let result = apply(&vault, plan(&vault, &batch, &env).unwrap(), &env);
    assert!(matches!(
        result,
        Err(schnipsle_import::ImportError::Log(
            schnipsle_core::ImportLogError::Vault(VaultError::ReadOnly(_))
        ))
    ));
    assert!(files(&vault_dir.path().join("notes")).is_empty());
}
