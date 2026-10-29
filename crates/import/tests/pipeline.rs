// SPDX-License-Identifier: Apache-2.0
//! Golden-file tests of the import pipeline (format.md §12): each case in
//! `tests/fixtures/import/pipeline/` imports a batch into an empty vault and
//! compares the dry-run report, the notes, the stored attachments and the
//! import log with `expected/`. Run with `SCHNIPSLE_BLESS=1` to write
//! `expected/` from the current output after a deliberate change.

mod support;

use std::fs;
use std::path::{Path, PathBuf};

use schnipsle_core::{Vault, VaultError};
use schnipsle_import::{
    AttachmentData, Batch, PlannedAction, SkipReason, apply, apply_with_progress, plan,
};
use support::{TestEnv, check, empty_vault, files, output};

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

    let mut steps = Vec::new();
    let outcome = apply_with_progress(&vault, planned, &env, &mut |done, total| {
        steps.push((done, total));
    })
    .unwrap();
    assert_eq!(outcome.written.len(), 8);
    assert_eq!(steps, (1..=8).map(|done| (done, 8)).collect::<Vec<_>>());
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
