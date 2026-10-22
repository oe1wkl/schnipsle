// SPDX-License-Identifier: Apache-2.0
//! Writes generated test notes into a folder, e.g. a test vault's `notes/`
//! for trying the app with many notes:
//!
//! ```sh
//! cargo run -p schnipsle-core --release --example generate_notes -- <notes folder> [count] [seed]
//! ```

#[path = "../tests/support/generate.rs"]
mod generate;

fn main() {
    let mut args = std::env::args().skip(1);
    let Some(dir) = args.next() else {
        eprintln!("usage: generate_notes <notes folder> [count = 20000] [seed = 42]");
        std::process::exit(2);
    };
    let count = args.next().and_then(|a| a.parse().ok()).unwrap_or(20_000);
    let seed = args.next().and_then(|a| a.parse().ok()).unwrap_or(42);
    match generate::generate(dir.as_ref(), count, seed) {
        Ok(()) => println!("{count} notes written to {dir}"),
        Err(e) => {
            eprintln!("failed: {e}");
            std::process::exit(1);
        }
    }
}
