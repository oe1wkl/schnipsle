// SPDX-License-Identifier: Apache-2.0
//! Makes a test vault of generated notes for measuring the app on a device
//! (N-03, ADR-0045). The text is invented and the same for a given seed:
//!
//! ```sh
//! cargo run -p schnipsle-core --release --example make-vault -- <folder> [notes = 10000] [seed = 42]
//! ```
//!
//! The folder is made if it does not exist; one that is not empty is refused,
//! so the command never mixes generated notes into a real vault.

use std::path::Path;
use std::process::ExitCode;
use std::time::Instant;

use schnipsle_core::Vault;

#[path = "../tests/support/generate.rs"]
mod generate;

fn main() -> ExitCode {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: make-vault <folder> [notes = 10000] [seed = 42]");
        return ExitCode::from(2);
    };
    let count = args.next().map_or(Ok(10_000), |a| a.parse());
    let seed = args.next().map_or(Ok(42), |a| a.parse());
    let (Ok(count), Ok(seed)) = (count, seed) else {
        eprintln!("usage: make-vault <folder> [notes = 10000] [seed = 42]");
        return ExitCode::from(2);
    };
    match make(Path::new(&dir), count, seed) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("make-vault: {e}");
            ExitCode::FAILURE
        }
    }
}

fn make(dir: &Path, count: usize, seed: u64) -> Result<(), Box<dyn std::error::Error>> {
    std::fs::create_dir_all(dir)?;
    // Finder's `.DS_Store` does not count; anything else might be someone's notes.
    if let Some(entry) = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .find(|e| e.file_name() != ".DS_Store")
    {
        return Err(format!(
            "{} is not empty (it holds {}); choose an empty folder",
            dir.display(),
            entry.file_name().to_string_lossy()
        )
        .into());
    }
    let vault = Vault::create(dir)?;
    let start = Instant::now();
    generate::generate(vault.notes_dir(), count, seed)?;
    println!(
        "{count} notes (seed {seed}) written to {} in {:.1} s",
        vault.notes_dir().display(),
        start.elapsed().as_secs_f64()
    );
    Ok(())
}
