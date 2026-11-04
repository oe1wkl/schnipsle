// SPDX-License-Identifier: Apache-2.0
//! Exporting notes (ADR-0010, ADR-0021) as a command, for File › Export… to
//! come. A PDF is printed on the main thread, which the export waits for
//! (ADR-0022), so this runs only from an `async` command.

use std::path::Path;

use schnipsle_export::{Exporter as _, Format, HtmlExporter, Input, Options, Report};
use tauri::{AppHandle, State};

use crate::{AppState, CmdResult, with_vault};

/// Exports `input` to `to`, a file or a folder as `options` say.
#[tauri::command]
pub async fn export_notes(
    app: AppHandle,
    state: State<'_, AppState>,
    format: Format,
    input: Input,
    options: Options,
    to: String,
) -> CmdResult<Report> {
    with_vault(&state, |vault| {
        let to = Path::new(&to);
        Ok(match format {
            Format::Html => HtmlExporter.export(vault, &input, &options, to)?,
            Format::Pdf => export_pdf(&app, vault, &input, options, to)?,
        })
    })
}

#[cfg(target_os = "macos")]
fn export_pdf(
    app: &AppHandle,
    vault: &schnipsle_core::vault::Vault,
    input: &Input,
    options: Options,
    to: &Path,
) -> CmdResult<Report> {
    let exporter = schnipsle_export::PdfExporter::new(printing::MainThreadPrinter(app.clone()));
    Ok(exporter.export(vault, input, &options, to)?)
}

#[cfg(not(target_os = "macos"))]
fn export_pdf(
    _app: &AppHandle,
    _vault: &schnipsle_core::vault::Vault,
    _input: &Input,
    _options: Options,
    _to: &Path,
) -> CmdResult<Report> {
    Err(crate::CommandError::other(
        "PDF export is not available on this platform yet",
    ))
}

#[cfg(target_os = "macos")]
mod printing {
    use std::io;
    use std::sync::mpsc;
    use std::time::Duration;

    use schnipsle_apple::{PdfPage, print_pdf};
    use schnipsle_export::{PrintJob, Printer};
    use tauri::AppHandle;

    /// How long a print may take before the export gives up; a page of 36 MB takes about 2 s.
    const LIMIT: Duration = Duration::from_secs(300);

    /// Prints on the main thread and waits for it on this one.
    pub struct MainThreadPrinter(pub AppHandle);

    impl Printer for MainThreadPrinter {
        fn print(&self, job: PrintJob) -> io::Result<Vec<u8>> {
            let page = PdfPage {
                html: job.html,
                width: job.width,
                height: job.height,
                margin: job.margin,
                title: job.title,
            };
            let (sender, receiver) = mpsc::channel();
            self.0
                .run_on_main_thread(move || {
                    print_pdf(page, move |result| {
                        let _ = sender.send(result);
                    });
                })
                .map_err(io::Error::other)?;
            receiver
                .recv_timeout(LIMIT)
                .map_err(|_| io::Error::other("Printing the PDF did not finish"))?
        }
    }
}
