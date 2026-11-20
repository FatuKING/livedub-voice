//! The usage history file (`usage.json` in the app config dir).
//!
//! ```json
//! {
//!   "version": 1,
//!   "days": {
//!     "2026-11-20": {
//!       "incoming": { "seconds": 754.2, "sessions": 2 },
//!       "outgoing": { "seconds": 120.0, "sessions": 1 }
//!     }
//!   },
//!   "capOverrideDate": "2026-11-20"
//! }
//! ```
//!
//! Days are local calendar days (the user's time zone when the audio was sent). Only seconds are
//! stored; costs are derived from [`super::PRICE_PER_MINUTE_USD`], so a price change applies to
//! the whole history.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::NaiveDate;
use serde::{Deserialize, Serialize};

use crate::persist::{with_suffix, write_atomic};

/// Current version of the usage file format. Bump it when the shape changes, and add the step
/// from the previous version to [`migrate`].
///
/// History: 1 = initial format.
pub const USAGE_VERSION: u32 = 1;

/// Name of the usage file inside the app config dir.
pub const USAGE_FILE_NAME: &str = "usage.json";

/// One direction's usage on one day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DirectionDay {
    /// Seconds of audio streamed to OpenAI.
    pub seconds: f64,
    /// Sessions that streamed audio that day (a session that spans midnight counts on both).
    pub sessions: u32,
}

/// Both directions' usage on one day.
#[derive(Debug, Clone, Copy, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DayRecord {
    pub incoming: DirectionDay,
    pub outgoing: DirectionDay,
}

impl DayRecord {
    pub fn seconds(&self) -> f64 {
        self.incoming.seconds + self.outgoing.seconds
    }
}

/// The whole usage file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct UsageFile {
    pub version: u32,
    /// Usage per local calendar day.
    pub days: BTreeMap<NaiveDate, DayRecord>,
    /// The day the user chose to keep translating over the spending cap.
    pub cap_override_date: Option<NaiveDate>,
}

impl Default for UsageFile {
    fn default() -> Self {
        Self {
            version: USAGE_VERSION,
            days: BTreeMap::new(),
            cap_override_date: None,
        }
    }
}

/// Upgrades a usage file read from disk to [`USAGE_VERSION`], one version at a time.
///
/// A file without `version` is taken as current. A newer version than this build knows is an
/// error (it is backed up rather than overwritten with an older format).
pub fn migrate(value: &mut serde_json::Value) -> Result<(), String> {
    let Some(object) = value.as_object_mut() else {
        return Err("usage file is not a JSON object".into());
    };
    let Some(version) = object.get("version") else {
        object.insert("version".into(), USAGE_VERSION.into());
        return Ok(());
    };
    let version = version
        .as_u64()
        .and_then(|version| u32::try_from(version).ok())
        .ok_or_else(|| format!("invalid usage file version {version}"))?;
    if version > USAGE_VERSION {
        return Err(format!(
            "usage file version {version} is newer than this build ({USAGE_VERSION})"
        ));
    }
    // Steps from older versions go here (`if version == 1 { …; version = 2; }`).
    object.insert("version".into(), version.into());
    Ok(())
}

/// Keeps only plausible numbers (a hand-edited file may hold anything).
fn sanitize(file: &mut UsageFile) {
    for day in file.days.values_mut() {
        for direction in [&mut day.incoming, &mut day.outgoing] {
            if !(direction.seconds.is_finite() && direction.seconds > 0.0) {
                direction.seconds = 0.0;
            }
        }
    }
}

/// Loads the usage history from `path`. Never fails: a missing file is an empty history, and a
/// file that cannot be read, parsed or is from a newer version is moved aside to `<file>.bak`
/// (best effort) and an empty history is returned.
pub fn load(path: &Path) -> UsageFile {
    let text = match fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return UsageFile::default(),
        Err(error) => {
            tracing::warn!(path = %path.display(), %error, "could not read the usage history");
            return UsageFile::default();
        }
    };
    let parsed = serde_json::from_str::<serde_json::Value>(&text)
        .map_err(|error| error.to_string())
        .and_then(|mut value| {
            migrate(&mut value)?;
            serde_json::from_value::<UsageFile>(value).map_err(|error| error.to_string())
        });
    match parsed {
        Ok(mut file) => {
            sanitize(&mut file);
            file
        }
        Err(error) => {
            let backup = backup_path(path);
            match fs::rename(path, &backup) {
                Ok(()) => tracing::warn!(
                    path = %path.display(),
                    backup = %backup.display(),
                    %error,
                    "invalid usage history, backed it up and starting a new one"
                ),
                Err(rename_error) => tracing::warn!(
                    path = %path.display(),
                    %error,
                    %rename_error,
                    "invalid usage history, could not back it up; starting a new one"
                ),
            }
            UsageFile::default()
        }
    }
}

/// Serializes the usage history for [`write`].
pub fn to_json(file: &UsageFile) -> io::Result<Vec<u8>> {
    let mut json = serde_json::to_vec_pretty(file).map_err(io::Error::other)?;
    json.push(b'\n');
    Ok(json)
}

/// Writes serialized usage history to `path` atomically.
pub fn write(path: &Path, json: &[u8]) -> io::Result<()> {
    write_atomic(path, json)
}

/// Path of the backup kept for an invalid usage file.
pub fn backup_path(path: &Path) -> PathBuf {
    with_suffix(path, ".bak")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn date(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    #[test]
    fn json_shape_is_stable() {
        let mut file = UsageFile::default();
        file.days.insert(
            date("2026-11-20"),
            DayRecord {
                incoming: DirectionDay {
                    seconds: 90.5,
                    sessions: 2,
                },
                outgoing: DirectionDay::default(),
            },
        );
        file.cap_override_date = Some(date("2026-11-20"));

        assert_eq!(
            serde_json::to_value(&file).unwrap(),
            json!({
                "version": 1,
                "days": {
                    "2026-11-20": {
                        "incoming": { "seconds": 90.5, "sessions": 2 },
                        "outgoing": { "seconds": 0.0, "sessions": 0 }
                    }
                },
                "capOverrideDate": "2026-11-20"
            })
        );
    }

    #[test]
    fn round_trips_through_disk() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("nested").join(USAGE_FILE_NAME);
        let mut file = UsageFile::default();
        file.days.entry(date("2026-11-01")).or_default().outgoing = DirectionDay {
            seconds: 12.25,
            sessions: 1,
        };

        write(&path, &to_json(&file).unwrap()).unwrap();

        assert_eq!(load(&path), file);
    }

    #[test]
    fn missing_file_is_an_empty_history() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(
            load(&dir.path().join(USAGE_FILE_NAME)),
            UsageFile::default()
        );
    }

    #[test]
    fn file_without_version_loads_as_current() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USAGE_FILE_NAME);
        fs::write(
            &path,
            r#"{ "days": { "2026-10-31": { "incoming": { "seconds": 30 } } } }"#,
        )
        .unwrap();

        let file = load(&path);

        assert_eq!(file.version, USAGE_VERSION);
        assert_eq!(file.days[&date("2026-10-31")].incoming.seconds, 30.0);
        assert_eq!(
            file.days[&date("2026-10-31")].outgoing,
            DirectionDay::default()
        );
        assert!(!backup_path(&path).exists());
    }

    #[test]
    fn newer_version_is_backed_up_not_overwritten() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USAGE_FILE_NAME);
        let newer = r#"{ "version": 99, "days": {} }"#;
        fs::write(&path, newer).unwrap();

        assert_eq!(load(&path), UsageFile::default());
        assert_eq!(fs::read_to_string(backup_path(&path)).unwrap(), newer);
    }

    #[test]
    fn corrupt_file_is_backed_up() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USAGE_FILE_NAME);
        fs::write(&path, "{ not json").unwrap();

        assert_eq!(load(&path), UsageFile::default());
        assert!(!path.exists());
        assert!(backup_path(&path).exists());
    }

    #[test]
    fn implausible_numbers_are_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(USAGE_FILE_NAME);
        fs::write(
            &path,
            r#"{ "version": 1, "days": { "2026-11-02": {
                "incoming": { "seconds": -50, "sessions": 1 },
                "outgoing": { "seconds": 20, "sessions": 1 }
            } } }"#,
        )
        .unwrap();

        let day = load(&path).days[&date("2026-11-02")];

        assert_eq!(day.incoming.seconds, 0.0);
        assert_eq!(day.outgoing.seconds, 20.0);
    }
}
