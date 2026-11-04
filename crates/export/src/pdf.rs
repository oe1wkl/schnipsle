// SPDX-License-Identifier: Apache-2.0
//! PDF export: the HTML of [`crate::html`], printed by the platform's web view
//! (ADR-0010, ADR-0022). The printing happens behind [`Printer`], as it depends
//! on the platform and its main thread; on macOS the app shell runs it with
//! `schnipsle_apple::print_pdf`.

use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::Path;

use schnipsle_core::vault::{Result, Vault};

use crate::document::{self, Collected, Rules, Source};
use crate::html;
use crate::output::{self, Names};
use crate::pdf_dates;
use crate::{Exporter, Format, Input, Layout, Options, Paper, Report};

/// Points per millimetre.
const MM: f64 = 72.0 / 25.4;

/// The margin on all sides of a page, in points.
const MARGIN: f64 = 20.0 * MM;

/// A page of HTML to print. Lengths are in points, 1/72 of an inch.
#[derive(Debug, Clone, PartialEq)]
pub struct PrintJob {
    /// The whole HTML page, with no scripts and nothing to load.
    pub html: String,
    /// The width of the paper.
    pub width: f64,
    /// The height of the paper.
    pub height: f64,
    /// The margin on all four sides.
    pub margin: f64,
    /// The title for the PDF's metadata.
    pub title: String,
}

/// Prints HTML to a PDF. Blocks until it is done, so an implementation must not
/// need the thread the export runs on.
pub trait Printer {
    /// The bytes of the PDF of `job`.
    ///
    /// # Errors
    /// If printing fails.
    fn print(&self, job: PrintJob) -> io::Result<Vec<u8>>;
}

/// Exports notes as PDF through a [`Printer`].
#[derive(Debug, Clone, Copy)]
pub struct PdfExporter<P> {
    printer: P,
}

impl<P: Printer> PdfExporter<P> {
    /// An exporter that prints with `printer`.
    pub fn new(printer: P) -> Self {
        Self { printer }
    }

    fn print(
        &self,
        html: String,
        title: &str,
        note: Option<&Source>,
        paper: Paper,
    ) -> io::Result<Vec<u8>> {
        let (width, height) = match paper {
            Paper::A4 => (210.0 * MM, 297.0 * MM),
            Paper::Letter => (612.0, 792.0),
        };
        let date = |key| note.and_then(|note| document::date_of(note, key));
        let mut pdf = self.printer.print(PrintJob {
            html,
            width,
            height,
            margin: MARGIN,
            title: title.to_owned(),
        })?;
        pdf_dates::set_dates(&mut pdf, date("created"), date("modified"));
        Ok(pdf)
    }

    /// One PDF of all `notes`, and its file name. Attachments never come along
    /// (ADR-0021 rule 6).
    fn one_document(
        &self,
        vault: &Vault,
        notes: &[Source],
        input: &Input,
        options: Options,
        to: &Path,
        collected: &mut Collected,
    ) -> Result<Option<String>> {
        let one = html::one_page(vault, notes, input, options, false, false, collected)?;
        let pdf = self.print(one.page, &one.title, one.note, options.paper)?;
        if options.layout == Layout::SingleFile {
            output::replace_file(to, &pdf)?;
            return Ok(to.file_name().map(|n| n.to_string_lossy().into_owned()));
        }
        fs::create_dir_all(to)?;
        let mut names = Names::new(to);
        let name = names.free(&one.title, "pdf");
        collected.numbered.append(&mut names.numbered);
        output::write_new(&to.join(&name), &pdf)?;
        Ok(Some(name))
    }

    /// A PDF per note. Unlike HTML pages they cannot link to each other: `WebKit`
    /// drops relative links, and `PDFKit` writes links to other files without the file.
    fn file_per_note(
        &self,
        vault: &Vault,
        notes: &[Source],
        options: Options,
        to: &Path,
        report: &mut Report,
        collected: &mut Collected,
    ) -> Result<()> {
        fs::create_dir_all(to)?;
        let rules = Rules {
            vault,
            hrefs: HashMap::new(),
            titles: document::titles_of(notes),
            out_of_reach: notes.iter().map(|n| n.path.clone()).collect(),
            copy_attachments: false,
            web_pictures: false,
            keep_tag_line: options.keep_tag_line,
            sections: false,
        };
        let mut names = Names::new(to);
        for note in notes {
            let page = html::page_of_note(note, &rules, collected)?;
            let pdf = self.print(page, &note.title, Some(note), options.paper)?;
            let name = names.free(&note.title, "pdf");
            output::write_new(&to.join(&name), &pdf)?;
            report.files.push(name);
        }
        collected.numbered.append(&mut names.numbered);
        Ok(())
    }
}

impl<P: Printer> Exporter for PdfExporter<P> {
    fn format(&self) -> Format {
        Format::Pdf
    }

    fn export(&self, vault: &Vault, input: &Input, options: &Options, to: &Path) -> Result<Report> {
        let mut report = Report::default();
        let notes = document::read_notes(vault, &input.notes, &mut report);
        if notes.is_empty() {
            return Ok(report);
        }
        let mut collected = Collected::default();
        match options.layout {
            Layout::FilePerNote => {
                self.file_per_note(vault, &notes, *options, to, &mut report, &mut collected)?;
            }
            Layout::SingleFile | Layout::Folder => {
                let name = self.one_document(vault, &notes, input, *options, to, &mut collected)?;
                report.files.extend(name);
            }
        }
        report.problems.append(&mut collected.problems);
        report.numbered.append(&mut collected.numbered);
        Ok(report)
    }
}
