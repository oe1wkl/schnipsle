// SPDX-License-Identifier: Apache-2.0
//! HTML export (ADR-0010): comrak's HTML for the tree of [`crate::document`], in
//! a page with a style sheet close to the editor's look, without scripts. A
//! note is a `<section>`, so print can start each on a new page.

use std::collections::HashSet;
use std::fmt::Write as _;
use std::fs;
use std::io;
use std::path::Path;

use comrak::Arena;
use comrak::nodes::{AstNode, NodeValue};
use schnipsle_core::front_matter;
use schnipsle_core::names::slug;
use schnipsle_core::tags::parse_options;
use schnipsle_core::vault::{Result, Vault, VaultError};

use crate::document::{self, Collected, Rules, Source};
use crate::output::{self, Names};
use crate::{Exporter, Format, Input, Layout, Options, Problem, ProblemKind, Report};

const THEME: &str = include_str!("theme.css");

comrak::create_formatter!(NoteHtml, {
    // The ☐ or ☑ is in the text (ADR-0021 rule 10), not a form control.
    NodeValue::TaskItem(_) => |context, entering| {
        if entering {
            context.cr()?;
            context.write_str("<li class=\"task\">")?;
        } else {
            context.write_str("</li>\n")?;
        }
    },
});

/// Exports notes as HTML.
#[derive(Debug, Clone, Copy, Default)]
pub struct HtmlExporter;

impl Exporter for HtmlExporter {
    fn format(&self) -> Format {
        Format::Html
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
                file_per_note(vault, &notes, *options, to, &mut report, &mut collected)?;
            }
            Layout::SingleFile | Layout::Folder => {
                one_document(
                    vault,
                    &notes,
                    input,
                    *options,
                    to,
                    &mut report,
                    &mut collected,
                )?;
            }
        }
        report.problems.append(&mut collected.problems);
        report.numbered.append(&mut collected.numbered);
        copy_attachments(vault, to, &collected, &mut report)?;
        Ok(report)
    }
}

/// One document of all `notes`.
fn one_document(
    vault: &Vault,
    notes: &[Source],
    input: &Input,
    options: Options,
    to: &Path,
    report: &mut Report,
    collected: &mut Collected,
) -> Result<()> {
    let single_file = options.layout == Layout::SingleFile;
    let one = one_page(vault, notes, input, options, !single_file, true, collected)?;
    if single_file {
        output::replace_file(to, one.page.as_bytes())?;
        let name = to.file_name().map(|n| n.to_string_lossy().into_owned());
        report.files.extend(name);
    } else {
        fs::create_dir_all(to)?;
        let mut names = Names::new(to);
        let name = names.free(&one.title, "html");
        collected.numbered.append(&mut names.numbered);
        output::write_new(&to.join(&name), one.page.as_bytes())?;
        report.files.push(name);
    }
    Ok(())
}

/// The page of one document, and what its metadata needs.
pub(crate) struct OnePage<'n> {
    pub title: String,
    /// The note it holds, if only one: a document of several has no dates of its own.
    pub note: Option<&'n Source>,
    pub page: String,
}

/// One page of all `notes`, with an anchor per note.
pub(crate) fn one_page<'n>(
    vault: &Vault,
    notes: &'n [Source],
    input: &Input,
    options: Options,
    copy_attachments: bool,
    web_pictures: bool,
    collected: &mut Collected,
) -> Result<OnePage<'n>> {
    let (anchors, rules) =
        document::rules_of_one_document(vault, notes, options, copy_attachments, web_pictures);
    let mut body = String::new();
    for (note, anchor) in notes.iter().zip(&anchors) {
        body.push_str(&section(note, anchor, &rules, collected)?);
    }
    let (title, note) = document::title_of_one_document(notes, input);
    let page = page(&title, note, &body);
    Ok(OnePage { title, note, page })
}

/// A document per note, linking to each other.
fn file_per_note(
    vault: &Vault,
    notes: &[Source],
    options: Options,
    to: &Path,
    report: &mut Report,
    collected: &mut Collected,
) -> Result<()> {
    fs::create_dir_all(to)?;
    let mut names = Names::new(to);
    let files: Vec<String> = notes
        .iter()
        .map(|note| names.free(&note.title, "html"))
        .collect();
    collected.numbered.append(&mut names.numbered);
    let rules = Rules {
        vault,
        hrefs: notes
            .iter()
            .zip(&files)
            .map(|(note, name)| (note.path.clone(), document::href_of(name)))
            .collect(),
        titles: document::titles_of(notes),
        out_of_reach: HashSet::new(),
        copy_attachments: true,
        web_pictures: true,
        keep_tag_line: options.keep_tag_line,
        sections: false,
    };
    for (note, name) in notes.iter().zip(files) {
        let page = page_of_note(note, &rules, collected)?;
        output::write_new(&to.join(&name), page.as_bytes())?;
        report.files.push(name);
    }
    Ok(())
}

/// The page of `note` alone.
pub(crate) fn page_of_note(
    note: &Source,
    rules: &Rules<'_>,
    collected: &mut Collected,
) -> Result<String> {
    let anchor = format!("note-{}", slug(&note.title));
    let body = section(note, &anchor, rules, collected)?;
    Ok(page(&note.title, Some(note), &body))
}

/// The HTML of `note`.
fn section(
    note: &Source,
    anchor: &str,
    rules: &Rules<'_>,
    collected: &mut Collected,
) -> Result<String> {
    let arena = Arena::new();
    let root = document::prepare(&arena, note, anchor, rules, collected);
    let mut out = format!("<section id=\"{}\">\n", escape(anchor));
    render(root, &mut out)?;
    out.push_str("</section>\n");
    Ok(out)
}

fn render<'a>(root: &'a AstNode<'a>, out: &mut String) -> Result<()> {
    let mut options = parse_options();
    // Links and raw HTML are clean by now (ADR-0021 rules 7 and 9), and `data:`
    // pictures must pass.
    options.render.r#unsafe = true;
    NoteHtml::format_document(root, &options, out)
        .map_err(|e| VaultError::Io(io::Error::other(e)))?;
    Ok(())
}

/// The whole page. The dates of `note`, the one it holds, go into its metadata.
fn page(title: &str, note: Option<&Source>, body: &str) -> String {
    let mut out = String::with_capacity(body.len() + THEME.len() + 256);
    out.push_str("<!doctype html>\n<html>\n<head>\n<meta charset=\"utf-8\">\n");
    out.push_str("<meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n");
    let _ = writeln!(out, "<title>{}</title>", escape(title));
    for (key, name) in [
        ("created", "dcterms.created"),
        ("modified", "dcterms.modified"),
    ] {
        if let Some(value) = note.and_then(|n| front_matter::get(&n.text, key)) {
            let _ = writeln!(out, "<meta name=\"{name}\" content=\"{}\">", escape(&value));
        }
    }
    let _ = writeln!(out, "<style>\n{THEME}</style>");
    let _ = writeln!(
        out,
        "</head>\n<body>\n<main>\n{body}</main>\n</body>\n</html>"
    );
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Copies the files the documents link to. One that cannot be copied is
/// reported; the link to it stays dead.
fn copy_attachments(
    vault: &Vault,
    to: &Path,
    collected: &Collected,
    report: &mut Report,
) -> Result<()> {
    for (target, copy) in &collected.copies {
        let file = target
            .split('/')
            .fold(to.to_owned(), |full, part| full.join(part));
        let copied = match vault.read_linked_file(&copy.path, &copy.link) {
            Ok(bytes) => output::copy_new(&file, &bytes)?,
            Err(_) => false,
        };
        if copied {
            report.files.push(target.clone());
        } else {
            report.problems.push(Problem {
                path: copy.path.clone(),
                title: copy.title.clone(),
                kind: ProblemKind::AttachmentNotIncluded,
                target: copy.link.clone(),
            });
        }
    }
    Ok(())
}
