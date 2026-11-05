// SPDX-License-Identifier: Apache-2.0
//! Exporting notes (ADR-0010, ADR-0021) for File › Export…: where to, the
//! export, and showing the result. A PDF is printed on the main thread, which
//! the export waits for (ADR-0022), so this runs only from an `async` command.

use std::path::Path;

use schnipsle_export::{
    Exporter as _, Format, HtmlExporter, Input, Layout, OdtExporter, Options, Pandoc,
    PandocExporter, Report,
};
use serde::Serialize;
use tauri::{AppHandle, State};

use crate::{AppState, CmdResult, with_vault};

/// What the dialog can offer on this computer.
#[derive(Serialize)]
pub struct ExportChoices {
    formats: Vec<Format>,
    /// The version of the Pandoc found, on desktop (ADR-0010).
    pandoc: Option<String>,
}

/// The formats this computer can write. Looks for Pandoc each time, so one
/// installed meanwhile counts.
#[tauri::command]
pub async fn export_choices() -> ExportChoices {
    let mut formats = vec![Format::Html];
    if cfg!(target_os = "macos") {
        formats.push(Format::Pdf);
    }
    formats.push(Format::Odt);
    let pandoc = find_pandoc().map(|pandoc| pandoc.version().to_owned());
    if pandoc.is_some() {
        formats.extend([Format::Docx, Format::Epub]);
    }
    ExportChoices { formats, pandoc }
}

/// iOS and Android start no other programs.
fn find_pandoc() -> Option<Pandoc> {
    if cfg!(any(target_os = "ios", target_os = "android")) {
        None
    } else {
        Pandoc::find()
    }
}

/// Asks where the export goes: a file for one document, which the save dialog
/// asks about replacing, otherwise a folder. `None` if the user cancelled.
#[tauri::command]
pub async fn pick_export(
    app: AppHandle,
    format: Format,
    layout: Layout,
    title: Option<String>,
) -> CmdResult<Option<String>> {
    #[cfg(desktop)]
    {
        use tauri_plugin_dialog::DialogExt as _;
        let dialog = app.dialog().file();
        let picked = if layout == Layout::SingleFile {
            let (filter, extension) = (filter_name(format), format.extension());
            dialog
                .add_filter(filter, &[extension])
                .set_file_name(schnipsle_export::suggested_file_name(
                    title.as_deref(),
                    format,
                ))
                .blocking_save_file()
        } else {
            dialog
                .set_can_create_directories(true)
                .blocking_pick_folder()
        };
        let Some(picked) = picked else {
            return Ok(None);
        };
        let path = picked.into_path().map_err(crate::CommandError::other)?;
        Ok(Some(path.display().to_string()))
    }
    #[cfg(mobile)]
    {
        let _ = (app, format, layout, title);
        Err(crate::CommandError::other(
            "Export is available in Schnipsle on the Mac",
        ))
    }
}

#[cfg(desktop)]
const fn filter_name(format: Format) -> &'static str {
    match format {
        Format::Html => "HTML page",
        Format::Pdf => "PDF document",
        Format::Odt => "OpenDocument text",
        Format::Docx => "Word document",
        Format::Epub => "EPUB e-book",
    }
}

/// Shows an exported file in Finder, or opens the folder of an export.
#[tauri::command]
pub async fn reveal_export(path: String) -> CmdResult<()> {
    let path = Path::new(&path);
    if path.is_dir() {
        return crate::open_with_system(path);
    }
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .status()
            .map_err(crate::CommandError::other)?;
        Ok(())
    }
    #[cfg(not(target_os = "macos"))]
    {
        crate::open_with_system(path.parent().unwrap_or(path))
    }
}

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
        schnipsle_export::check_target(vault, to).map_err(|e| crate::CommandError {
            kind: "insideVault",
            ..crate::CommandError::other(e)
        })?;
        Ok(match format {
            Format::Html => HtmlExporter.export(vault, &input, &options, to)?,
            Format::Odt => OdtExporter.export(vault, &input, &options, to)?,
            Format::Pdf => export_pdf(&app, vault, &input, options, to)?,
            Format::Docx | Format::Epub => {
                let pandoc = find_pandoc()
                    .ok_or_else(|| crate::CommandError::other("Pandoc is not there any more"))?;
                PandocExporter::new(format, pandoc).export(vault, &input, &options, to)?
            }
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
