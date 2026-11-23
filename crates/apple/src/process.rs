// SPDX-License-Identifier: Apache-2.0
//! What the app measures about its own process (ADR-0045): its memory as the
//! system counts it, and which device and system it runs on.
//!
//! The memory is the physical footprint: what Xcode's memory gauge and the
//! Memory column of Activity Monitor show, and what iOS compares with its
//! limit before it ends an app. The web view's own processes are not part of
//! it.

use std::ffi::{CStr, c_char, c_int, c_void};
use std::io;

/// The memory of this process, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Footprint {
    /// The physical footprint now.
    pub now: u64,
    /// The highest physical footprint since the process started, if the
    /// system tells it (macOS 10.15 and iOS 13 and later do).
    pub peak: Option<u64>,
}

/// `TASK_VM_INFO` in `<mach/task_info.h>`.
const TASK_VM_INFO: u32 = 22;

/// The start of `struct task_vm_info` in `<mach/task_info.h>`: revision 3,
/// which brought `ledger_phys_footprint_peak`. The system fills whole
/// revisions only, the newest that fits the count and that it knows, and
/// says how much it filled.
/// Every field after the first lies on its natural alignment, so the
/// `#pragma pack(4)` of the header changes nothing here.
#[repr(C)]
#[derive(Default)]
#[allow(dead_code, reason = "the fields are there for the layout")]
struct TaskVmInfo {
    virtual_size: u64,
    region_count: i32,
    page_size: i32,
    resident_size: u64,
    resident_size_peak: u64,
    /// From `device` to `compressed_lifetime`, 14 sizes this crate does not use.
    unused: [u64; 14],
    phys_footprint: u64,
    min_address: u64,
    max_address: u64,
    ledger_phys_footprint_peak: i64,
    /// The other 20 ledgers of revision 3.
    more_ledgers: [i64; 20],
}

/// `size_of::<T>()` in `natural_t` units, as Mach counts the size of a reply.
const fn count_of(bytes: usize) -> u32 {
    #[allow(clippy::cast_possible_truncation, reason = "a few hundred bytes")]
    let count = (bytes / size_of::<u32>()) as u32;
    count
}

/// The count up to and with `phys_footprint` (revision 1).
const WITH_FOOTPRINT: u32 = count_of(std::mem::offset_of!(TaskVmInfo, min_address));
/// The count of revision 3, with `ledger_phys_footprint_peak`.
const WITH_PEAK: u32 = count_of(size_of::<TaskVmInfo>());

unsafe extern "C" {
    /// The port of this task, which `mach_task_self()` reads in C.
    static mach_task_self_: u32;
    fn task_info(target: u32, flavor: u32, out: *mut i32, count: *mut u32) -> c_int;
    fn sysctlbyname(
        name: *const c_char,
        old: *mut c_void,
        old_len: *mut usize,
        new: *mut c_void,
        new_len: usize,
    ) -> c_int;
}

/// The memory of this process now and at its peak.
///
/// # Errors
/// If the system does not answer, or answers without the footprint.
pub fn footprint() -> io::Result<Footprint> {
    let mut info = TaskVmInfo::default();
    let mut count = WITH_PEAK;
    // SAFETY: `mach_task_self_` is set by the system before any code of the
    // app runs and never changes; reading a `u32` from it is always valid.
    let task = unsafe { mach_task_self_ };
    // SAFETY: `info` is a `repr(C)` prefix of `task_vm_info` that is `count`
    // naturals long; the kernel writes at most `count` naturals into it and
    // sets `count` to how many it wrote.
    let result = unsafe {
        task_info(
            task,
            TASK_VM_INFO,
            (&raw mut info).cast::<i32>(),
            &raw mut count,
        )
    };
    if result != 0 {
        return Err(io::Error::other(format!("task_info failed: {result}")));
    }
    if count < WITH_FOOTPRINT {
        return Err(io::Error::other("the system tells no physical footprint"));
    }
    Ok(Footprint {
        now: info.phys_footprint,
        peak: (count >= WITH_PEAK)
            .then(|| u64::try_from(info.ledger_phys_footprint_peak).ok())
            .flatten(),
    })
}

/// A text value of `sysctl`, such as `hw.machine`; `None` if there is none.
fn sysctl_text(name: &CStr) -> Option<String> {
    let mut len = 0usize;
    // SAFETY: `name` is a C string; with a null buffer the call only writes
    // the needed length into `len`.
    let sized = unsafe {
        sysctlbyname(
            name.as_ptr(),
            std::ptr::null_mut(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if sized != 0 || len == 0 {
        return None;
    }
    let mut buffer = vec![0u8; len];
    // SAFETY: `buffer` is `len` bytes long, and `len` tells the call so; it
    // writes at most that many and sets `len` to how many it wrote.
    let read = unsafe {
        sysctlbyname(
            name.as_ptr(),
            buffer.as_mut_ptr().cast::<c_void>(),
            &raw mut len,
            std::ptr::null_mut(),
            0,
        )
    };
    if read != 0 {
        return None;
    }
    buffer.truncate(len);
    let text = CStr::from_bytes_until_nul(&buffer).ok()?.to_str().ok()?;
    Some(text.to_owned())
}

/// The model of this device, such as `iPhone18,1` or `Mac16,10`; on the iOS
/// simulator its processor, such as `arm64`.
#[must_use]
pub fn device_model() -> Option<String> {
    #[cfg(target_os = "ios")]
    return sysctl_text(c"hw.machine");
    #[cfg(target_os = "macos")]
    return sysctl_text(c"hw.model");
}

/// The version of the system, such as `26.2`.
#[must_use]
pub fn system_version() -> Option<String> {
    sysctl_text(c"kern.osproductversion")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn footprint_is_plausible() {
        let before = footprint().unwrap();
        assert!(before.now > 1_000_000, "{before:?}");
        // Touch 64 MB, which the footprint must show.
        let block = vec![1u8; 64 << 20];
        let after = footprint().unwrap();
        assert!(after.now >= before.now + (60 << 20), "{before:?} {after:?}");
        let peak = after.peak.unwrap();
        assert!(peak >= after.now, "{after:?}");
        drop(block);
    }

    #[test]
    fn device_and_system_are_told() {
        assert!(device_model().is_some_and(|m| !m.is_empty()));
        let version = system_version().unwrap();
        assert!(
            version.split('.').all(|p| p.parse::<u32>().is_ok()),
            "{version}"
        );
    }

    #[test]
    fn the_layout_matches_the_header() {
        // `phys_footprint` follows two 32-bit fields and 17 64-bit ones of
        // `task_vm_info`: 144 bytes in, and it ends after 38 naturals.
        assert_eq!(std::mem::offset_of!(TaskVmInfo, phys_footprint), 144);
        assert_eq!(WITH_FOOTPRINT, 38);
        // `TASK_VM_INFO_REV3_COUNT`: revision 2 (42) and 21 ledgers.
        assert_eq!(WITH_PEAK, 84);
    }
}
