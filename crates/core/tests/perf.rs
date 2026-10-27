// SPDX-License-Identifier: Apache-2.0
//! Performance with 20,000 notes (N-03: search under 200 ms for 10,000).
//!
//! Slow in a debug build, so it is ignored by default; CI runs it with
//! `cargo test -p schnipsle-core --release --test perf -- --ignored --nocapture`.

mod support;

use std::time::{Duration, Instant};

use schnipsle_core::{Index, SortOrder, Vault};

const NOTES: usize = 20_000;
const LIMIT: Duration = Duration::from_millis(200);

/// The slowest of three runs, so one lucky run does not hide a slow query.
fn slowest<T>(mut f: impl FnMut() -> T) -> (T, Duration) {
    let mut worst = Duration::ZERO;
    let mut result = None;
    for _ in 0..3 {
        let start = Instant::now();
        result = Some(f());
        worst = worst.max(start.elapsed());
    }
    (result.unwrap(), worst)
}

#[test]
#[ignore = "slow in debug builds; CI runs it with --release"]
fn twenty_thousand_notes() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join("vault")).unwrap();
    let vault = Vault::create(dir.path().join("vault")).unwrap();
    let start = Instant::now();
    support::generate::generate(vault.notes_dir(), NOTES, 42).unwrap();
    println!("generated {NOTES} notes in {:?}", start.elapsed());

    let mut index = Index::open(&dir.path().join("index.sqlite")).unwrap();
    let start = Instant::now();
    let report = vault.sync_index(&mut index).unwrap();
    println!("built the index in {:?}: {report:?}", start.elapsed());
    assert_eq!(report.indexed, NOTES);

    let start = Instant::now();
    let report = vault.sync_index(&mut index).unwrap();
    let unchanged = start.elapsed();
    println!("synced again, nothing changed, in {unchanged:?}");
    assert_eq!(report.indexed, 0);

    let mut slow = Vec::new();
    for query in [
        "antenne",
        "ant",
        "lötzinn gehäuse",
        "\"radio amateur\"",
        "antenne OR kirche OR gitarre",
        "#radio",
        "#projekte kabel -#archiv",
        "(spule OR dipol) #radio/cw",
        "-#radio",
    ] {
        let (hits, time) = slowest(|| index.search(query, 0).unwrap());
        println!("search {query:?}: {} hits, {time:?}", hits.len());
        if time > LIMIT {
            slow.push(format!("search {query:?}: {time:?}"));
        }
    }
    for keys in [
        vec![],
        vec!["radio".to_owned()],
        vec!["radio".into(), "projekte".into()],
    ] {
        let (folders, time) = slowest(|| index.tag_folders(&keys).unwrap());
        println!("tag folders {keys:?}: {} folders, {time:?}", folders.len());
        if time > LIMIT {
            slow.push(format!("tag folders {keys:?}: {time:?}"));
        }
        let (notes, time) = slowest(|| index.notes_in(&keys, SortOrder::Modified).unwrap());
        println!("notes in {keys:?}: {} notes, {time:?}", notes.len());
        if time > LIMIT {
            slow.push(format!("notes in {keys:?}: {time:?}"));
        }
    }
    assert!(slow.is_empty(), "slower than {LIMIT:?}: {slow:#?}");
}
