// SPDX-License-Identifier: Apache-2.0
//! Exporting notes (ADR-0010, ADR-0021) for File › Export…: where to, the
//! export, and showing the result. A PDF is printed on the main thread, which
//! the export waits for (ADR-0022), so this runs only from an `async` command.
//!
//! On iOS there is no save dialog: an export goes into a folder of the app's
//! own and is then handed to the share sheet, from which it can be saved to
//! Files, sent or printed. On macOS Share… does the same, with the share menu.
//! Several notes can also go into a zip archive, one file per note.

use std::path::{Path, PathBuf};

use schnipsle_export::{
    Format, HtmlExporter, Input, Layout, OdtExporter, Options, Pandoc, PandocExporter, Report,
    ScoreBlock, export_to,
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
    /// An export can be shared afterwards ([`share_export`]): on macOS and iOS.
    share: bool,
}

/// Whether an export is shared afterwards ([`share_export`]) rather than
/// written to a place picked first, and so started by buttons of its own
/// rather than from the File menu: on iOS.
#[tauri::command]
pub async fn exports_are_shared() -> bool {
    cfg!(mobile)
}

/// The formats this computer can write. Looks for Pandoc each time, so one
/// installed meanwhile counts.
#[tauri::command]
pub async fn export_choices() -> ExportChoices {
    let mut formats = vec![Format::Html];
    if cfg!(any(target_os = "macos", target_os = "ios")) {
        formats.push(Format::Pdf);
    }
    formats.push(Format::Odt);
    let pandoc = find_pandoc().map(|pandoc| pandoc.version().to_owned());
    if pandoc.is_some() {
        formats.extend([Format::Docx, Format::Epub]);
    }
    ExportChoices {
        formats,
        pandoc,
        share: cfg!(any(target_os = "macos", target_os = "ios")),
    }
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
/// asks about replacing, otherwise a folder. `None` if the user cancelled. To
/// be shared ([`share_export`] afterwards), as with `share` on macOS and always
/// on iOS, nothing is asked: the place is in a folder of the app's own.
#[tauri::command]
pub async fn pick_export(
    app: AppHandle,
    format: Format,
    layout: Layout,
    title: Option<String>,
    share: Option<bool>,
) -> CmdResult<Option<String>> {
    if share == Some(true) || cfg!(mobile) {
        let to =
            schnipsle_export::share_target(&share_base(&app)?, title.as_deref(), format, layout)
                .map_err(crate::CommandError::other)?;
        return Ok(Some(to.display().to_string()));
    }
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
        } else if layout == Layout::Zip {
            dialog
                .add_filter("Zip archive", &["zip"])
                .set_file_name(schnipsle_export::suggested_archive_name(title.as_deref()))
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
    unreachable!("exports are always shared on mobile")
}

/// The folder exports to be shared go to, emptied each time: in the app's own
/// temporary folder on iOS; on the desktop, where the temporary folder is
/// every app's, in the app's own caches.
fn share_base(app: &AppHandle) -> CmdResult<PathBuf> {
    if cfg!(mobile) {
        Ok(std::env::temp_dir().join("exports"))
    } else {
        use tauri::Manager as _;
        let caches = app
            .path()
            .app_cache_dir()
            .map_err(crate::CommandError::other)?;
        Ok(caches.join("exports"))
    }
}

/// Shows the share sheet for an export on iOS, and waits until it closes:
/// whether something was done with it. On macOS shows the share menu next to
/// `anchor` (left, top, width, height in the page), and returns at once.
#[tauri::command]
pub async fn share_export(
    app: AppHandle,
    path: String,
    anchor: Option<[f64; 4]>,
) -> CmdResult<bool> {
    #[cfg(target_os = "ios")]
    {
        use tauri_plugin_vault_access::VaultAccessExt as _;
        let _ = anchor;
        app.vault_access()
            .share(&path)
            .map_err(crate::CommandError::other)
    }
    #[cfg(target_os = "macos")]
    {
        let (sender, receiver) = std::sync::mpsc::channel();
        let anchor = anchor.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        app.run_on_main_thread(move || {
            let _ = sender.send(schnipsle_apple::share_files(Path::new(&path), anchor));
        })
        .map_err(crate::CommandError::other)?;
        receiver
            .recv()
            .map_err(crate::CommandError::other)?
            .map_err(crate::CommandError::other)?;
        Ok(true)
    }
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = (app, path, anchor);
        Err(crate::CommandError::other(
            "Sharing an export is not available on this platform",
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

/// The `abc` blocks of the notes of `input`, for the dialog to engrave before
/// it calls [`export_notes`] (ADR-0033).
#[tauri::command]
pub async fn export_scores(state: State<'_, AppState>, input: Input) -> CmdResult<Vec<ScoreBlock>> {
    with_vault(&state, |vault| {
        Ok(schnipsle_export::score_blocks(vault, &input))
    })
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
            Format::Html => export_to(&HtmlExporter, vault, &input, &options, to)?,
            Format::Odt => export_to(&OdtExporter, vault, &input, &options, to)?,
            Format::Pdf => export_pdf(&app, vault, &input, options, to)?,
            Format::Docx | Format::Epub => {
                let pandoc = find_pandoc()
                    .ok_or_else(|| crate::CommandError::other("Pandoc is not there any more"))?;
                let exporter = PandocExporter::new(format, pandoc);
                export_to(&exporter, vault, &input, &options, to)?
            }
        })
    })
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
fn export_pdf(
    app: &AppHandle,
    vault: &schnipsle_core::vault::Vault,
    input: &Input,
    options: Options,
    to: &Path,
) -> CmdResult<Report> {
    let exporter = schnipsle_export::PdfExporter::new(printing::MainThreadPrinter(app.clone()));
    Ok(export_to(&exporter, vault, input, &options, to)?)
}

#[cfg(not(any(target_os = "macos", target_os = "ios")))]
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

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod printing {
    use std::io;
    use std::sync::mpsc;
    use std::time::Duration;

    use schnipsle_apple::{PdfPage, print_pdf};
    #[cfg(target_os = "ios")]
    use schnipsle_export::{Anchor, Line, Place};
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

        /// The print path of iOS loses the links; they are found by their texts
        /// (ADR-0037). On this thread, as `PDFKit` needs no other.
        #[cfg(target_os = "ios")]
        fn find_anchors(
            &self,
            pdf: &[u8],
            anchors: &[Anchor],
        ) -> Option<io::Result<Vec<Option<Place>>>> {
            let texts: Vec<schnipsle_apple::AnchorText<'_>> = anchors
                .iter()
                .map(|anchor| schnipsle_apple::AnchorText {
                    before: &anchor.before,
                    text: &anchor.text,
                    moves_on: anchor.moves_on(),
                })
                .collect();
            let found = schnipsle_apple::find_anchors(pdf, &texts).map(|found| {
                found
                    .into_iter()
                    .map(|lines| {
                        lines.map(|lines| Place {
                            lines: lines
                                .into_iter()
                                .map(|line| Line {
                                    page: line.page,
                                    rect: line.rect,
                                })
                                .collect(),
                        })
                    })
                    .collect()
            });
            Some(found)
        }
    }
}
