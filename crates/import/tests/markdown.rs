// SPDX-License-Identifier: Apache-2.0
//! The Markdown adapter on the sample folder in
//! `tests/fixtures/import/markdown/` (see the README there): the dry-run
//! report, the notes, the attachments and the import log are compared with
//! `expected/`. Run with `SCHNIPSLE_BLESS=1` to write `expected/` from the
//! current output after a deliberate change.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use schnipsle_import::{
    ImportError, PlannedAction, SkipReason, apply, check_input, markdown, plan,
};
use support::{TestEnv, check, empty_vault, files, output};

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/import/markdown")
}

/// The sample folder copied to `<temp>/Old notes`, with a fixed time, which
/// a fresh checkout does not have.
struct Folder {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn folder_copy() -> Folder {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Old notes");
    for (name, bytes) in files(&sample().join("notes")) {
        let file = path.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, bytes).unwrap();
        touch(&file, 1_793_225_700); // 2026-10-28T22:15:00Z
    }
    Folder { _dir: dir, path }
}

fn touch(file: &Path, secs: u64) {
    fs::File::options()
        .write(true)
        .open(file)
        .unwrap()
        .set_modified(UNIX_EPOCH + Duration::from_secs(secs))
        .unwrap();
}

#[test]
fn sample_folder() {
    let folder = folder_copy();
    let (vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch = markdown::read(&folder.path).unwrap();
    let outcome = apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();
    check(&sample(), &output(vault_dir.path(), &outcome));
}

#[test]
fn a_single_file() {
    let folder = folder_copy();
    let batch = markdown::read(&folder.path.join("Songs/Abendlied.cho")).unwrap();
    assert_eq!(batch.input, "Abendlied.cho");
    let [note] = &batch.notes[..] else {
        panic!("{:?}", batch.notes);
    };
    assert_eq!(note.source_id, "Abendlied.cho");
    assert_eq!(note.title, "Abendlied");
    assert!(note.body.starts_with("# Abendlied\n\n````chordpro\n"));
    match markdown::read(&folder.path.join("readme.txt")) {
        Err(ImportError::Source(why)) => assert!(why.contains("not a Markdown file"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_changed_file_is_updated_by_a_second_import() {
    let folder = folder_copy();
    let (_vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch = markdown::read(&folder.path).unwrap();
    apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();

    let again = plan(&vault, &markdown::read(&folder.path).unwrap(), &env).unwrap();
    assert_eq!(again.count(PlannedAction::Add), 0);
    assert_eq!(again.count(PlannedAction::Update), 0);
    assert!(
        again
            .items
            .iter()
            .all(|i| i.reason == Some(SkipReason::NotNewer))
    );

    let changed = folder.path.join("Projects/Copied note.md");
    fs::write(&changed, "Changed since.\n").unwrap();
    touch(&changed, 1_793_300_000);
    let again = plan(&vault, &markdown::read(&folder.path).unwrap(), &env).unwrap();
    let updated: Vec<_> = again
        .items
        .iter()
        .filter(|i| i.action == PlannedAction::Update)
        .map(|i| i.source_id.as_str())
        .collect();
    assert_eq!(updated, ["Projects/Copied note.md"]);
}

#[test]
fn the_open_vault_is_no_source() {
    let folder = folder_copy();
    let (vault_dir, vault) = empty_vault();
    let notes = vault_dir.path().join("notes");
    fs::create_dir_all(&notes).unwrap();
    for inside in [vault_dir.path().to_path_buf(), notes] {
        assert!(matches!(
            check_input(&vault, &inside),
            Err(ImportError::Source(_))
        ));
    }
    let parent = vault_dir.path().parent().unwrap();
    assert!(matches!(
        check_input(&vault, parent),
        Err(ImportError::Source(_))
    ));
    assert!(check_input(&vault, &folder.path).is_ok());
}
