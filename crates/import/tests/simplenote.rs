// SPDX-License-Identifier: Apache-2.0
//! The Simplenote adapter on the sample export in
//! `tests/fixtures/import/simplenote/` (see the README there): the dry-run
//! report, the notes, the trash and the import log are compared with
//! `expected/`. Run with `SCHNIPSLE_BLESS=1` to write `expected/` from the
//! current output after a deliberate change.

mod support;

use std::fs;
use std::path::PathBuf;

use schnipsle_import::{PlannedAction, SkipReason, apply, plan, simplenote};
use support::{TestEnv, check, empty_vault, output};

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/import/simplenote")
}

#[test]
fn sample_export() {
    let (vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch = simplenote::read(&sample().join("simplenote-export.zip")).unwrap();
    let outcome = apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();
    // The trashed note is listed apart for the index.
    assert_eq!(outcome.written.len(), 8);
    assert_eq!(outcome.trashed, ["01M63HM9800000000000000009"]);
    check(&sample(), &output(vault_dir.path(), &outcome));
}

#[test]
fn the_zip_its_notes_json_and_the_unpacked_folder_read_alike() {
    let ids = |path: PathBuf| {
        let batch = simplenote::read(&path).unwrap();
        batch
            .notes
            .iter()
            .map(|n| (n.source_id.clone(), n.body.clone(), n.trashed))
            .collect::<Vec<_>>()
    };
    let zip = ids(sample().join("simplenote-export.zip"));
    assert_eq!(zip.len(), 9);
    assert_eq!(zip, ids(sample().join("notes.json")));
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("source")).unwrap();
    fs::copy(
        sample().join("notes.json"),
        dir.path().join("source/notes.json"),
    )
    .unwrap();
    assert_eq!(zip, ids(dir.path().to_owned()));
}

#[test]
fn a_second_import_of_the_same_export_changes_nothing() {
    let (_vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch = simplenote::read(&sample().join("notes.json")).unwrap();
    apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();
    let again = plan(&vault, &batch, &env).unwrap();
    assert_eq!(again.count(PlannedAction::Add), 0);
    assert_eq!(again.count(PlannedAction::Update), 0);
    // The trashed note is in the trash here, so it counts as deleted.
    let reasons: Vec<_> = again.items.iter().filter_map(|i| i.reason).collect();
    assert_eq!(reasons.len(), 9);
    assert_eq!(
        reasons
            .iter()
            .filter(|r| **r == SkipReason::Missing)
            .count(),
        1
    );
    assert!(
        reasons
            .iter()
            .all(|r| matches!(r, SkipReason::NotNewer | SkipReason::Missing))
    );
}

#[test]
fn a_note_trashed_in_simplenote_after_an_import_stays_here() {
    let (_vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let mut batch = simplenote::read(&sample().join("notes.json")).unwrap();
    apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();
    let lake = &mut batch.notes[0];
    lake.trashed = true;
    lake.modified = Some("2026-10-01T00:00:00.000Z".to_owned());
    let again = plan(&vault, &batch, &env).unwrap();
    let item = &again.items[0];
    assert_eq!(item.action, PlannedAction::Update);
    assert!(!item.trashed);
    assert!(item.problems[0].contains("in the trash in the source"));
    let outcome = apply(&vault, again, &env).unwrap();
    assert_eq!(outcome.written.len(), 1);
    assert!(
        vault
            .list_trash()
            .unwrap()
            .iter()
            .all(|t| t.title != "Lake trip")
    );
}
