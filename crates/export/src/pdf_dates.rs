// SPDX-License-Identifier: Apache-2.0
//! The dates of a finished PDF (ADR-0024). Printing stamps the time of printing
//! on it, and `PDFKit` on macOS 26 stamps the time of writing over any others. So
//! the two dates are corrected in the file's bytes: only in its Info dictionary,
//! and only by a value of the same length, so no offset in the file changes.

use std::ops::Range;

use schnipsle_core::stamp::rfc3339_utc;

/// Sets the dates that have a value. A date stays as it is if the file does not
/// hold it in the expected form, which leaves the PDF intact and the print time on it.
pub(crate) fn set_dates(pdf: &mut [u8], created_ms: Option<i64>, modified_ms: Option<i64>) {
    let Some(info) = info_object(pdf) else {
        return;
    };
    for (key, ms) in [
        (&b"/CreationDate"[..], created_ms),
        (&b"/ModDate"[..], modified_ms),
    ] {
        if let Some(ms) = ms.and_then(|ms| u64::try_from(ms).ok()) {
            set_date(&mut pdf[info.clone()], key, ms);
        }
    }
}

/// `(D:20261001070000+00'00')`
fn pdf_date(ms: u64) -> Vec<u8> {
    let digits: String = rfc3339_utc(ms)
        .chars()
        .filter(char::is_ascii_digit)
        .collect();
    format!("(D:{digits}+00'00')").into_bytes()
}

fn set_date(info: &mut [u8], key: &[u8], ms: u64) {
    let Some(at) = find(info, key) else {
        return;
    };
    let after_key = at + key.len();
    let Some(blank) = info[after_key..]
        .iter()
        .position(|b| !b.is_ascii_whitespace())
    else {
        return;
    };
    let start = after_key + blank;
    if !info[start..].starts_with(b"(D:") {
        return;
    }
    let Some(length) = info[start..].iter().position(|b| *b == b')') else {
        return;
    };
    let date = pdf_date(ms);
    if date.len() == length + 1 {
        info[start..=start + length].copy_from_slice(&date);
    }
}

/// Where the Info dictionary is, by the file's trailer, which comes last.
fn info_object(pdf: &[u8]) -> Option<Range<usize>> {
    const INFO: &[u8] = b"/Info ";
    let reference = rfind(pdf, INFO)? + INFO.len();
    let mut parts = pdf[reference..].split(u8::is_ascii_whitespace);
    let number = parts.next().filter(|p| is_number(p))?;
    let generation = parts.next().filter(|p| is_number(p))?;
    if !parts.next()?.starts_with(b"R") {
        return None;
    }
    let object = [number, b" ", generation, b" obj"].concat();
    let start = pdf
        .windows(object.len())
        .enumerate()
        .rev()
        .find(|(at, window)| {
            *window == object.as_slice() && (*at == 0 || pdf[at - 1].is_ascii_whitespace())
        })?
        .0;
    let length = find(&pdf[start..], b"endobj")?;
    Some(start..start + length)
}

fn is_number(part: &[u8]) -> bool {
    !part.is_empty() && part.iter().all(u8::is_ascii_digit)
}

fn find(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).position(|w| w == needle)
}

fn rfind(bytes: &[u8], needle: &[u8]) -> Option<usize> {
    bytes.windows(needle.len()).rposition(|w| w == needle)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-01T07:00:00Z and 2026-10-20T16:30:00Z.
    const CREATED: Option<i64> = Some(1_790_838_000_000);
    const MODIFIED: Option<i64> = Some(1_792_513_800_000);

    fn pdf(info: &str) -> Vec<u8> {
        format!(
            "%PDF-1.3\n3 0 obj\n<< /Length 9 >>\nendobj\n116 0 obj\n<< /ModDate (D:20000101000000Z00'00') >>\nendobj\n16 0 obj\n{info}\nendobj\nxref\ntrailer\n<< /Size 5 /Root 2 0 R /Info 16 0 R /ID [ <00> ] >>\nstartxref\n0\n%%EOF\n"
        )
        .into_bytes()
    }

    fn text(bytes: &[u8]) -> String {
        String::from_utf8_lossy(bytes).into_owned()
    }

    #[test]
    fn both_forms_macos_writes() {
        for (info, expected) in [
            (
                "<< /ModDate (D:20261104141857Z00'00') /Producer (x) /CreationDate\n(D:20261104141857Z00'00') >>",
                "<< /ModDate (D:20261020163000+00'00') /Producer (x) /CreationDate\n(D:20261001070000+00'00') >>",
            ),
            (
                "<< /CreationDate (D:20260101010000+01'00') /ModDate (D:20260202034000+01'00') >>",
                "<< /CreationDate (D:20261001070000+00'00') /ModDate (D:20261020163000+00'00') >>",
            ),
        ] {
            let mut bytes = pdf(info);
            let length = bytes.len();
            set_dates(&mut bytes, CREATED, MODIFIED);
            assert_eq!(bytes.len(), length);
            assert!(text(&bytes).contains(expected), "{}", text(&bytes));
        }
    }

    #[test]
    fn only_the_info_dictionary_changes() {
        let mut bytes = pdf("<< /ModDate (D:20261104141857Z00'00') >>");
        set_dates(&mut bytes, None, MODIFIED);
        let text = text(&bytes);
        assert!(text.contains("116 0 obj\n<< /ModDate (D:20000101000000Z00'00') >>"));
        assert!(text.contains("16 0 obj\n<< /ModDate (D:20261020163000+00'00') >>"));
    }

    #[test]
    fn a_date_without_a_value_stays() {
        let info = "<< /CreationDate (D:20261104141857Z00'00') >>";
        let mut bytes = pdf(info);
        set_dates(&mut bytes, None, MODIFIED);
        assert!(text(&bytes).contains(info));
    }

    #[test]
    fn what_is_not_in_the_expected_form_stays() {
        for info in [
            "<< /CreationDate (D:2026) /ModDate (D:20261104141857) >>",
            "<< /CreationDate <FEFF> /ModDate 12 0 R >>",
            "<< /CreationDate (D:20261104141857Z00'00'",
            "<< /Title (no dates) >>",
        ] {
            let mut bytes = pdf(info);
            let before = bytes.clone();
            set_dates(&mut bytes, CREATED, MODIFIED);
            assert_eq!(text(&bytes), text(&before));
        }
        for file in [
            &b""[..],
            b"%PDF-1.3\n",
            b"trailer << /Info 7 0 R >>",
            b"/Info x y R",
        ] {
            let mut bytes = file.to_vec();
            set_dates(&mut bytes, CREATED, MODIFIED);
            assert_eq!(bytes, file);
        }
    }

    #[test]
    fn a_time_before_1970_is_left_alone() {
        let info = "<< /CreationDate (D:20261104141857Z00'00') >>";
        let mut bytes = pdf(info);
        set_dates(&mut bytes, Some(-1), None);
        assert!(text(&bytes).contains(info));
    }
}
