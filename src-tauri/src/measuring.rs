// SPDX-License-Identifier: Apache-2.0
//! What the app measures about itself (N-03, ADR-0045): how long its steps
//! take, in the core and as the page sees them, and how much memory it uses.
//!
//! The times are read from a monotonic clock and kept in memory only: per
//! kind, the last [`KEPT`] and the slowest. Settings › Diagnostics shows them.
//! Nothing is written to disk and nothing is sent (N-02).

use std::collections::{BTreeMap, VecDeque};
use std::sync::{Mutex, OnceLock, PoisonError};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::{CmdResult, CommandError};

/// How many measurements of each kind are kept.
const KEPT: usize = 20;

/// What the page times itself and reports with [`ui_measured`]: a list from
/// the call until it is drawn, or a note until the editor shows it.
const UI_KINDS: [&str; 5] = [
    "allNotesUi",
    "notesInTagsUi",
    "smartFolderUi",
    "searchUi",
    "readNoteUi",
];

/// When the app's code started, for the start time.
static STARTED: OnceLock<Instant> = OnceLock::new();
/// The measurements by kind.
static SERIES: Mutex<BTreeMap<&'static str, Series>> = Mutex::new(BTreeMap::new());

/// One measurement.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Sample {
    /// How long it took, in milliseconds.
    ms: f64,
    /// What was measured, such as the number of notes.
    detail: Detail,
    /// When, in milliseconds since 1970 (UTC).
    at_ms: u64,
}

/// What a measurement was about. The page words it in its own language
/// (ADR-0041 rule 5); a field that does not apply is left out.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Detail {
    /// The notes it found, listed, read or exported.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub notes: Option<usize>,
    /// The tag folders it listed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub folders: Option<usize>,
    /// The search, cut short after 40 characters.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// The size of a note, in kilobytes.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kilobytes: Option<usize>,
    /// The notes an index sync read again.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub read: Option<usize>,
    /// The notes an index sync removed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub removed: Option<usize>,
    /// The file extension of an export's format, such as `pdf`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Debug, Default)]
struct Series {
    count: u64,
    recent: VecDeque<Sample>,
    slowest: Option<Sample>,
}

/// The measurements of one kind, for the page.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SeriesView {
    kind: &'static str,
    /// How many were measured since the app started.
    count: u64,
    /// The last ones, newest first.
    recent: Vec<Sample>,
    slowest: Option<Sample>,
}

/// The app's memory, in bytes (ADR-0045 rule 3).
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Memory {
    now: u64,
    peak: Option<u64>,
}

/// What Settings › Diagnostics shows.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    version: String,
    /// Such as `iPhone18,1` or `Mac16,10`.
    device: Option<String>,
    /// `macos` or `ios`, and the version.
    system: String,
    memory: Option<Memory>,
    /// Why the memory could not be read.
    memory_problem: Option<String>,
    series: Vec<SeriesView>,
}

/// Notes the time the app's code starts; call first thing in `run`.
pub fn note_start() {
    STARTED.get_or_init(Instant::now);
}

fn series() -> std::sync::MutexGuard<'static, BTreeMap<&'static str, Series>> {
    SERIES.lock().unwrap_or_else(PoisonError::into_inner)
}

/// Keeps a measurement of `kind`.
pub fn record(kind: &'static str, took: Duration, detail: Detail) {
    let sample = Sample {
        ms: took.as_secs_f64() * 1000.0,
        detail,
        at_ms: schnipsle_core::stamp::now_ms(),
    };
    let mut all = series();
    let series = all.entry(kind).or_default();
    series.count += 1;
    if series.slowest.as_ref().is_none_or(|s| sample.ms > s.ms) {
        series.slowest = Some(sample.clone());
    }
    if series.recent.len() == KEPT {
        series.recent.pop_front();
    }
    series.recent.push_back(sample);
}

/// Runs `f` and keeps how long it took as `kind` if it succeeded, with the
/// detail `detail` gives for its result.
pub fn timed<T, E>(
    kind: &'static str,
    f: impl FnOnce() -> Result<T, E>,
    detail: impl FnOnce(&T) -> Detail,
) -> Result<T, E> {
    let start = Instant::now();
    let result = f();
    if let Ok(value) = &result {
        record(kind, start.elapsed(), detail(value));
    }
    result
}

/// `n` notes, as a detail.
pub fn notes(n: usize) -> Detail {
    Detail {
        notes: Some(n),
        ..Detail::default()
    }
}

/// A search as a detail: its hits and the query, cut short if it is long.
pub fn search(hits: usize, query: &str) -> Detail {
    let mut shown: String = query.chars().take(40).collect();
    if shown.len() < query.len() {
        shown.push('…');
    }
    Detail {
        query: Some(shown),
        ..notes(hits)
    }
}

/// The page has drawn its first note list: the start is over. Only the first
/// report counts, so a page loaded again does not measure a start.
#[tauri::command]
pub fn ui_started() {
    static ONCE: OnceLock<()> = OnceLock::new();
    if let Some(started) = STARTED.get() {
        ONCE.get_or_init(|| record("start", started.elapsed(), Detail::default()));
    }
}

/// A time the page measured itself, of one of [`UI_KINDS`].
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
pub fn ui_measured(kind: String, ms: f64, detail: Detail) -> CmdResult<()> {
    let Some(kind) = UI_KINDS.iter().find(|k| **k == kind) else {
        return Err(CommandError::other(format!("{kind} is not measured")));
    };
    if !ms.is_finite() || ms < 0.0 {
        return Err(CommandError::other(format!("{ms} ms is not a time")));
    }
    record(kind, Duration::from_secs_f64(ms / 1000.0), detail);
    Ok(())
}

/// The measurements and the memory now, for Settings › Diagnostics.
#[tauri::command]
#[allow(clippy::needless_pass_by_value)] // Tauri passes command arguments by value
pub fn diagnostics(app: tauri::AppHandle) -> Diagnostics {
    let series = series()
        .iter()
        .map(|(kind, s)| SeriesView {
            kind,
            count: s.count,
            recent: s.recent.iter().rev().cloned().collect(),
            slowest: s.slowest.clone(),
        })
        .collect();
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    let (device, version, memory) = (
        schnipsle_apple::device_model(),
        schnipsle_apple::system_version(),
        schnipsle_apple::footprint()
            .map(|f| Memory {
                now: f.now,
                peak: f.peak,
            })
            .map_err(|e| e.to_string()),
    );
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    let (device, version, memory) = (
        None::<String>,
        None::<String>,
        Err::<Memory, _>("not measured on this system".to_owned()),
    );
    let system = std::env::consts::OS;
    let (memory, memory_problem) = match memory {
        Ok(memory) => (Some(memory), None),
        Err(problem) => (None, Some(problem)),
    };
    Diagnostics {
        version: app.package_info().version.to_string(),
        device,
        system: version.map_or_else(|| system.to_owned(), |v| format!("{system} {v}")),
        memory,
        memory_problem,
        series,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keeps_the_last_ones_and_the_slowest() {
        for ms in [
            5, 90, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25,
        ] {
            record("testKind", Duration::from_millis(ms), notes(1));
        }
        let all = series();
        let s = &all["testKind"];
        assert_eq!(s.count, 21);
        assert_eq!(s.recent.len(), KEPT);
        // The first one, 5 ms, was dropped; the slowest stays after it would have.
        assert!((s.recent[0].ms - 90.0).abs() < 0.01);
        assert!((s.slowest.as_ref().unwrap().ms - 90.0).abs() < 0.01);
    }

    #[test]
    fn only_known_kinds_come_from_the_page() {
        assert!(ui_measured("searchUi".into(), 12.5, notes(3)).is_ok());
        assert!(ui_measured("start".into(), 1.0, Detail::default()).is_err());
        assert!(ui_measured("searchUi".into(), f64::NAN, Detail::default()).is_err());
        assert!(ui_measured("searchUi".into(), -1.0, Detail::default()).is_err());
    }

    #[test]
    fn a_long_search_is_cut_short() {
        let json = |d: &Detail| serde_json::to_string(d).unwrap();
        assert_eq!(json(&search(3, "kabel")), r#"{"notes":3,"query":"kabel"}"#);
        let long = format!("ä{}", "x".repeat(49));
        let cut = search(0, &long).query.unwrap();
        assert_eq!(cut.chars().count(), 41);
        assert!(cut.ends_with("x…"));
    }
}
