// SPDX-License-Identifier: Apache-2.0
//! Settings (ADR-0039): those of the vault, in `.notesapp/settings.yaml`, which
//! syncs to every device (format.md §7, ADR-0017, ADR-0030), and those of this
//! device, in a file of the app's own that never lies in a vault.
//!
//! [`Vault::edit_settings`] changes only the lines of the keys it sets
//! (`flat_yaml::set`), so comments, unknown keys and their order survive. It
//! writes atomically and only if the file is still what it read
//! (format.md §10); if it changed in between, for example on another device,
//! the changes are applied again to the new content, as for smart folders.
//!
//! [`DeviceSettings`] keeps the keys it does not know, so a newer app's
//! settings survive an older one.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::chordpro::chord::Notation;
use crate::flat_yaml;
use crate::history::Retention;
use crate::layout::SETTINGS_FILE;
use crate::vault::{Access, TempFile, Vault, VaultError, accessor_ran_twice, sha256_hex};

type Result<T, E = VaultError> = std::result::Result<T, E>;

/// How often changes are applied again when the file keeps changing.
const ATTEMPTS: usize = 3;

const KEEP_ALL_DAYS: &str = "history_keep_all_days";
const KEEP_DAILY_DAYS: &str = "history_keep_daily_days";
const CHORD_NOTATION: &str = "chord_notation";

/// Why the value in effect is not what the file says.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Deviation {
    /// The file's value is not valid; the default applies.
    Invalid,
    /// The daily period is shorter than the first one, which it is extended to.
    Extended,
}

/// One setting of the vault: what applies, and what the file says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Setting<T> {
    /// The value in effect.
    pub value: T,
    /// What applies when the file does not set it.
    pub default: T,
    /// The value as the file has it; `None` if the file does not set it.
    pub written: Option<String>,
    /// Why `value` is not what the file says, if it is not.
    pub deviation: Option<Deviation>,
}

impl<T: Copy> Setting<T> {
    fn read(text: &str, key: &str, default: T, parse: impl Fn(&str) -> Option<T>) -> Self {
        let written = flat_yaml::value(text, key);
        let parsed = written.as_deref().map(parse);
        Self {
            value: parsed.flatten().unwrap_or(default),
            default,
            deviation: matches!(parsed, Some(None)).then_some(Deviation::Invalid),
            written,
        }
    }
}

/// The settings of a vault (format.md §7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultSettings {
    /// Days during which every snapshot is kept (ADR-0017).
    pub history_keep_all_days: Setting<u64>,
    /// Days until which the last snapshot of each day is kept.
    pub history_keep_daily_days: Setting<u64>,
    /// The names of the notes in song sheets (ADR-0030).
    pub chord_notation: Setting<Notation>,
}

impl VaultSettings {
    /// The settings in the text of a `settings.yaml`. A missing or invalid
    /// value means its default; a daily period shorter than the first is
    /// extended to it.
    #[must_use]
    pub fn from_text(text: &str) -> Self {
        let defaults = Retention::default();
        let days = |key, default| Setting::read(text, key, default, |v| v.parse::<u64>().ok());
        let all = days(KEEP_ALL_DAYS, defaults.all_days);
        let mut daily = days(KEEP_DAILY_DAYS, defaults.daily_days);
        if daily.value < all.value {
            daily.value = all.value;
            daily.deviation.get_or_insert(Deviation::Extended);
        }
        Self {
            history_keep_all_days: all,
            history_keep_daily_days: daily,
            chord_notation: Setting::read(
                text,
                CHORD_NOTATION,
                Notation::default(),
                Notation::parse,
            ),
        }
    }

    /// The retention in effect.
    #[must_use]
    pub fn retention(&self) -> Retention {
        Retention {
            all_days: self.history_keep_all_days.value,
            daily_days: self.history_keep_daily_days.value,
        }
    }
}

/// A change of one setting of the vault: a new value, or `None` to remove the
/// key, so that the default applies. Serialized as `{"key": "historyKeepAllDays",
/// "value": 14}`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "key", content = "value", rename_all = "camelCase")]
pub enum Change {
    /// `history_keep_all_days`.
    HistoryKeepAllDays(Option<u64>),
    /// `history_keep_daily_days`.
    HistoryKeepDailyDays(Option<u64>),
    /// `chord_notation`.
    ChordNotation(Option<Notation>),
}

impl Change {
    fn key(self) -> &'static str {
        match self {
            Self::HistoryKeepAllDays(_) => KEEP_ALL_DAYS,
            Self::HistoryKeepDailyDays(_) => KEEP_DAILY_DAYS,
            Self::ChordNotation(_) => CHORD_NOTATION,
        }
    }

    /// The value as written in the file.
    fn text(self) -> Option<String> {
        match self {
            Self::HistoryKeepAllDays(days) | Self::HistoryKeepDailyDays(days) => {
                days.map(|d| d.to_string())
            }
            Self::ChordNotation(notation) => notation.map(|n| {
                match n {
                    Notation::English => "english",
                    Notation::German => "german",
                }
                .to_owned()
            }),
        }
    }

    /// Whether the change touches how long snapshots are kept.
    #[must_use]
    pub fn is_retention(self) -> bool {
        matches!(
            self,
            Self::HistoryKeepAllDays(_) | Self::HistoryKeepDailyDays(_)
        )
    }
}

/// The text of a `settings.yaml` with `changes` made, each in its own line.
#[must_use]
pub fn apply(text: &str, changes: &[Change]) -> String {
    changes.iter().fold(text.to_owned(), |text, change| {
        flat_yaml::set(&text, change.key(), change.text().as_deref())
    })
}

impl Vault {
    /// The settings of the vault; the defaults if it has none.
    ///
    /// # Errors
    /// I/O errors other than a missing file.
    pub fn settings(&self) -> Result<VaultSettings> {
        Ok(VaultSettings::from_text(&self.settings_text()?))
    }

    /// Makes `changes` in `.notesapp/settings.yaml`, see the module
    /// documentation, and returns the settings now in effect. Nothing is
    /// written if the changes leave the file as it is.
    ///
    /// # Errors
    /// [`VaultError::ReadOnly`]; I/O errors, also when the file keeps changing.
    pub fn edit_settings(&self, changes: &[Change]) -> Result<VaultSettings> {
        if let Some(why) = self.read_only() {
            return Err(VaultError::ReadOnly(why.to_owned()));
        }
        let file = self.root().join(SETTINGS_FILE);
        for _ in 0..ATTEMPTS {
            let (text, read_hash) = self.read_settings_file(&file)?;
            let new_text = apply(text.as_deref().unwrap_or_default(), changes);
            if text.as_deref().unwrap_or_default() == new_text {
                return Ok(VaultSettings::from_text(&new_text));
            }
            if let Some(dir) = file.parent() {
                fs::create_dir_all(dir)?;
            }
            let mut tmp = Some(TempFile::write(&file, new_text.as_bytes())?);
            let replaced = self.coordinated(Access::Write, &file, |file| {
                if read_hash != current_hash(file)? {
                    return Ok(false);
                }
                tmp.take().ok_or_else(accessor_ran_twice)?.replace(file)?;
                Ok(true)
            })?;
            if replaced {
                return Ok(VaultSettings::from_text(&new_text));
            }
        }
        Err(io::Error::other("the settings file keeps changing; nothing was saved").into())
    }

    /// The file's text and hash, `None` for both if it is missing.
    fn read_settings_file(&self, file: &Path) -> Result<(Option<String>, Option<String>)> {
        let bytes = self.coordinated(Access::Read, file, |file| match fs::read(file) {
            Ok(bytes) => Ok(Some(bytes)),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        })?;
        Ok(bytes.map_or((None, None), |bytes| {
            let hash = sha256_hex(&bytes);
            (
                Some(String::from_utf8_lossy(&bytes).into_owned()),
                Some(hash),
            )
        }))
    }
}

fn current_hash(file: &Path) -> Result<Option<String>> {
    match fs::read(file) {
        Ok(bytes) => Ok(Some(sha256_hex(&bytes))),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

const PANDOC: &str = "pandoc";
const CHECK_FOR_UPDATES: &str = "checkForUpdates";

/// The settings of this device, as JSON in the app's own folder (ADR-0039).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct DeviceSettings {
    values: Map<String, Value>,
}

impl DeviceSettings {
    /// The settings in `file`; the defaults if there is none.
    ///
    /// # Errors
    /// I/O errors other than a missing file; a file that is not a JSON object.
    pub fn load(file: &Path) -> io::Result<Self> {
        match fs::read(file) {
            Ok(bytes) => Ok(Self {
                values: serde_json::from_slice(&bytes).map_err(io::Error::other)?,
            }),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e),
        }
    }

    /// Writes the settings to `file` atomically, creating its folder.
    ///
    /// # Errors
    /// I/O errors; the file stays as it was.
    pub fn save(&self, file: &Path) -> io::Result<()> {
        if let Some(dir) = file.parent() {
            fs::create_dir_all(dir)?;
        }
        let mut text = serde_json::to_string_pretty(&self.values).map_err(io::Error::other)?;
        text.push('\n');
        TempFile::write(file, text.as_bytes())?.replace(file)
    }

    /// The Pandoc program picked by the user; `None` to find it (ADR-0025).
    #[must_use]
    pub fn pandoc(&self) -> Option<PathBuf> {
        self.values
            .get(PANDOC)
            .and_then(Value::as_str)
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
    }

    /// Sets the Pandoc program, or `None` to find it.
    pub fn set_pandoc(&mut self, path: Option<&Path>) {
        match path {
            Some(path) => {
                self.values.insert(
                    PANDOC.to_owned(),
                    Value::String(path.to_string_lossy().into_owned()),
                );
            }
            None => {
                self.values.remove(PANDOC);
            }
        }
    }

    /// Whether the app looks for updates (ADR-0036); on unless turned off.
    #[must_use]
    pub fn check_for_updates(&self) -> bool {
        self.values
            .get(CHECK_FOR_UPDATES)
            .and_then(Value::as_bool)
            .unwrap_or(true)
    }

    /// Turns the update check on or off.
    pub fn set_check_for_updates(&mut self, on: bool) {
        self.values
            .insert(CHECK_FOR_UPDATES.to_owned(), Value::Bool(on));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixtures() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/format/7-settings")
    }

    fn vault() -> (tempfile::TempDir, Vault) {
        let dir = tempfile::tempdir().unwrap();
        let vault = Vault::create(dir.path()).unwrap();
        (dir, vault)
    }

    fn settings_file(dir: &tempfile::TempDir) -> PathBuf {
        dir.path().join(SETTINGS_FILE)
    }

    #[test]
    fn defaults_without_a_file() {
        let settings = VaultSettings::from_text("");
        assert_eq!(settings.retention(), Retention::default());
        assert_eq!(settings.chord_notation.value, Notation::English);
        assert_eq!(settings.history_keep_all_days.written, None);
        assert_eq!(settings.history_keep_all_days.deviation, None);
    }

    #[test]
    fn the_example_file() {
        let text = fs::read_to_string(fixtures().join("settings.yaml")).unwrap();
        let settings = VaultSettings::from_text(&text);
        assert_eq!(
            settings.retention(),
            Retention {
                all_days: 14,
                daily_days: 180
            }
        );
        assert_eq!(settings.chord_notation.value, Notation::German);
        assert_eq!(settings.chord_notation.written.as_deref(), Some("german"));
    }

    #[test]
    fn invalid_and_extended_values_say_why() {
        let settings = VaultSettings::from_text(
            "history_keep_all_days: 30\nhistory_keep_daily_days: 10\nchord_notation: latin\n",
        );
        let daily = &settings.history_keep_daily_days;
        assert_eq!(
            (daily.value, daily.deviation),
            (30, Some(Deviation::Extended))
        );
        let notation = &settings.chord_notation;
        assert_eq!(notation.value, Notation::English);
        assert_eq!(notation.written.as_deref(), Some("latin"));
        assert_eq!(notation.deviation, Some(Deviation::Invalid));

        // An invalid daily period is the default, then extended if need be.
        let settings =
            VaultSettings::from_text("history_keep_all_days: 120\nhistory_keep_daily_days: x\n");
        let daily = &settings.history_keep_daily_days;
        assert_eq!(
            (daily.value, daily.deviation),
            (120, Some(Deviation::Invalid))
        );
    }

    #[test]
    fn changes_serialize_as_key_and_value() {
        let changes: Vec<Change> = serde_json::from_str(
            r#"[{"key":"historyKeepAllDays","value":14},
                {"key":"historyKeepDailyDays","value":null},
                {"key":"chordNotation","value":"german"}]"#,
        )
        .unwrap();
        assert_eq!(
            changes,
            [
                Change::HistoryKeepAllDays(Some(14)),
                Change::HistoryKeepDailyDays(None),
                Change::ChordNotation(Some(Notation::German)),
            ]
        );
        assert!(changes[1].is_retention());
        assert!(!changes[2].is_retention());
    }

    /// The fixtures `<case>.before.yaml`, the changes in `<case>.changes.json`,
    /// and the expected `<case>.after.yaml` (format.md §7).
    #[test]
    fn the_fixture_edits() {
        let mut cases = 0;
        for entry in fs::read_dir(fixtures()).unwrap() {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            let Some(case) = name.strip_suffix(".changes.json") else {
                continue;
            };
            let read =
                |suffix: &str| fs::read_to_string(fixtures().join(format!("{case}{suffix}")));
            let changes: Vec<Change> =
                serde_json::from_str(&read(".changes.json").unwrap()).unwrap();
            let before = read(".before.yaml").unwrap();
            let after = apply(&before, &changes);
            let expected = fixtures().join(format!("{case}.after.yaml"));
            if std::env::var_os("SCHNIPSLE_BLESS").is_some() {
                fs::write(&expected, &after).unwrap();
            }
            assert_eq!(after, fs::read_to_string(&expected).unwrap(), "{case}");
            cases += 1;
        }
        assert!(cases >= 3, "only {cases} cases");
    }

    #[test]
    fn edit_writes_the_file_and_keeps_the_rest() {
        let (dir, vault) = vault();
        let after = vault
            .edit_settings(&[Change::HistoryKeepAllDays(Some(14))])
            .unwrap();
        assert_eq!(after.history_keep_all_days.value, 14);
        assert_eq!(
            fs::read_to_string(settings_file(&dir)).unwrap(),
            "history_keep_all_days: 14\n"
        );

        fs::write(
            settings_file(&dir),
            "# mine\nhistory_keep_all_days: 14 # two weeks\nfuture: 1\n",
        )
        .unwrap();
        let after = vault
            .edit_settings(&[
                Change::HistoryKeepAllDays(None),
                Change::ChordNotation(Some(Notation::German)),
            ])
            .unwrap();
        assert_eq!(after.history_keep_all_days.value, 7);
        assert_eq!(after.chord_notation.value, Notation::German);
        assert_eq!(
            fs::read_to_string(settings_file(&dir)).unwrap(),
            "# mine\nfuture: 1\nchord_notation: german\n"
        );
        assert_eq!(vault.settings().unwrap(), after);
        assert_eq!(vault.retention(), after.retention());
        assert_eq!(vault.chord_notation(), Notation::German);
    }

    #[test]
    fn an_edit_that_changes_nothing_writes_nothing() {
        let (dir, vault) = vault();
        vault.edit_settings(&[Change::ChordNotation(None)]).unwrap();
        assert!(!settings_file(&dir).exists());
    }

    #[test]
    fn a_read_only_vault_is_not_written() {
        let (dir, _) = vault();
        // A vault of a newer format opens read-only (format.md §2).
        fs::write(
            dir.path().join(crate::layout::VAULT_FILE),
            "format: 99\nid: 01JA7Q5Y0Z8H3K2M6N4P9R1S2T\n",
        )
        .unwrap();
        let vault = Vault::open(dir.path()).unwrap();
        assert!(vault.read_only().is_some());
        assert!(matches!(
            vault.edit_settings(&[Change::HistoryKeepAllDays(Some(1))]),
            Err(VaultError::ReadOnly(_))
        ));
        assert!(!settings_file(&dir).exists());
    }

    #[test]
    fn device_settings_keep_unknown_keys() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("config").join("settings.json");
        let mut settings = DeviceSettings::load(&file).unwrap();
        assert_eq!(settings.pandoc(), None);
        assert!(settings.check_for_updates());

        settings.set_pandoc(Some(Path::new("/opt/pandoc/bin/pandoc")));
        settings.set_check_for_updates(false);
        settings.save(&file).unwrap();
        let mut text: Map<String, Value> =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        text.insert("readingFont".into(), Value::String("serif".into()));
        fs::write(&file, serde_json::to_string(&text).unwrap()).unwrap();

        let mut settings = DeviceSettings::load(&file).unwrap();
        assert_eq!(
            settings.pandoc(),
            Some(PathBuf::from("/opt/pandoc/bin/pandoc"))
        );
        assert!(!settings.check_for_updates());
        settings.set_pandoc(None);
        settings.save(&file).unwrap();
        let text: Map<String, Value> =
            serde_json::from_str(&fs::read_to_string(&file).unwrap()).unwrap();
        assert_eq!(
            text.get("readingFont"),
            Some(&Value::String("serif".into()))
        );
        assert_eq!(text.get(PANDOC), None);
        assert_eq!(text.get(CHECK_FOR_UPDATES), Some(&Value::Bool(false)));
    }

    #[test]
    fn a_damaged_device_file_is_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("settings.json");
        fs::write(&file, "[1, 2").unwrap();
        assert!(DeviceSettings::load(&file).is_err());
        fs::write(&file, "[1, 2]").unwrap();
        assert!(DeviceSettings::load(&file).is_err());
    }
}
