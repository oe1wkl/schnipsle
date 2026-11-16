// SPDX-License-Identifier: Apache-2.0
//! An export packed into a zip archive ([`crate::Layout::Zip`]): one file per
//! note with copies of its attachments, as in a folder export, written into a
//! temporary folder and then packed under one folder named as the archive, so
//! unpacking it gives that folder. The archive replaces what is there, as the
//! save dialog has asked.

use std::fs;
use std::io::{self, Cursor, Write as _};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use schnipsle_core::vault::{Result, Vault};
use zip::write::FullFileOptions;
use zip::{CompressionMethod, DateTime, ZipWriter};

use crate::output::replace_file;
use crate::{Exporter, Input, Layout, Options, Report};

/// Files whose content is compressed already, stored as they are.
const COMPRESSED: [&str; 17] = [
    "png", "jpg", "jpeg", "gif", "webp", "heic", "avif", "pdf", "odt", "docx", "epub", "zip",
    "mp3", "m4a", "aac", "mp4", "mov",
];

/// Exports `input` with `exporter` into the zip archive `to`. Without a note to
/// export nothing is written.
pub(crate) fn export(
    exporter: &dyn Exporter,
    vault: &Vault,
    input: &Input,
    options: Options,
    to: &Path,
) -> Result<Report> {
    let staging = tempfile::tempdir()?;
    let name = to
        .file_stem()
        .map_or_else(|| "Notes".to_owned(), |s| s.to_string_lossy().into_owned());
    let folder = staging.path().join(&name);
    let unpacked = Options {
        layout: Layout::FilePerNote,
        ..options
    };
    let report = exporter.export(vault, input, &unpacked, &folder)?;
    if report.files.is_empty() {
        return Ok(report);
    }
    let archive = pack(&folder, &name, SystemTime::now())?;
    replace_file(to, &archive)?;
    Ok(report)
}

/// The files of `folder` zipped, in the order of their paths, under the folder
/// `name`, each dated `time`.
fn pack(folder: &Path, name: &str, time: SystemTime) -> io::Result<Vec<u8>> {
    let mut files = Vec::new();
    collect(folder, "", &mut files)?;
    files.sort();
    let seconds = time
        .duration_since(UNIX_EPOCH)
        .map_or(0, |since| since.as_secs());
    let mut base = FullFileOptions::default()
        .last_modified_time(dos_time(seconds))
        .unix_permissions(0o644);
    // The time in UTC as well, which unpacking on macOS and Linux prefers to
    // the local time without a zone that the zip format has.
    let mut stamp = vec![1_u8];
    stamp.extend_from_slice(&u32::try_from(seconds).unwrap_or(u32::MAX).to_le_bytes());
    base.add_extra_field(0x5455, &stamp, false)
        .map_err(io::Error::other)?;
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for path in files {
        let bytes = fs::read(folder.join(&path))?;
        let extension = Path::new(&path)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default();
        let method = if COMPRESSED.contains(&extension.as_str()) {
            CompressionMethod::Stored
        } else {
            CompressionMethod::Deflated
        };
        let options = base
            .clone()
            .compression_method(method)
            .large_file(bytes.len() >= 0xFFFF_FFFF);
        zip.start_file(format!("{name}/{path}"), options)
            .map_err(io::Error::other)?;
        zip.write_all(&bytes)?;
    }
    Ok(zip.finish().map_err(io::Error::other)?.into_inner())
}

/// The files under `folder`, by their path from it with `/`.
fn collect(folder: &Path, prefix: &str, files: &mut Vec<String>) -> io::Result<()> {
    for entry in fs::read_dir(folder)? {
        let entry = entry?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{prefix}{name}");
        if entry.file_type()?.is_dir() {
            collect(&entry.path(), &format!("{path}/"), files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// The date and time of `seconds` since 1970 in UTC, as the zip format holds
/// it; 1980 for a time it cannot hold.
fn dos_time(seconds: u64) -> DateTime {
    let days = seconds / 86_400;
    let of_day = seconds % 86_400;
    let (year, month, day) = civil(days);
    let fits = |value: u64| u8::try_from(value).unwrap_or(0);
    u16::try_from(year)
        .ok()
        .and_then(|year| {
            DateTime::from_date_and_time(
                year,
                fits(month),
                fits(day),
                fits(of_day / 3600),
                fits(of_day % 3600 / 60),
                fits(of_day % 60),
            )
            .ok()
        })
        .unwrap_or_default()
}

/// Year, month and day of `days` since 1970-01-01, in the Gregorian calendar
/// (Howard Hinnant's `civil_from_days`).
fn civil(days: u64) -> (u64, u64, u64) {
    let z = days + 719_468;
    let era = z / 146_097;
    let day_of_era = z % 146_097;
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let mp = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = year_of_era + era * 400 + u64::from(month <= 2);
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use std::io::Read as _;
    use std::time::Duration;

    use zip::ZipArchive;

    use super::*;

    #[test]
    fn the_calendar_is_right() {
        assert_eq!(civil(0), (1970, 1, 1));
        assert_eq!(civil(11_016), (2000, 2, 29));
        assert_eq!(civil(20_773), (2026, 11, 16));
        let time = dos_time(20_773 * 86_400 + 13 * 3600 + 5 * 60 + 31);
        assert_eq!(
            (
                time.year(),
                time.month(),
                time.day(),
                time.hour(),
                time.minute(),
                time.second()
            ),
            (2026, 11, 16, 13, 5, 30)
        );
        assert_eq!(dos_time(0), DateTime::default(), "before 1980");
    }

    #[test]
    fn a_folder_is_packed_under_its_name_in_order() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("Radio");
        fs::create_dir_all(folder.join("attachments/ab")).unwrap();
        fs::write(folder.join("Tuner.html"), "<p>tuner</p>").unwrap();
        fs::write(folder.join("attachments/ab/kit.png"), [137, 80, 78, 71]).unwrap();
        fs::write(folder.join("Antenna.html"), "<p>antenna</p>").unwrap();
        let time = UNIX_EPOCH + Duration::from_secs(20_773 * 86_400);
        let bytes = pack(&folder, "Radio", time).unwrap();
        let mut archive = ZipArchive::new(Cursor::new(bytes)).unwrap();
        let names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().unwrap().into_owned())
            .collect();
        assert_eq!(
            names,
            [
                "Radio/Antenna.html",
                "Radio/Tuner.html",
                "Radio/attachments/ab/kit.png"
            ]
        );
        let picture = archive.by_name("Radio/attachments/ab/kit.png").unwrap();
        assert_eq!(picture.compression(), CompressionMethod::Stored);
        drop(picture);
        let mut page = archive.by_name("Radio/Tuner.html").unwrap();
        assert_eq!(page.compression(), CompressionMethod::Deflated);
        assert_eq!(page.last_modified().map(|t| t.year()), Some(2026));
        let mut text = String::new();
        page.read_to_string(&mut text).unwrap();
        assert_eq!(text, "<p>tuner</p>");
    }
}
