// SPDX-License-Identifier: Apache-2.0
//! Shared by the golden-file tests: a fixed environment, the output of a run
//! as files, and the comparison with `expected/` (`SCHNIPSLE_BLESS=1`
//! writes it instead).

use std::cell::Cell;
use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::Path;

use schnipsle_core::Vault;
use schnipsle_core::stamp::ulid;
use schnipsle_import::{Env, Outcome};

/// A fixed clock and ids counting up, so the output is the same every run.
pub struct TestEnv {
    pub now_ms: Cell<u64>,
    next: Cell<u64>,
}

impl TestEnv {
    pub fn new() -> Self {
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

/// Every file below `root`, by path relative to it.
pub fn files(root: &Path) -> BTreeMap<String, Vec<u8>> {
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

/// What a run produced, as files: `plan.json`, `notes/…`, `trash/…` (the
/// time of deletion, which is the real time, replaced by `<deleted>`),
/// `attachments.txt` (paths, without sidecars) and `import-log.yaml`
/// (snapshot names, which carry the real time, replaced by `<snapshot>`).
pub fn output(vault_dir: &Path, outcome: &Outcome) -> BTreeMap<String, Vec<u8>> {
    let mut out = BTreeMap::new();
    let plan = serde_json::to_string_pretty(&outcome.plan).unwrap() + "\n";
    out.insert("plan.json".to_owned(), plan.into_bytes());
    for (path, bytes) in files(&vault_dir.join("notes")) {
        out.insert(format!("notes/{path}"), bytes);
    }
    for (path, bytes) in files(&vault_dir.join(".notesapp/trash")) {
        let text = String::from_utf8(bytes).unwrap();
        let text = if Path::new(&path).extension().is_some_and(|e| e == "yaml") {
            let mut masked = String::new();
            for line in text.lines() {
                match line.split_once("deleted: ") {
                    Some((head, _)) => writeln!(masked, "{head}deleted: <deleted>"),
                    None => writeln!(masked, "{line}"),
                }
                .unwrap();
            }
            masked
        } else {
            text
        };
        out.insert(format!("trash/{path}"), text.into_bytes());
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

pub fn check(dir: &Path, actual: &BTreeMap<String, Vec<u8>>) {
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

pub fn empty_vault() -> (tempfile::TempDir, Vault) {
    let dir = tempfile::tempdir().unwrap();
    let vault = Vault::create(dir.path()).unwrap();
    (dir, vault)
}
