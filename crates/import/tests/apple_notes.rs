// SPDX-License-Identifier: Apache-2.0
//! The Apple Notes adapter on the sample export in
//! `tests/fixtures/import/apple-notes/export/` (see the README there): the
//! dry-run report, the notes, the attachments and the import log are
//! compared with `expected/`. Run with `SCHNIPSLE_BLESS=1` to write
//! `expected/` from the current output after a deliberate change.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use schnipsle_import::{ImportError, PlannedAction, SkipReason, apple_notes, apply, plan};
use support::{TestEnv, check, empty_vault, files, output};

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/import/apple-notes")
}

/// The sample export copied to `<temp>/Schnipsle-Import`, its files with a
/// fixed time, the time of the export, which a fresh checkout does not have.
struct Export {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn export_copy() -> Export {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Schnipsle-Import");
    let export_time = UNIX_EPOCH + Duration::from_secs(1_793_225_700); // 2026-10-28T22:15:00Z
    for (name, bytes) in files(&sample().join("export")) {
        let file = path.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, bytes).unwrap();
        fs::File::options()
            .write(true)
            .open(&file)
            .unwrap()
            .set_modified(export_time)
            .unwrap();
    }
    Export { _dir: dir, path }
}

fn import(export: &Path) -> (tempfile::TempDir, schnipsle_import::Outcome) {
    let (vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch = apple_notes::read(export).unwrap();
    let outcome = apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();
    (vault_dir, outcome)
}

#[test]
fn sample_export() {
    let export = export_copy();
    let (vault_dir, outcome) = import(&export.path);
    check(&sample(), &output(vault_dir.path(), &outcome));
}

#[test]
fn a_second_import_of_the_same_export_changes_nothing() {
    let export = export_copy();
    let (_vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch = apple_notes::read(&export.path).unwrap();
    apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();
    let again = plan(&vault, &batch, &env).unwrap();
    assert_eq!(again.count(PlannedAction::Add), 0);
    assert_eq!(again.count(PlannedAction::Update), 0);
    assert!(
        again
            .items
            .iter()
            .all(|i| i.reason == Some(SkipReason::NotNewer))
    );
}

#[test]
fn without_metadata_every_file_keeps_its_export_folder() {
    let export = export_copy();
    fs::remove_file(export.path.join(apple_notes::METADATA_FILE)).unwrap();
    let batch = apple_notes::read(&export.path).unwrap();
    assert_eq!(batch.notes.len(), 10);
    for note in &batch.notes {
        assert!(note.source_id.starts_with("export:"), "{}", note.source_id);
        assert_eq!(note.modified.as_deref(), Some("2026-10-28T22:15:00Z"));
        assert_eq!(note.folders.len(), 1);
    }
    // One notice for the whole import, not one per file.
    assert_eq!(batch.notices.len(), 1);
    assert!(batch.notices[0].message.contains("is missing"));
}

#[test]
fn damaged_or_newer_metadata_is_refused() {
    let export = export_copy();
    let file = export.path.join(apple_notes::METADATA_FILE);
    for (text, why) in [
        ("{\"format\": 2, \"notes\": []}", "format 2"),
        ("{\"format\": 1, \"notes\": [", "damaged"),
        ("{\"notes\": []}", "no format"),
    ] {
        fs::write(&file, text).unwrap();
        match apple_notes::read(&export.path) {
            Err(ImportError::Source(message)) => assert!(message.contains(why), "{message}"),
            other => panic!("{text}: {other:?}"),
        }
    }
}
