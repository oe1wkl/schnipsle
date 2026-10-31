// SPDX-License-Identifier: Apache-2.0
//! The HTML adapter on the sample pages in `tests/fixtures/import/html/`
//! (see the README there): the dry-run report, the notes, the attachments
//! and the import log are compared with `expected/`. Run with
//! `SCHNIPSLE_BLESS=1` to write `expected/` from the current output after a
//! deliberate change.

mod support;

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{Duration, UNIX_EPOCH};

use schnipsle_import::{ImportError, PlannedAction, SkipReason, apply, html, plan};
use support::{TestEnv, check, empty_vault, files, output};

fn sample() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/import/html")
}

/// The sample pages copied to `<temp>/Saved pages`, with a fixed time,
/// which a fresh checkout does not have.
struct Pages {
    _dir: tempfile::TempDir,
    path: PathBuf,
}

fn pages_copy() -> Pages {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Saved pages");
    for (name, bytes) in files(&sample().join("pages")) {
        let file = path.join(name);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, bytes).unwrap();
        touch(&file, 1_793_225_700); // 2026-10-28T22:15:00Z
    }
    Pages { _dir: dir, path }
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
fn sample_pages() {
    let pages = pages_copy();
    let (vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch = html::read(&pages.path).unwrap();
    let outcome = apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();
    check(&sample(), &output(vault_dir.path(), &outcome));
}

#[test]
fn a_single_file() {
    let pages = pages_copy();
    let batch = html::read(&pages.path.join("Recipes/Bread rolls.htm")).unwrap();
    assert_eq!(batch.input, "Bread rolls.htm");
    let [note] = &batch.notes[..] else {
        panic!("{:?}", batch.notes);
    };
    assert_eq!(note.source_id, "Bread rolls.htm");
    assert!(note.folders.is_empty());
    assert_eq!(note.title, "Bread rolls (Semmeln)");
    // The manual is outside the folder of the chosen file.
    assert_eq!(note.attachments.len(), 1);
    match html::read(&pages.path.join("Notes/readme.txt")) {
        Err(ImportError::Source(why)) => assert!(why.contains("not an HTML file"), "{why}"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_changed_page_is_updated_by_a_second_import() {
    let pages = pages_copy();
    let (_vault_dir, vault) = empty_vault();
    let env = TestEnv::new();
    let batch = html::read(&pages.path).unwrap();
    apply(&vault, plan(&vault, &batch, &env).unwrap(), &env).unwrap();

    let again = plan(&vault, &html::read(&pages.path).unwrap(), &env).unwrap();
    assert_eq!(again.count(PlannedAction::Add), 0);
    assert_eq!(again.count(PlannedAction::Update), 0);
    assert!(
        again
            .items
            .iter()
            .all(|i| i.reason == Some(SkipReason::NotNewer))
    );

    let changed = pages.path.join("Notes/Shopping.html");
    fs::write(&changed, "<p>Milk, eggs</p>\n").unwrap();
    touch(&changed, 1_793_300_000);
    let again = plan(&vault, &html::read(&pages.path).unwrap(), &env).unwrap();
    let updated: Vec<_> = again
        .items
        .iter()
        .filter(|i| i.action == PlannedAction::Update)
        .map(|i| i.source_id.as_str())
        .collect();
    assert_eq!(updated, ["Notes/Shopping.html"]);
}
