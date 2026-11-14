// SPDX-License-Identifier: Apache-2.0
//! Spike: PDF on iOS (`docs/spikes/ios-pdf.md`). Not for release.
//!
//! Started with the environment variable `SCHNIPSLE_SPIKE_PDF` (through
//! `xcrun devicectl device process launch --environment-variables`), it copies the
//! invented export samples into a temporary vault, exports them as PDF through the
//! iOS print path, and writes the PDFs and `results.json` to
//! `Documents/spike-pdf/` in the app's container, to be copied to the Mac.

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use schnipsle_core::vault::Vault;
use schnipsle_export::{
    Exporter as _, Input, Layout, Options, Paper, PdfExporter, PrintJob, Printer, Score,
};
use tauri::AppHandle;

use crate::exporting::printing::MainThreadPrinter;

/// The sample vault (`tests/fixtures/export/vault`), whose text is invented.
const VAULT: &[(&str, &[u8])] = &[
    (
        ".notesapp/vault.yaml",
        include_bytes!("../../tests/fixtures/export/vault/.notesapp/vault.yaml"),
    ),
    (
        "notes/welcome.md",
        include_bytes!("../../tests/fixtures/export/vault/notes/welcome.md"),
    ),
    (
        "notes/packing-list.md",
        include_bytes!("../../tests/fixtures/export/vault/notes/packing-list.md"),
    ),
    (
        "notes/raw-html.md",
        include_bytes!("../../tests/fixtures/export/vault/notes/raw-html.md"),
    ),
    (
        "notes/Songs/abendlied.md",
        include_bytes!("../../tests/fixtures/export/vault/notes/Songs/abendlied.md"),
    ),
    (
        "notes/Radio/antenna-tuner.md",
        include_bytes!("../../tests/fixtures/export/vault/notes/Radio/antenna-tuner.md"),
    ),
    (
        "notes/Recipes/bread-rolls.md",
        include_bytes!("../../tests/fixtures/export/vault/notes/Recipes/bread-rolls.md"),
    ),
    (
        "attachments/bb/bb1b9c77caef6638052849663c12d4a60bba4fee3c4ce047822ba11c00cc7974.png",
        include_bytes!(
            "../../tests/fixtures/export/vault/attachments/bb/bb1b9c77caef6638052849663c12d4a60bba4fee3c4ce047822ba11c00cc7974.png"
        ),
    ),
    (
        "attachments/d0/d009639f2187c44b0fa8838f659b03ac0d0a54cbfcda6b36ae9c54c2e564d06f.pdf",
        include_bytes!(
            "../../tests/fixtures/export/vault/attachments/d0/d009639f2187c44b0fa8838f659b03ac0d0a54cbfcda6b36ae9c54c2e564d06f.pdf"
        ),
    ),
    (
        "attachments/e2/e24f615689c51a90fce260edbb6b16e9e27db8ce9a60ba360c189e1e7ded433f.jpg",
        include_bytes!(
            "../../tests/fixtures/export/vault/attachments/e2/e24f615689c51a90fce260edbb6b16e9e27db8ce9a60ba360c189e1e7ded433f.jpg"
        ),
    ),
];

const NOTES: [&str; 6] = [
    "welcome.md",
    "packing-list.md",
    "Radio/antenna-tuner.md",
    "Recipes/bread-rolls.md",
    "Songs/abendlied.md",
    "raw-html.md",
];

/// The print job of the golden file `pdf/expected/one-document/all.html`.
const ALL_HTML: &str =
    include_str!("../../tests/fixtures/export/pdf/expected/one-document/all.html");

const KANON: &str = include_str!("../../tests/fixtures/export/scores/kanon.svg");

/// Starts the spike when the environment asks for it.
pub fn start(app: AppHandle) {
    if std::env::var_os("SCHNIPSLE_SPIKE_PDF").is_none() {
        return;
    }
    std::thread::spawn(move || {
        // Let the window come up first, as an export from the dialog would.
        std::thread::sleep(Duration::from_secs(3));
        let log = run(&app);
        eprintln!("spike-pdf: {log}");
    });
}

fn out_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map_or_else(std::env::temp_dir, PathBuf::from);
    home.join("Documents/spike-pdf")
}

fn run(app: &AppHandle) -> String {
    let out = out_dir();
    let _ = fs::remove_dir_all(&out);
    if let Err(e) = fs::create_dir_all(&out) {
        return format!("no output folder: {e}");
    }
    let mut log = String::new();
    let vault_dir = std::env::temp_dir().join("spike-pdf-vault");
    let _ = fs::remove_dir_all(&vault_dir);
    for (path, bytes) in VAULT {
        let file = vault_dir.join(path);
        let _ = fs::create_dir_all(file.parent().unwrap_or(&vault_dir));
        if let Err(e) = fs::write(&file, bytes) {
            let _ = writeln!(log, "write {path}: {e}");
        }
    }
    let printer = MainThreadPrinter(app.clone());

    // The printer alone, with the golden page: what the print path gives.
    let raw = |name: &str, html: &str, paper: (f64, f64), margin: f64| -> String {
        let start = Instant::now();
        let result = printer.print(PrintJob {
            html: html.to_owned(),
            width: paper.0,
            height: paper.1,
            margin,
            title: "Samples".to_owned(),
        });
        let took = start.elapsed().as_secs_f64();
        match result {
            Ok(pdf) => {
                let _ = fs::write(out.join(name), &pdf);
                format!("{name}: {} bytes in {took:.2} s", pdf.len())
            }
            Err(e) => format!("{name}: failed after {took:.2} s: {e}"),
        }
    };
    let _ = writeln!(
        log,
        "{}",
        raw("raw-a4.pdf", ALL_HTML, (595.28, 841.89), 56.69)
    );
    let _ = writeln!(
        log,
        "{}",
        raw("raw-letter-10mm.pdf", ALL_HTML, (612.0, 792.0), 28.35)
    );
    let _ = writeln!(
        log,
        "{}",
        raw("stress.pdf", &stress(), (595.28, 841.89), 56.69)
    );
    for (name, method) in [
        ("method-renderer.pdf", schnipsle_apple::Method::Renderer),
        ("method-createpdf.pdf", schnipsle_apple::Method::CreatePdf),
    ] {
        let _ = writeln!(log, "{}", by_method(app, &out, name, method));
    }

    // The exporter, as the dialog would call it.
    match Vault::open(&vault_dir) {
        Ok(vault) => {
            let exporter = PdfExporter::new(MainThreadPrinter(app.clone()));
            for (name, layout, contents, page_numbers) in [
                ("toc-a4.pdf", Layout::SingleFile, true, true),
                ("plain-a4.pdf", Layout::SingleFile, false, false),
                ("per-note", Layout::FilePerNote, false, true),
            ] {
                let options = Options {
                    layout,
                    keep_tag_line: false,
                    abc_source: false,
                    paper: Paper::A4,
                    contents,
                    page_numbers,
                };
                let start = Instant::now();
                let to = out.join(name);
                let result = exporter.export(&vault, &input(), &options, &to);
                let took = start.elapsed().as_secs_f64();
                let _ = match result {
                    Ok(report) => writeln!(
                        log,
                        "{name}: {:?} in {took:.2} s, warnings {:?}",
                        report.files, report.warnings
                    ),
                    Err(e) => writeln!(log, "{name}: failed after {took:.2} s: {e}"),
                };
            }
        }
        Err(e) => {
            let _ = writeln!(log, "vault: {e}");
        }
    }
    let _ = fs::write(out.join("results.txt"), &log);
    log
}

/// The golden page on A4 by another `method`.
fn by_method(
    app: &AppHandle,
    out: &std::path::Path,
    name: &str,
    method: schnipsle_apple::Method,
) -> String {
    let page = schnipsle_apple::PdfPage {
        html: ALL_HTML.to_owned(),
        width: 595.28,
        height: 841.89,
        margin: 56.69,
        title: "Samples".to_owned(),
    };
    let (sender, receiver) = std::sync::mpsc::channel();
    let start = Instant::now();
    if app
        .run_on_main_thread(move || {
            schnipsle_apple::print_pdf_with(page, method, move |result| {
                let _ = sender.send(result);
            });
        })
        .is_err()
    {
        return format!("{name}: not started");
    }
    let result = receiver.recv_timeout(Duration::from_secs(60));
    let took = start.elapsed().as_secs_f64();
    match result {
        Ok(Ok(pdf)) => {
            let _ = fs::write(out.join(name), &pdf);
            format!("{name}: {} bytes in {took:.2} s", pdf.len())
        }
        Ok(Err(e)) => format!("{name}: failed after {took:.2} s: {e}"),
        Err(_) => format!("{name}: no answer after {took:.2} s"),
    }
}

fn input() -> Input {
    Input {
        notes: NOTES.iter().map(|p| (*p).to_owned()).collect(),
        name: Some("Samples".to_owned()),
        scores: vec![Score {
            source: "X:1\nT:Kanon\nM:4/4\nL:1/4\nK:D\nf2 e2 | d2 c2 |\n".to_owned(),
            transpose: 0,
            tunes: vec![KANON.to_owned()],
        }],
    }
}

/// 300 short sections, each on a page of its own, a wide table, a long word and
/// a long code block: what made the macOS spike shrink or cut off pages.
fn stress() -> String {
    let mut html = String::from(
        "<!doctype html><html><head><meta charset=\"utf-8\"><style>\
         section + section { break-before: page }\
         @media print { p, li, td, th, h1 { overflow-wrap: anywhere } table { max-width: 100% } }\
         </style></head><body>",
    );
    html.push_str("<section><h1>Wide</h1><table><tr>");
    for i in 0..14 {
        let _ = write!(html, "<td>column number {i} with words</td>");
    }
    html.push_str("</tr></table><p>");
    html.push_str(&"Donaudampfschifffahrtsgesellschaft".repeat(8));
    html.push_str("</p><pre>");
    for i in 0..200 {
        let _ = writeln!(html, "line {i} of a long code block");
    }
    html.push_str("</pre></section>");
    for i in 0..300 {
        let _ = write!(
            html,
            "<section id=\"s{i}\"><h1>Section {i}</h1><p>Short text. <a href=\"#s0\">Back to the start</a>, \
             <a href=\"https://example.org/{i}\">a web link</a>.</p></section>"
        );
    }
    html.push_str("</body></html>");
    html
}
