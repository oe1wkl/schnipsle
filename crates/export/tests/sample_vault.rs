// SPDX-License-Identifier: Apache-2.0
//! The sample vault in `tests/fixtures/export/vault/` is as its README says,
//! so the exporters' golden files rest on known input (ADR-0021).

use std::fs;
use std::path::{Path, PathBuf};

use schnipsle_core::front_matter;
use schnipsle_core::tags::extract_tags;
use schnipsle_core::vault::{sha256_hex, title_of};

fn vault() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/export/vault")
}

fn read(path: &str) -> String {
    fs::read_to_string(vault().join("notes").join(path)).unwrap()
}

/// The notes in the order of the README's table, with their titles.
const NOTES: [(&str, &str); 6] = [
    ("welcome.md", "Welcome"),
    ("packing-list.md", "Packing list for Field Day"),
    ("Radio/antenna-tuner.md", "Antenna tuner"),
    ("Recipes/bread-rolls.md", "Bread rolls"),
    ("Songs/abendlied.md", "Abendlied"),
    ("raw-html.md", "Raw HTML"),
];

#[test]
fn notes_and_titles() {
    let mut found = Vec::new();
    collect(&vault().join("notes"), "", &mut found);
    found.sort();
    let mut expected: Vec<String> = NOTES.iter().map(|(p, _)| (*p).to_owned()).collect();
    expected.sort();
    assert_eq!(found, expected);
    for (path, title) in NOTES {
        let text = read(path);
        assert_eq!(title_of(&text, "x"), title, "{path}");
        assert!(front_matter::get(&text, "id").is_some(), "{path}");
    }
}

fn collect(dir: &Path, prefix: &str, out: &mut Vec<String>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().to_string_lossy().into_owned();
        if entry.path().is_dir() {
            collect(&entry.path(), &format!("{prefix}{name}/"), out);
        } else {
            out.push(format!("{prefix}{name}"));
        }
    }
}

/// The last line of the body, if it holds nothing but tags.
fn final_tag_line(text: &str) -> Option<String> {
    let last = front_matter::body(text).trim_end().lines().last()?.trim();
    let words = last.split(' ').count();
    (extract_tags(last).len() == words).then(|| last.to_owned())
}

#[test]
fn final_tag_lines() {
    let lines: Vec<_> = NOTES
        .iter()
        .map(|(p, _)| final_tag_line(&read(p)))
        .collect();
    assert_eq!(
        lines,
        [
            Some("#schnipsle #demo".to_owned()),
            None, // "Bring the #keyer and #m32." has words too
            Some("#Radio".to_owned()),
            Some("#Recipes #baking".to_owned()),
            None,
            None,
        ]
    );
    // Tags in the text, which stay text in an export.
    let tags: Vec<_> = extract_tags(&read("packing-list.md"))
        .into_iter()
        .map(|t| t.name)
        .collect();
    assert_eq!(tags, ["keyer", "m32"]);
}

#[test]
fn attachments_are_stored_by_their_hash_and_one_is_missing() {
    let mut missing = Vec::new();
    for (path, _) in NOTES {
        let text = read(path);
        let dir = vault()
            .join("notes")
            .join(path)
            .parent()
            .unwrap()
            .to_owned();
        for link in text.split("](").skip(1) {
            let dest = &link[..link.find(')').unwrap()];
            if !dest.contains("attachments/") || dest.starts_with("http") {
                continue;
            }
            let file = dir.join(dest);
            match fs::read(&file) {
                Ok(bytes) => {
                    let hash = sha256_hex(&bytes);
                    let name = file.file_stem().unwrap().to_string_lossy().into_owned();
                    assert_eq!(name, hash, "{dest}");
                }
                Err(_) => missing.push(format!("{path}: {dest}")),
            }
        }
    }
    assert_eq!(
        missing,
        ["Radio/antenna-tuner.md: ../../attachments/00/\
          0000000000000000000000000000000000000000000000000000000000000000.png"]
    );
}

#[test]
fn the_trash_holds_one_note() {
    let trash = vault().join(".notesapp/trash");
    let mut names: Vec<_> = fs::read_dir(trash)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    assert_eq!(
        names,
        [
            "01JC4X00000000000000000007.md",
            "01JC4X00000000000000000007.yaml"
        ]
    );
}
