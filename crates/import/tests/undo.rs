// SPDX-License-Identifier: Apache-2.0
//! Undoing import runs (format.md §12.4) on the two exports in
//! `tests/fixtures/import/pipeline/reimport/`: the first adds notes A to F,
//! the second updates A and adds E.

#[allow(dead_code)] // only the environment and the empty vault are used here
mod support;

use std::fs;
use std::path::PathBuf;

use schnipsle_core::{Vault, VaultError, front_matter};
use schnipsle_import::{
    Batch, ImportError, Outcome, PlannedAction, SkipReason, UndoAction, apply, plan, plan_undo,
    runs, undo,
};
use support::{TestEnv, empty_vault};

fn batch(file: &str) -> Batch {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/fixtures/import/pipeline/reimport");
    serde_json::from_str(&fs::read_to_string(dir.join(file)).unwrap()).unwrap()
}

fn import(vault: &Vault, file: &str, env: &TestEnv) -> Outcome {
    apply(vault, plan(vault, &batch(file), env).unwrap(), env).unwrap()
}

/// The first import, B edited and F deleted here, then the second import a
/// day later: as in the pipeline's re-import test.
struct Imported {
    dir: tempfile::TempDir,
    vault: Vault,
    env: TestEnv,
    first: Outcome,
    second: Outcome,
}

impl Imported {
    fn new() -> Self {
        let (dir, vault) = empty_vault();
        let env = TestEnv::new();
        let first = import(&vault, "first.json", &env);
        let b = vault.read_note(&path_of(&first, "Note B")).unwrap();
        vault
            .replace_note_as_is(&b.path, &format!("{}Edited here.\n", b.text), &b.hash)
            .unwrap();
        fs::remove_file(vault.note_file(&path_of(&first, "Note F")).unwrap()).unwrap();
        env.now_ms.set(env.now_ms.get() + 86_400_000);
        let second = import(&vault, "second.json", &env);
        Self {
            dir,
            vault,
            env,
            first,
            second,
        }
    }

    fn path_of(&self, title: &str) -> String {
        path_of(&self.first, title)
    }
}

fn path_of(outcome: &Outcome, title: &str) -> String {
    let item = outcome
        .plan
        .items
        .iter()
        .find(|i| i.title == title)
        .unwrap();
    item.path.clone().unwrap()
}

fn run_id(outcome: &Outcome) -> String {
    outcome.run_id.clone().unwrap()
}

fn actions(undo: &schnipsle_import::Undo) -> Vec<(String, UndoAction)> {
    undo.items
        .iter()
        .map(|i| (i.title.clone(), i.action))
        .collect()
}

#[test]
fn runs_newest_first() {
    let t = Imported::new();
    let listed = runs(&t.vault).unwrap();
    let ids: Vec<_> = listed.iter().map(|r| r.id.clone()).collect();
    assert_eq!(ids, [run_id(&t.second), run_id(&t.first)]);
    assert_eq!((listed[0].added, listed[0].updated), (1, 1));
    assert_eq!((listed[1].added, listed[1].updated), (5, 0));
    assert_eq!(listed[0].source, "simplenote");
    assert!(
        listed
            .iter()
            .all(|r| r.undone.is_none() && r.problem.is_none())
    );
}

#[test]
fn undo_the_second_import_then_the_first() {
    let t = Imported::new();
    let vault = &t.vault;
    let a = t.path_of("Note A");

    // The dry run of the second: E goes, A gets its first version back.
    let second = run_id(&t.second);
    let planned = plan_undo(vault, &second).unwrap();
    let expected = [
        ("Note E".to_owned(), UndoAction::Trash),
        ("Note A".to_owned(), UndoAction::Restore),
    ];
    assert_eq!(actions(&planned), expected);
    assert!(vault.list_trash().unwrap().is_empty());

    let done = undo(vault, &second, &t.env).unwrap();
    assert_eq!(actions(&done), expected);
    assert!(done.run.undone.is_some());
    let trashed = vault.list_trash().unwrap();
    assert_eq!(trashed.len(), 1);
    assert_eq!(done.items[0].trash_key.as_deref(), Some(&*trashed[0].key));
    // A is again exactly as the first import wrote it, and the version the
    // undo replaced is in its history.
    let text = vault.read_note(&a).unwrap().text;
    assert!(text.contains("First version."));
    let id = front_matter::get(&text, "id").unwrap();
    let history = vault.snapshots(&id).unwrap();
    let last = vault.read_snapshot(&id, history.last().unwrap()).unwrap();
    assert!(last.contains("Second version from the source."));

    // An undone run cannot be undone again.
    for result in [
        plan_undo(vault, &second).map(|_| ()),
        undo(vault, &second, &t.env).map(|_| ()),
    ] {
        assert!(matches!(result, Err(ImportError::Source(why)) if why.contains("undone already")));
    }

    // The first: B was edited here and stays; F is gone already.
    let first = run_id(&t.first);
    let done = undo(vault, &first, &t.env).unwrap();
    assert_eq!(
        actions(&done),
        [
            ("note-f".to_owned(), UndoAction::Missing), // by its file name
            ("Note D".to_owned(), UndoAction::Trash),
            ("Note C".to_owned(), UndoAction::Trash),
            ("Note B".to_owned(), UndoAction::Keep),
            ("Note A".to_owned(), UndoAction::Trash),
        ]
    );
    assert!(
        done.items[3]
            .problem
            .as_deref()
            .unwrap()
            .contains("Changed here")
    );
    assert_eq!(vault.list_trash().unwrap().len(), 4);
    let (_, log) = vault
        .import_logs()
        .unwrap()
        .into_iter()
        .find(|(name, _)| name.starts_with(&first))
        .unwrap();
    let kept: Vec<_> = log.unwrap().entries().iter().map(|e| e.kept).collect();
    assert_eq!(kept, [false, true, false, false, false]);

    // Importing the first export again adds the notes back, but not B: its
    // kept entry still counts (§12.3 rule 1).
    let again = plan(vault, &batch("first.json"), &t.env).unwrap();
    let reasons: Vec<_> = again.items.iter().map(|i| (i.action, i.reason)).collect();
    assert_eq!(
        reasons,
        [
            (PlannedAction::Add, None),
            (PlannedAction::Skip, Some(SkipReason::ChangedHere)),
            (PlannedAction::Add, None),
            (PlannedAction::Add, None),
            (PlannedAction::Add, None),
        ]
    );
}

#[test]
fn a_version_no_longer_in_the_history_keeps_the_note() {
    let t = Imported::new();
    let text = t.vault.read_note(&t.path_of("Note A")).unwrap().text;
    let id = front_matter::get(&text, "id").unwrap();
    for name in t.vault.snapshots(&id).unwrap() {
        let file = t.dir.path().join(".notesapp/history").join(&id).join(name);
        fs::remove_file(file).unwrap();
    }
    let done = undo(&t.vault, &run_id(&t.second), &t.env).unwrap();
    assert_eq!(done.items[1].action, UndoAction::Keep);
    assert!(
        done.items[1]
            .problem
            .as_deref()
            .unwrap()
            .contains("not in the history")
    );
    assert_eq!(done.items[0].action, UndoAction::Trash);
    assert!(text.contains("Second version"));
    assert_eq!(t.vault.read_note(&t.path_of("Note A")).unwrap().text, text);
}

#[test]
fn a_read_only_vault_is_left_alone() {
    let t = Imported::new();
    let root = t.dir.path();
    let vault_file = root.join(".notesapp/vault.yaml");
    let yaml = fs::read_to_string(&vault_file)
        .unwrap()
        .replace("format: 1", "format: 9");
    fs::write(&vault_file, yaml).unwrap();
    let read_only = Vault::open(root).unwrap();
    assert!(read_only.read_only().is_some());
    let second = run_id(&t.second);
    // The dry run still shows what would happen.
    assert_eq!(plan_undo(&read_only, &second).unwrap().items.len(), 2);
    assert!(matches!(
        undo(&read_only, &second, &t.env),
        Err(ImportError::Vault(VaultError::ReadOnly(_)))
    ));
    assert!(read_only.list_trash().unwrap().is_empty());
    assert!(runs(&read_only).unwrap()[0].undone.is_none());
}

#[test]
fn unknown_runs_are_refused() {
    let t = Imported::new();
    assert!(matches!(
        plan_undo(&t.vault, "01M6000000000000000000000Z"),
        Err(ImportError::Source(_))
    ));
}
