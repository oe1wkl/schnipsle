// SPDX-License-Identifier: Apache-2.0
//! Folders listed in bulk with `getattrlistbulk(2)` (ADR-0047).
//!
//! One call returns the name, kind, modification time, flags and size of
//! many entries of a folder, where `std::fs` needs one `lstat` per entry; on an
//! iPhone with 10,000 notes in iCloud Drive that is about 1.5 ms instead of
//! 230 ms. [`BulkLister`] gives the same answers as
//! [`schnipsle_core::PlainLister`]: an entry whose attributes the system
//! cannot give is asked with `lstat`, and a folder the call fails for is
//! listed with `PlainLister`, so errors are the same as there. The flags
//! tell whether a file is dataless (ADR-0050).

use std::ffi::{OsString, c_int, c_void};
use std::fs::{self, File};
use std::io;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;

use schnipsle_core::listing::{SF_DATALESS, is_dataless, kind_of};
use schnipsle_core::{EntryKind, FileStat, FolderEntry, FolderLister, PlainLister};

/// Lists folders with `getattrlistbulk`, falling back to [`PlainLister`].
#[derive(Debug, Default, Clone, Copy)]
pub struct BulkLister;

impl FolderLister for BulkLister {
    fn list(&self, dir: &Path) -> io::Result<Vec<FolderEntry>> {
        bulk_list(dir).or_else(|_| PlainLister.list(dir))
    }
}

/// `struct attrlist` of `<sys/attr.h>`.
#[repr(C)]
struct AttrList {
    bitmapcount: u16,
    reserved: u16,
    commonattr: u32,
    volattr: u32,
    dirattr: u32,
    fileattr: u32,
    forkattr: u32,
}

// SAFETY: the declaration matches `getattrlistbulk(2)` in `<sys/attr.h>` of
// macOS 10.10 and later and iOS 8 and later.
unsafe extern "C" {
    fn getattrlistbulk(
        dirfd: c_int,
        attr_list: *mut AttrList,
        attr_buf: *mut c_void,
        attr_buf_size: usize,
        options: u64,
    ) -> c_int;
}

const ATTR_BIT_MAP_COUNT: u16 = 5;
const ATTR_CMN_NAME: u32 = 0x0000_0001;
const ATTR_CMN_OBJTYPE: u32 = 0x0000_0008;
const ATTR_CMN_MODTIME: u32 = 0x0000_0400;
const ATTR_CMN_FLAGS: u32 = 0x0004_0000;
const ATTR_CMN_ERROR: u32 = 0x2000_0000;
const ATTR_CMN_RETURNED_ATTRS: u32 = 0x8000_0000;
const ATTR_FILE_DATALENGTH: u32 = 0x0000_0200;
/// `fsobj_type_t` values of `<sys/vnode.h>`.
const VREG: u32 = 1;
const VDIR: u32 = 2;

/// Room for a few thousand entries per call.
const BUFFER: usize = 256 * 1024;

fn bulk_list(dir: &Path) -> io::Result<Vec<FolderEntry>> {
    let folder = File::open(dir)?;
    let mut list = AttrList {
        bitmapcount: ATTR_BIT_MAP_COUNT,
        reserved: 0,
        commonattr: ATTR_CMN_RETURNED_ATTRS
            | ATTR_CMN_NAME
            | ATTR_CMN_ERROR
            | ATTR_CMN_OBJTYPE
            | ATTR_CMN_MODTIME
            | ATTR_CMN_FLAGS,
        volattr: 0,
        dirattr: 0,
        fileattr: ATTR_FILE_DATALENGTH,
        forkattr: 0,
    };
    // Each entry starts at an 8-byte boundary of the buffer.
    let mut storage = vec![0_u8; BUFFER + 8];
    let start = storage.as_ptr().align_offset(8);
    let buf = &mut storage[start..start + BUFFER];
    let mut out = Vec::new();
    loop {
        // SAFETY: `folder` stays open for the call, `list` is a valid
        // `attrlist`, and `buf` is writable for `buf.len()` bytes, which the
        // call does not write past.
        let count = unsafe {
            getattrlistbulk(
                folder.as_raw_fd(),
                &raw mut list,
                buf.as_mut_ptr().cast(),
                buf.len(),
                0,
            )
        };
        let count = match usize::try_from(count) {
            Ok(0) => break,
            Ok(count) => count,
            Err(_) => return Err(io::Error::last_os_error()),
        };
        let mut at = 0;
        for _ in 0..count {
            let len = read_u32(buf, at)
                .and_then(|len| usize::try_from(len).ok())
                .filter(|&len| len > 0)
                .ok_or_else(|| io::Error::other("getattrlistbulk returned a damaged entry"))?;
            let group = buf
                .get(at..at + len)
                .ok_or_else(|| io::Error::other("getattrlistbulk returned a damaged entry"))?;
            let entry = parse(group)
                .ok_or_else(|| io::Error::other("getattrlistbulk returned a damaged entry"))?;
            out.extend(entry.resolve(dir));
            at += len;
        }
    }
    Ok(out)
}

/// One entry as the call returned it; fields it did not return are `None`.
struct Raw {
    name: OsString,
    error: u32,
    kind: Option<u32>,
    modified: Option<(i64, i64)>,
    /// `st_flags`, as `lstat` gives them.
    flags: Option<u32>,
    size: Option<i64>,
}

impl Raw {
    /// The entry as [`PlainLister`] would give it; `None` to leave it out.
    fn resolve(self, dir: &Path) -> Option<FolderEntry> {
        let kind = self.kind.map(|kind| match kind {
            VREG => EntryKind::File,
            VDIR => EntryKind::Folder,
            _ => EntryKind::Other,
        });
        let stat = match (self.error, kind, self.modified) {
            (0, Some(EntryKind::File), Some(modified)) => self.size.map(|size| FileStat {
                modified_ms: millis(modified),
                size: u64::try_from(size).unwrap_or(0),
            }),
            (0, Some(_), Some(modified)) => Some(FileStat {
                modified_ms: millis(modified),
                size: 0,
            }),
            _ => None,
        };
        if let (Some(kind), Some(stat), Some(flags)) = (kind, stat, self.flags) {
            return Some(FolderEntry {
                name: self.name,
                kind,
                stat: Some(stat),
                dataless: flags & SF_DATALESS != 0,
            });
        }
        // The system could not tell everything: ask as `PlainLister` does.
        let meta = fs::symlink_metadata(dir.join(&self.name));
        let kind = kind.or_else(|| meta.as_ref().ok().map(|m| kind_of(m.file_type())))?;
        Some(FolderEntry {
            name: self.name,
            kind,
            stat: meta.as_ref().ok().map(FileStat::of),
            dataless: meta.as_ref().is_ok_and(is_dataless),
        })
    }
}

/// Milliseconds since 1970 of a `timespec`, 0 for a time before it.
fn millis((seconds, nanos): (i64, i64)) -> u64 {
    match (u64::try_from(seconds), u64::try_from(nanos)) {
        (Ok(seconds), Ok(nanos)) => seconds
            .saturating_mul(1000)
            .saturating_add(nanos / 1_000_000),
        _ => 0,
    }
}

fn read_u32(buf: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_ne_bytes(buf.get(at..at + 4)?.try_into().ok()?))
}

fn read_i32(buf: &[u8], at: usize) -> Option<i32> {
    Some(i32::from_ne_bytes(buf.get(at..at + 4)?.try_into().ok()?))
}

fn read_i64(buf: &[u8], at: usize) -> Option<i64> {
    Some(i64::from_ne_bytes(buf.get(at..at + 8)?.try_into().ok()?))
}

/// Reads one group: its length, the attributes returned (`attribute_set_t`),
/// then the attributes in the order of `getattrlist(2)`, each packed at a
/// 4-byte boundary, the error right after the returned set.
fn parse(group: &[u8]) -> Option<Raw> {
    let common = read_u32(group, 4)?;
    let file = read_u32(group, 16)?;
    let mut at = 4 + 20;
    let mut error = 0;
    if common & ATTR_CMN_ERROR != 0 {
        error = read_u32(group, at)?;
        at += 4;
    }
    if common & ATTR_CMN_NAME == 0 {
        return None;
    }
    // An `attrreference_t`: offset from itself, and length with the final NUL.
    let offset = usize::try_from(read_i32(group, at)?).ok()?;
    let len = usize::try_from(read_u32(group, at + 4)?).ok()?;
    let bytes = group.get(at + offset..at + offset + len)?;
    let name = bytes.strip_suffix(&[0]).unwrap_or(bytes).to_vec();
    at += 8;
    let mut kind = None;
    if common & ATTR_CMN_OBJTYPE != 0 {
        kind = Some(read_u32(group, at)?);
        at += 4;
    }
    let mut modified = None;
    if common & ATTR_CMN_MODTIME != 0 {
        modified = Some((read_i64(group, at)?, read_i64(group, at + 8)?));
        at += 16;
    }
    let mut flags = None;
    if common & ATTR_CMN_FLAGS != 0 {
        flags = Some(read_u32(group, at)?);
        at += 4;
    }
    let mut size = None;
    if file & ATTR_FILE_DATALENGTH != 0 {
        size = Some(read_i64(group, at)?);
    }
    Some(Raw {
        name: OsString::from_vec(name),
        error,
        kind,
        modified,
        flags,
        size,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{Duration, UNIX_EPOCH};

    fn sorted(mut entries: Vec<FolderEntry>) -> Vec<FolderEntry> {
        entries.sort_by(|a, b| a.name.cmp(&b.name));
        entries
    }

    fn set_modified(path: &Path, when: std::time::SystemTime) {
        File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(when)
            .unwrap();
    }

    #[test]
    fn gives_the_same_entries_as_the_plain_lister() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        // More than one buffer holds, so the call runs several times.
        for i in 0..4000 {
            fs::write(root.join(format!("Note {i}.md")), "x".repeat(i % 700)).unwrap();
        }
        fs::write(root.join("Grüße und Überschrift.md"), "ü").unwrap();
        fs::write(root.join("Cafe\u{301} decomposed.md"), "é").unwrap();
        fs::write(root.join(".hidden.md"), "").unwrap();
        fs::write(root.join("empty.md"), "").unwrap();
        fs::write(root.join(".Note 1.md.icloud"), "placeholder").unwrap();
        let precise = root.join("precise.md");
        fs::write(&precise, "t").unwrap();
        set_modified(
            &precise,
            UNIX_EPOCH + Duration::new(1_700_000_000, 987_654_321),
        );
        let old = root.join("old.md");
        fs::write(&old, "o").unwrap();
        set_modified(&old, UNIX_EPOCH - Duration::from_millis(1500));
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("inside.md"), "in").unwrap();
        std::os::unix::fs::symlink(&precise, root.join("link.md")).unwrap();
        std::os::unix::fs::symlink(root.join("sub"), root.join("folder link")).unwrap();

        let bulk = sorted(BulkLister.list(root).unwrap());
        let plain = sorted(PlainLister.list(root).unwrap());
        assert_eq!(bulk.len(), 4010);
        assert_eq!(bulk, plain);
        let find = |name: &str| bulk.iter().find(|e| e.name == name).unwrap();
        assert_eq!(
            find("precise.md").stat.unwrap().modified_ms,
            1_700_000_000_987
        );
        assert_eq!(find("old.md").stat.unwrap().modified_ms, 0);
        assert_eq!(find("sub").kind, EntryKind::Folder);
        assert_eq!(find("link.md").kind, EntryKind::Other);
        assert_eq!(find("Note 699.md").stat.unwrap().size, 699);
    }

    #[test]
    fn an_empty_folder_has_no_entries() {
        let dir = tempfile::tempdir().unwrap();
        assert!(BulkLister.list(dir.path()).unwrap().is_empty());
    }

    #[test]
    fn errors_are_those_of_the_plain_lister() {
        let dir = tempfile::tempdir().unwrap();
        let missing = dir.path().join("missing");
        let file = dir.path().join("file.md");
        fs::write(&file, "x").unwrap();
        for path in [missing, file] {
            let bulk = BulkLister.list(&path).unwrap_err();
            let plain = PlainLister.list(&path).unwrap_err();
            assert_eq!(bulk.kind(), plain.kind(), "{}", path.display());
        }
    }

    #[test]
    fn times_before_1970_count_as_0() {
        assert_eq!(millis((-2, 500_000_000)), 0);
        assert_eq!(millis((1, 999_999_999)), 1999);
    }
}
