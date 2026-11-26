// SPDX-License-Identifier: Apache-2.0
//! Spike: the app's iCloud container for the default vault (ADR-0048,
//! `docs/spikes/icloud-container.md`). Settings › Diagnostics runs it on the
//! spike branch; it is removed after the spike.
//!
//! The report gives, with times since it started: whether the device has an
//! iCloud account, the container's folder as the system gives it (and on the
//! Mac also by its path), what is in its `Documents`, and on iOS what
//! `NSMetadataQuery` lists there. `make` writes a test vault
//! `Schnipsle-Spike` with one note there, as the app writes; `remove` deletes
//! it again.

use crate::CmdResult;

/// Runs the probe; `action` is `report`, `make` or `remove`.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
pub async fn icloud_probe(app: tauri::AppHandle, action: String) -> CmdResult<String> {
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    return Ok(apple::probe(&app, &action));
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    {
        let _ = (app, action);
        Ok("The iCloud probe runs on macOS and iOS only.".to_owned())
    }
}

#[cfg(any(target_os = "macos", target_os = "ios"))]
mod apple {
    use std::fs;
    use std::path::{Path, PathBuf};
    use std::time::{Instant, SystemTime, UNIX_EPOCH};

    use schnipsle_core::Vault;
    use tauri::Manager as _;

    const CONTAINER: &str = "iCloud.cc.kraml.schnipsle";
    const CONTAINER_FOLDER: &str = "iCloud~cc~kraml~schnipsle";
    const TEST_FOLDER: &str = "Schnipsle-Spike";
    /// `SF_DATALESS` of `<sys/stat.h>`: only in iCloud, not downloaded.
    const SF_DATALESS: u32 = 0x4000_0000;

    struct Report {
        start: Instant,
        lines: Vec<String>,
    }

    impl Report {
        fn line(&mut self, text: impl AsRef<str>) {
            let ms = self.start.elapsed().as_secs_f64() * 1000.0;
            self.lines.push(format!("{ms:>8.1} ms  {}", text.as_ref()));
        }
    }

    pub(super) fn probe(app: &tauri::AppHandle, action: &str) -> String {
        let mut r = Report {
            start: Instant::now(),
            lines: Vec::new(),
        };
        r.line(format!(
            "iCloud probe: {action}, {} {}, app {}",
            std::env::consts::OS,
            schnipsle_apple::system_version().unwrap_or_default(),
            app.package_info().version
        ));
        r.line(format!(
            "iCloud account (ubiquityIdentityToken): {}",
            schnipsle_apple::has_icloud_account()
        ));
        let asked = Instant::now();
        let container = schnipsle_apple::ubiquity_container(CONTAINER);
        r.line(format!(
            "container from the system ({:.1} ms): {}",
            asked.elapsed().as_secs_f64() * 1000.0,
            container
                .as_ref()
                .map_or_else(|| "none".to_owned(), |p| p.display().to_string())
        ));
        let by_path = std::env::var_os("HOME")
            .map(|home| {
                PathBuf::from(home)
                    .join("Library/Mobile Documents")
                    .join(CONTAINER_FOLDER)
            })
            .filter(|_| cfg!(target_os = "macos"));
        if let Some(path) = &by_path {
            r.line(format!(
                "container by path: {} (there: {})",
                path.display(),
                path.is_dir()
            ));
        }
        let local = app.path().document_dir().ok();
        r.line(format!(
            "local Documents: {}",
            local
                .as_ref()
                .map_or_else(|| "none".to_owned(), |p| p.display().to_string())
        ));
        let Some(root) = container.or(by_path) else {
            r.line("no container: nothing more to probe");
            return r.lines.join("\n");
        };
        let documents = root.join("Documents");
        match action {
            "make" => make(&mut r, &documents),
            "remove" => remove(&mut r, &documents),
            _ => {}
        }
        r.line(format!(
            "Documents: {} (there: {})",
            documents.display(),
            documents.is_dir()
        ));
        let mut listed = Vec::new();
        list(&documents, &documents, 0, &mut listed);
        r.line(format!("listing of Documents: {} entries", listed.len()));
        for entry in listed {
            r.lines.push(format!("              {entry}"));
        }
        #[cfg(target_os = "ios")]
        query(&mut r, app);
        r.lines.join("\n")
    }

    /// Writes the test vault with a note, as the app writes notes.
    fn make(r: &mut Report, documents: &Path) {
        let folder = documents.join(TEST_FOLDER);
        if let Err(e) = fs::create_dir_all(&folder) {
            r.line(format!("make: cannot make {}: {e}", folder.display()));
            return;
        }
        r.line("make: folder there");
        let vault = match Vault::create_with(&folder, crate::coordinator()) {
            Ok(vault) => vault,
            Err(e) => {
                r.line(format!("make: not a vault: {e}"));
                return;
            }
        };
        r.line("make: vault made or opened");
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_millis());
        let path = format!("Spike test {stamp}.md");
        let text = format!(
            "# Spike test {stamp}\n\nWritten by the iCloud container spike \
             (docs/spikes/icloud-container.md).\n\n#spike\n"
        );
        match vault.create_note(&path, &text) {
            Ok(_) => r.line(format!("make: note {path} written")),
            Err(e) => {
                r.line(format!("make: note not written: {e}"));
                return;
            }
        }
        match vault.read_note(&path) {
            Ok(note) => r.line(format!(
                "make: note read back, {} bytes, has its text: {}",
                note.text.len(),
                note.text.contains(&text)
            )),
            Err(e) => r.line(format!("make: note not read back: {e}")),
        }
        match vault.list_notes() {
            Ok(notes) => r.line(format!("make: the vault lists {} notes", notes.len())),
            Err(e) => r.line(format!("make: notes not listed: {e}")),
        }
    }

    fn remove(r: &mut Report, documents: &Path) {
        let folder = documents.join(TEST_FOLDER);
        if !folder.exists() {
            r.line("remove: no test vault");
            return;
        }
        match fs::remove_dir_all(&folder) {
            Ok(()) => r.line("remove: test vault removed"),
            Err(e) => r.line(format!("remove: not removed: {e}")),
        }
    }

    /// Entries under `dir`, three levels deep: path, kind, size, and whether
    /// the file is only in iCloud.
    fn list(root: &Path, dir: &Path, depth: usize, out: &mut Vec<String>) {
        let Ok(entries) = fs::read_dir(dir) else {
            return;
        };
        let mut entries: Vec<_> = entries.flatten().map(|e| e.path()).collect();
        entries.sort();
        for path in entries {
            if out.len() >= 300 {
                return;
            }
            let shown = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .display()
                .to_string();
            match fs::symlink_metadata(&path) {
                Ok(meta) if meta.is_dir() => {
                    out.push(format!("{shown}/"));
                    if depth < 3 {
                        list(root, &path, depth + 1, out);
                    }
                }
                Ok(meta) => {
                    let dataless = flags(&meta) & SF_DATALESS != 0;
                    out.push(format!(
                        "{shown} | {} B{}",
                        meta.len(),
                        if dataless { " | dataless" } else { "" }
                    ));
                }
                Err(e) => out.push(format!("{shown} | {e}")),
            }
        }
    }

    #[cfg(target_os = "macos")]
    fn flags(meta: &fs::Metadata) -> u32 {
        std::os::macos::fs::MetadataExt::st_flags(meta)
    }

    #[cfg(target_os = "ios")]
    fn flags(meta: &fs::Metadata) -> u32 {
        std::os::ios::fs::MetadataExt::st_flags(meta)
    }

    /// What `NSMetadataQuery` lists, also files not downloaded yet.
    #[cfg(target_os = "ios")]
    fn query(r: &mut Report, app: &tauri::AppHandle) {
        use tauri_plugin_vault_access::VaultAccessExt as _;
        r.line("NSMetadataQuery: started");
        match app.vault_access().icloud_query() {
            Ok(q) => {
                r.line(format!(
                    "NSMetadataQuery: started {}, finished gathering {}, after {:.1} ms, {} items",
                    q.started,
                    q.finished,
                    q.ms,
                    q.lines.len()
                ));
                let mut lines = q.lines;
                lines.sort();
                for line in lines {
                    let shown = line
                        .find(CONTAINER_FOLDER)
                        .map_or(line.as_str(), |at| &line[at + CONTAINER_FOLDER.len()..]);
                    r.lines.push(format!("              {shown}"));
                }
            }
            Err(e) => r.line(format!("NSMetadataQuery: failed: {e}")),
        }
    }
}
