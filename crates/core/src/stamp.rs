// SPDX-License-Identifier: Apache-2.0
//! Timestamps and note ids as the vault format writes them: RFC 3339 in UTC
//! (format.md §4), compact UTC stamps for snapshot names (§7), and ULIDs (§4).

use std::fmt::Write as _;
use std::time::{SystemTime, UNIX_EPOCH};

/// Milliseconds since the Unix epoch, now.
#[must_use]
pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX))
}

/// `2026-10-15T21:29:37Z`
#[must_use]
pub fn rfc3339_utc(ms: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(ms / 1000);
    format!("{y:04}-{mo:02}-{d:02}T{h:02}:{mi:02}:{s:02}Z")
}

/// `20261015T212937Z`, the snapshot name of format.md §7.
#[must_use]
pub fn compact_utc(ms: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(ms / 1000);
    format!("{y:04}{mo:02}{d:02}T{h:02}{mi:02}{s:02}Z")
}

/// Splits Unix seconds into UTC date and time (proleptic Gregorian calendar).
fn civil(secs: u64) -> (u64, u64, u64, u64, u64, u64) {
    let days = secs / 86_400;
    let rem = secs % 86_400;
    // Howard Hinnant's days-to-civil, shifted so the era starts on 0000-03-01.
    let z = days + 719_468;
    let era = z / 146_097;
    let doe = z % 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + u64::from(month <= 2);
    (year, month, day, rem / 3600, rem % 3600 / 60, rem % 60)
}

const CROCKFORD: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// A new ULID for the current time.
///
/// # Errors
/// If the operating system cannot provide random bytes.
pub fn new_ulid() -> std::io::Result<String> {
    let mut random = [0u8; 10];
    getrandom::fill(&mut random).map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(ulid(now_ms(), random))
}

/// Encodes a ULID: 48-bit millisecond timestamp, then 80 random bits, as 26
/// Crockford base-32 characters.
#[must_use]
pub fn ulid(ms: u64, random: [u8; 10]) -> String {
    let mut value = u128::from(ms & 0xFFFF_FFFF_FFFF) << 80;
    for (i, b) in random.iter().enumerate() {
        value |= u128::from(*b) << (8 * (9 - i));
    }
    let mut out = String::with_capacity(26);
    for i in (0..26).rev() {
        let digit = usize::try_from((value >> (5 * i)) & 0x1F).unwrap_or(0);
        let _ = out.write_char(char::from(CROCKFORD[digit]));
    }
    out
}

/// `true` for a syntactically valid ULID. Ids come from note files, so they are
/// checked before being used in a path (`.notesapp/history/<id>/`).
#[must_use]
pub fn is_ulid(s: &str) -> bool {
    s.len() == 26
        && s.as_bytes()[0] <= b'7' // 128 bits: the first digit carries 3 bits
        && s.bytes()
            .all(|b| CROCKFORD.contains(&b.to_ascii_uppercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc() {
        for (secs, rfc, compact) in [
            (0, "1970-01-01T00:00:00Z", "19700101T000000Z"),
            (951_868_799, "2000-02-29T23:59:59Z", "20000229T235959Z"),
            (1_792_099_777, "2026-10-15T21:29:37Z", "20261015T212937Z"),
            (4_107_542_400, "2100-03-01T00:00:00Z", "21000301T000000Z"),
        ] {
            assert_eq!(rfc3339_utc(secs * 1000 + 999), rfc);
            assert_eq!(compact_utc(secs * 1000), compact);
        }
    }

    #[test]
    fn ulid_encoding() {
        assert_eq!(ulid(0, [0; 10]), "00000000000000000000000000");
        assert_eq!(ulid(u64::MAX, [0xFF; 10]), "7ZZZZZZZZZZZZZZZZZZZZZZZZZ");
        // Timestamp part of the example in the ULID specification.
        assert!(ulid(1_469_922_850_259, [0; 10]).starts_with("01ARZ3NDEK"));
        let id = new_ulid().unwrap();
        assert!(is_ulid(&id), "{id}");
        assert_ne!(id, new_ulid().unwrap());
    }

    #[test]
    fn ulid_validation() {
        assert!(is_ulid("01ARZ3NDEKTSV4RRFFQ69G5FAV"));
        assert!(is_ulid("01arz3ndektsv4rrffq69g5fav"));
        for bad in [
            "",
            "../../etc",
            "01ARZ3NDEKTSV4RRFFQ69G5FA",
            "81ARZ3NDEKTSV4RRFFQ69G5FAV",
            "01ARZ3NDEKTSV4RRFFQ69G5FAU", // U is not Crockford
            "01ARZ3NDEK/SV4RRFFQ69G5FAV",
        ] {
            assert!(!is_ulid(bad), "{bad}");
        }
    }
}
