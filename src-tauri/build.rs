// SPDX-License-Identifier: Apache-2.0
//! Generates Tauri's build-time context, and the list of the manual's files
//! the app carries (ADR-0051).

use std::fmt::Write as _;
use std::path::{Path, PathBuf};
use std::{env, fs};

fn main() {
    embed_manual();
    tauri_build::build();
}

/// Writes `$OUT_DIR/manual.rs`: `FILES`, the notes and pictures of
/// `docs/manual/` as `(path below the manual's root, bytes)`.
fn embed_manual() {
    let manifest = env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    let root = Path::new(&manifest).join("../docs/manual");
    println!("cargo:rerun-if-changed={}", root.display());
    let mut files = Vec::new();
    for top in ["notes", "attachments"] {
        collect(&root, &root.join(top), &mut files);
    }
    files.sort();
    let mut out = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (path, full) in files {
        let full = full.canonicalize().expect("a file just listed is there");
        // As a string literal, quoted and escaped.
        let full = full.to_string_lossy().into_owned();
        writeln!(out, "    ({path:?}, include_bytes!({full:?})),").expect("writing to a String");
    }
    out.push_str("];\n");
    let target = Path::new(&env::var("OUT_DIR").expect("cargo sets OUT_DIR")).join("manual.rs");
    fs::write(target, out).expect("OUT_DIR is writable");
}

/// The files below `dir`, hidden ones left out, with their paths below `root`.
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, PathBuf)>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return; // no pictures yet
    };
    for entry in entries {
        let entry = entry.expect("the manual's folder can be listed");
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let relative = path
                .strip_prefix(root)
                .expect("below the manual's root")
                .components()
                .map(|c| c.as_os_str().to_string_lossy().into_owned())
                .collect::<Vec<_>>()
                .join("/");
            files.push((relative, path));
        }
    }
}
