// SPDX-License-Identifier: Apache-2.0
//! Attachments (format.md §6) and the relative links to them (§5.5).
//!
//! An attachment is stored once per content, at
//! `attachments/<h0h1>/<sha256-hex>.<ext>`, written like a note (atomic,
//! coordinated, never replacing anything). The first time a content is added,
//! a sidecar `<file>.yaml` records the original file name and when it was
//! added. Notes link to it with a path relative to the note file, e.g.
//! `![Schematic](../attachments/3f/3f9a….png)`.
//!
//! [`Vault::read_linked_file`] resolves such a link back to the file, so the
//! editor can show images; it also serves images next to the note (folders
//! brought in from elsewhere) but never a file outside the vault or a hidden one.

use std::fs;
use std::io;
use std::path::Path;

use serde::Serialize;

use crate::flat_yaml;
use crate::layout::{ATTACHMENTS_DIR, NOTES_DIR};
use crate::stamp::{now_ms, rfc3339_utc};
use crate::vault::{
    Access, Result, TempFile, Vault, VaultError, accessor_ran_twice, placeholder_of, sha256_hex,
};

/// Longest extension kept; anything longer is not an extension.
const MAX_EXTENSION: usize = 10;

/// Extensions shown as images (`![…](…)`); other files get a plain link.
const IMAGE_EXTENSIONS: &[&str] = &[
    "apng", "avif", "bmp", "gif", "heic", "heif", "jpeg", "jpg", "png", "svg", "tif", "tiff",
    "webp",
];

/// An attachment just added, see [`Vault::add_attachment`]. Serialized with
/// camelCase names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AddedAttachment {
    /// Where it is stored, relative to the vault root.
    pub path: String,
    /// The link to it from the note, relative to the note file.
    pub link: String,
    /// Markdown to insert into the note: an image for image files, else a link.
    pub markdown: String,
}

impl Vault {
    /// Stores `bytes` as an attachment (format.md §6) for the note at
    /// `note_path` (relative to `notes/`) and returns the Markdown that links
    /// to it. `file_name` is the original name; its extension is kept, and
    /// the name becomes the link text. Content already stored is not written
    /// again.
    ///
    /// # Errors
    /// [`VaultError::ReadOnly`]; [`VaultError::InvalidPath`] for a bad note
    /// path or a file name without a usable extension; I/O errors.
    pub fn add_attachment(
        &self,
        note_path: &str,
        file_name: &str,
        bytes: &[u8],
    ) -> Result<AddedAttachment> {
        self.check_writable()?;
        self.resolve(note_path)?;
        let ext =
            extension_of(file_name).ok_or_else(|| VaultError::InvalidPath(file_name.to_owned()))?;
        let hash = sha256_hex(bytes);
        let path = format!("{ATTACHMENTS_DIR}/{}/{hash}.{ext}", &hash[..2]);
        let target = self.root().join(&path);
        if let Some(dir) = target.parent() {
            fs::create_dir_all(dir)?;
        }
        if self.place_new_file(&target, bytes)? {
            let sidecar = target.with_file_name(format!("{hash}.{ext}.yaml"));
            let record = format!(
                "name: {}\nadded: {}\n",
                flat_yaml::quote(file_name),
                rfc3339_utc(now_ms())
            );
            // Only metadata: the attachment itself is in place already.
            if let Err(e) = self.place_new_file(&sidecar, record.as_bytes()) {
                eprintln!("no metadata for {path}: {e}");
            }
        }
        let depth = note_path.matches('/').count() + 1; // + 1 for notes/
        let link = format!("{}{path}", "../".repeat(depth));
        let text = link_text(file_name);
        let markdown = if IMAGE_EXTENSIONS.contains(&ext.as_str()) {
            format!("![{text}]({link})")
        } else {
            format!("[{text}]({link})")
        };
        Ok(AddedAttachment {
            path,
            link,
            markdown,
        })
    }

    /// Reads the file a link in the note at `note_path` points to (format.md
    /// §5.5): a path relative to the note file, percent-encoded or in `<…>`,
    /// optionally with `?query` or `#fragment`.
    ///
    /// # Errors
    /// [`VaultError::InvalidPath`] for links that are absolute, URLs, leave the
    /// vault or reach a hidden file; [`VaultError::NotDownloaded`] for iCloud
    /// placeholders; I/O errors, e.g. if the file does not exist.
    pub fn read_linked_file(&self, note_path: &str, link: &str) -> Result<Vec<u8>> {
        let invalid = || VaultError::InvalidPath(link.to_owned());
        self.resolve(note_path)?;
        let link = link.trim();
        let link = link
            .strip_prefix('<')
            .and_then(|l| l.strip_suffix('>'))
            .unwrap_or(link);
        let link = link.split(['#', '?']).next().unwrap_or_default();
        let link = percent_decode(link).ok_or_else(invalid)?;
        if link.is_empty() || link.starts_with('/') || link.contains('\\') || link.contains(':') {
            return Err(invalid()); // absolute, a URL (`https:`, `data:`) or a Windows path
        }
        // Components below the vault root: start at the note's folder.
        let mut parts: Vec<&str> = std::iter::once(NOTES_DIR)
            .chain(note_path.split('/'))
            .collect();
        parts.pop(); // the note's file name
        for part in link.split('/') {
            match part {
                "" | "." => {}
                ".." => {
                    parts.pop().ok_or_else(invalid)?;
                }
                hidden if hidden.starts_with('.') => return Err(invalid()),
                name => parts.push(name),
            }
        }
        if parts.is_empty() {
            return Err(invalid());
        }
        let full = parts
            .iter()
            .fold(self.root().to_owned(), |path, part| path.join(part));
        if !full.exists() && placeholder_of(&full).exists() {
            return Err(VaultError::NotDownloaded(parts.join("/")));
        }
        self.coordinated(Access::Read, &full, |file| Ok(fs::read(file)?))
    }

    /// Puts `bytes` at `target` unless something is there already; `true` if
    /// it was written.
    fn place_new_file(&self, target: &Path, bytes: &[u8]) -> Result<bool> {
        if target.exists() {
            return Ok(false);
        }
        let mut tmp = Some(TempFile::write(target, bytes)?);
        let placed = self.coordinated(Access::Write, target, |file| {
            Ok(tmp.take().ok_or_else(accessor_ran_twice)?.place_new(file)?)
        });
        match placed {
            Ok(()) => Ok(true),
            // Stored meanwhile, e.g. by another device: the same content.
            Err(VaultError::Io(e)) if e.kind() == io::ErrorKind::AlreadyExists => Ok(false),
            Err(e) => Err(e),
        }
    }
}

/// The lowercased extension of `file_name` if it is a plausible one: ASCII
/// letters and digits only, at most 10 characters.
fn extension_of(file_name: &str) -> Option<String> {
    let name = file_name.rsplit(['/', '\\']).next().unwrap_or(file_name);
    let (stem, ext) = name.rsplit_once('.')?;
    let usable = !stem.is_empty()
        && !ext.is_empty()
        && ext.len() <= MAX_EXTENSION
        && ext.chars().all(|c| c.is_ascii_alphanumeric());
    usable.then(|| ext.to_ascii_lowercase())
}

/// `file_name` as link text: one line, with `\`, `[` and `]` escaped.
fn link_text(file_name: &str) -> String {
    let mut out = String::with_capacity(file_name.len());
    for c in file_name.chars() {
        match c {
            '\\' | '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out
}

/// Decodes `%XX` escapes; `None` for a broken escape or invalid UTF-8.
#[must_use]
pub fn percent_decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        if b == b'%' {
            let hex = std::str::from_utf8(bytes.get(i + 1..i + 3)?).ok()?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(b);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::VAULT_FILE;

    const PNG: &[u8] = b"\x89PNG fake image";

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        vault
            .create_note("Projekte/Antenne.md", "# Antenne\n")
            .unwrap();
        vault.create_note("top.md", "# Top\n").unwrap();
        (dir, vault)
    }

    #[test]
    fn stores_by_content_hash_with_a_relative_link() {
        let (dir, vault) = vault();
        let added = vault
            .add_attachment("Projekte/Antenne.md", "Schaltplan.PNG", PNG)
            .unwrap();
        let hash = sha256_hex(PNG);
        assert_eq!(added.path, format!("attachments/{}/{hash}.png", &hash[..2]));
        assert_eq!(added.link, format!("../../{}", added.path));
        assert_eq!(added.markdown, format!("![Schaltplan.PNG]({})", added.link));
        assert_eq!(fs::read(dir.path().join(&added.path)).unwrap(), PNG);

        let sidecar = fs::read_to_string(dir.path().join(format!("{}.yaml", added.path))).unwrap();
        assert_eq!(
            flat_yaml::value(&sidecar, "name").as_deref(),
            Some("Schaltplan.PNG")
        );
        assert!(flat_yaml::value(&sidecar, "added").is_some_and(|a| a.starts_with("20")));

        // The link resolves from the note it was made for.
        assert_eq!(
            vault
                .read_linked_file("Projekte/Antenne.md", &added.link)
                .unwrap(),
            PNG
        );
        let from_top = vault.add_attachment("top.md", "x.png", PNG).unwrap();
        assert_eq!(from_top.link, format!("../{}", added.path));
        assert_eq!(
            vault.read_linked_file("top.md", &from_top.link).unwrap(),
            PNG
        );
    }

    #[test]
    fn identical_content_is_stored_once() {
        let (dir, vault) = vault();
        let first = vault.add_attachment("top.md", "a.png", PNG).unwrap();
        let sidecar = dir.path().join(format!("{}.yaml", first.path));
        let record = fs::read_to_string(&sidecar).unwrap();
        let second = vault
            .add_attachment("top.md", "renamed copy.png", PNG)
            .unwrap();
        assert_eq!(second.path, first.path);
        assert_eq!(
            second.markdown,
            format!("![renamed copy.png]({})", first.link)
        );
        // The first name is kept; nothing was written again.
        assert_eq!(fs::read_to_string(&sidecar).unwrap(), record);
        let stored: Vec<_> = fs::read_dir(dir.path().join(&first.path).parent().unwrap())
            .unwrap()
            .collect();
        assert_eq!(stored.len(), 2); // the file and its sidecar, no temp files
    }

    #[test]
    fn other_files_get_a_plain_link_and_names_are_escaped() {
        let (_dir, vault) = vault();
        let pdf = vault
            .add_attachment("top.md", "Datenblatt [v2].pdf", b"%PDF")
            .unwrap();
        assert_eq!(pdf.path.rsplit_once('.').map(|(_, ext)| ext), Some("pdf"));
        assert_eq!(
            pdf.markdown,
            format!("[Datenblatt \\[v2\\].pdf]({})", pdf.link)
        );
    }

    #[test]
    fn needs_a_usable_extension_and_a_writable_vault() {
        let (dir, vault) = vault();
        for name in ["noext", ".png", "a.", "a.p-g", "a.waytoolongext", "a.pñg"] {
            assert!(
                matches!(
                    vault.add_attachment("top.md", name, PNG),
                    Err(VaultError::InvalidPath(_))
                ),
                "{name}"
            );
        }
        assert!(matches!(
            vault.add_attachment("../x.md", "a.png", PNG),
            Err(VaultError::InvalidPath(_))
        ));
        assert!(!dir.path().join(ATTACHMENTS_DIR).exists());

        fs::write(
            dir.path().join(VAULT_FILE),
            "format: 9\nid: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T\n",
        )
        .unwrap();
        let read_only = Vault::open(dir.path()).unwrap();
        assert!(matches!(
            read_only.add_attachment("top.md", "a.png", PNG),
            Err(VaultError::ReadOnly(_))
        ));
        assert!(!dir.path().join(ATTACHMENTS_DIR).exists());
    }

    #[test]
    fn links_resolve_relative_to_the_note() {
        let (dir, vault) = vault();
        let root = dir.path();
        fs::write(root.join("notes/Projekte/Bild 1.png"), "next to the note").unwrap();
        fs::create_dir_all(root.join("notes/Projekte/img")).unwrap();
        fs::write(root.join("notes/Projekte/img/a.svg"), "below").unwrap();
        let read = |link: &str| vault.read_linked_file("Projekte/Antenne.md", link);

        assert_eq!(read("Bild%201.png").unwrap(), b"next to the note");
        assert_eq!(read("<Bild 1.png>").unwrap(), b"next to the note");
        assert_eq!(read("./img/a.svg#frag").unwrap(), b"below");
        assert_eq!(
            read("img/../Bild%201.png?v=2").unwrap(),
            b"next to the note"
        );
        assert!(matches!(read("missing.png"), Err(VaultError::Io(_))));

        fs::write(root.join("notes/Projekte/.Wolke.png.icloud"), "plist").unwrap();
        assert!(matches!(
            read("Wolke.png"),
            Err(VaultError::NotDownloaded(_))
        ));
    }

    #[test]
    fn links_never_leave_the_vault_or_reach_hidden_files() {
        let (dir, vault) = vault();
        let outside = dir.path().parent().unwrap().join("outside.png");
        let _ = fs::write(&outside, "secret");
        for bad in [
            "../../../outside.png",
            "../../../../etc/passwd",
            "/etc/passwd",
            "https://example.com/a.png",
            "data:image/png;base64,AAAA",
            "file:///etc/passwd",
            "../../.notesapp/vault.yaml",
            ".hidden.png",
            "..\\..\\x.png",
            "%2e%2e/%2e%2e/%2e%2e/outside.png",
            "bad%zz.png",
            "",
        ] {
            assert!(
                matches!(
                    vault.read_linked_file("Projekte/Antenne.md", bad),
                    Err(VaultError::InvalidPath(_))
                ),
                "{bad}"
            );
        }
        let _ = fs::remove_file(outside);
    }

    #[test]
    fn percent_decoding() {
        assert_eq!(percent_decode("a%20b%C3%A4").as_deref(), Some("a bä"));
        assert_eq!(percent_decode("100%"), None);
        assert_eq!(percent_decode("%FF"), None);
    }
}
