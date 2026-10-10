//! The two files in the plugin's data folder.
//!
//! ## `config.yml`
//!
//! Written once, on first run, from the shipped copy, and never rewritten. There
//! is nothing here a command has to save back: every setting belongs to the
//! admin, and assignments live in PumpkinPerm. So there is no second config file
//! and no reason to touch this one, which is what keeps its comments.
//!
//! ## `playtime.yml`
//!
//! The other direction. It is ours, it is rewritten whenever a total moves, and
//! there is nothing an admin can usefully configure about it - so it is a plain
//! `uuid: seconds` map with no header, no version and no comments. [`Config`] is
//! not read from it and nothing falls back to it: the only thing a damaged file
//! does is stop [`crate::playtime`] writing over the numbers it failed to parse,
//! which is that module's decision rather than this one's.
//!
//! Both live in the same preopened folder, which is what the plugin's
//! `FS_READ_DATA` and `FS_WRITE_DATA` permissions are for. A failure there is
//! logged and stepped over, never fatal: a tab list that will not start because
//! it could not save a number is worse than a tab list that loses one.

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use tracing::error;

use crate::config::{CONFIG_FILE, PLAYTIME_FILE, Config, DEFAULT_CONFIG};

/// The outcome of reading the configuration.
pub struct Loaded {
    /// The parsed configuration, or the shipped default if the file was absent.
    pub config: Config,
    /// The parser's message, when there was one.
    pub error: Option<String>,
    /// Whether the file was created because it was missing.
    pub created: bool,
}

/// Reads `config.yml` from `folder`, writing the shipped copy first if absent.
///
/// # Panics
/// If the default cannot be written, because there is nowhere to write it and
/// nothing useful can be done about that during `on_load`.
pub fn load(folder: &str) -> Loaded {
    let path = Path::new(folder).join(CONFIG_FILE);

    if !path.exists() {
        write_default(&path, folder);
        return Loaded {
            config: Config::parse(DEFAULT_CONFIG).expect("the shipped config.yml parses"),
            error: None,
            created: true,
        };
    }

    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) => {
            let message = format!("could not read {}: {error}", path.display());
            error!("{message}");
            return Loaded {
                config: Config::default(),
                error: Some(message),
                created: false,
            };
        }
    };

    match Config::parse(&text) {
        Ok(config) => Loaded {
            config,
            error: None,
            created: false,
        },
        Err(error) => {
            error!("{} is not valid: {error}", path.display());
            Loaded {
                config: Config::default(),
                error: Some(error),
                created: false,
            }
        }
    }
}

/// Writes the shipped `config.yml`.
fn write_default(path: &PathBuf, folder: &str) {
    if let Err(error) = std::fs::create_dir_all(folder) {
        panic!("could not create the data folder {folder}: {error}");
    }
    if let Err(error) = std::fs::write(path, DEFAULT_CONFIG) {
        panic!("could not write {}: {error}", path.display());
    }
}

/// Reads the playtime totals out of `folder`.
///
/// [`Ok`] with an empty map when the file is not there, which is a first run and
/// not a fault: there is nothing to repair, and the store is written the first
/// time a total moves.
///
/// # Errors
/// The message, for [`crate::playtime`] to log once and then refuse to overwrite
/// whatever could not be read.
pub fn load_playtimes(folder: &str) -> Result<BTreeMap<String, u64>, String> {
    let path = Path::new(folder).join(PLAYTIME_FILE);
    if !path.exists() {
        return Ok(BTreeMap::new());
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    serde_norway::from_str(&text)
        .map_err(|error| format!("{} is not readable: {error}", path.display()))
}

/// Writes the playtime totals into `folder`.
///
/// Rewrites the whole file. It is a few bytes per player and a few bytes of YAML
/// syntax, so a partial update would save writes at the cost of a format that can
/// be corrupted halfway and has no way to say which entries are good.
///
/// # Errors
/// The message, for the caller's log. Never a panic, unlike [`load`]: losing
/// this file loses hours, so a plugin that cannot write it has to keep running
/// with the totals in memory and say so, not stop.
pub fn save_playtimes(folder: &str, totals: &BTreeMap<String, u64>) -> Result<(), String> {
    std::fs::create_dir_all(folder)
        .map_err(|error| format!("could not create the data folder {folder}: {error}"))?;
    let path = Path::new(folder).join(PLAYTIME_FILE);
    let text = serde_norway::to_string(totals)
        .map_err(|error| format!("could not serialise the playtimes for {}: {error}", path.display()))?;
    std::fs::write(&path, text).map_err(|error| format!("could not write {}: {error}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The file is read back with the same parser it is written with, and a uuid
    /// is the awkward case: it looks like something a YAML parser might have an
    /// opinion about, and if it came back as anything other than a string key the
    /// lookup in [`crate::playtime::seconds`] would simply never find anybody.
    #[test]
    fn the_playtime_store_round_trips_as_a_map_of_strings() {
        let totals: BTreeMap<String, u64> = [
            ("069a79f4-44e9-4726-a5be-fca90e38aaf5", 3_723_u64),
            ("00000000-0000-0000-0000-000000000001", 0),
            ("1-1", 86_400),
        ]
        .into_iter()
        .map(|(uuid, seconds)| (uuid.to_string(), seconds))
        .collect();

        let text = serde_norway::to_string(&totals).expect("serialises");
        let read: BTreeMap<String, u64> =
            serde_norway::from_str(&text).expect("parses");
        assert_eq!(read, totals);

        // The values must survive as numbers too: a key that round-trips as a
        // string but a value that round-trips as text would fail to parse at
        // all, which is a louder failure, so this is really about the key.
        assert!(text.contains("3723"), "{text}");
    }

    /// A file that is not a map of numbers has to be a refusal rather than a
    /// silent empty store, or every player's hours are gone with nothing said.
    #[test]
    fn a_damaged_playtime_file_is_an_error_rather_than_an_empty_store() {
        for damaged in ["not yaml at all: [", "- a\n- b\n", "key: not-a-number\n"] {
            assert!(
                serde_norway::from_str::<BTreeMap<String, u64>>(damaged).is_err(),
                "{damaged:?} should not parse"
            );
        }
    }
}
