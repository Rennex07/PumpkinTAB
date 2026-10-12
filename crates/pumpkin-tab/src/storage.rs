//! The one file in the plugin's data folder.
//!
//! `config.yml`, written once on first run from the shipped copy and never
//! rewritten. There is nothing here a command has to save back: every setting
//! belongs to the admin, and assignments live in PumpkinPerm. So there is no
//! second config file and no reason to touch this one, which is what keeps its
//! comments.
//!
//! It lives in the preopened folder, which is what the plugin's `FS_READ_DATA`
//! and `FS_WRITE_DATA` permissions are for.

use std::path::{Path, PathBuf};

use tracing::error;

use crate::config::{CONFIG_FILE, Config, DEFAULT_CONFIG};

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_missing_file_is_created_and_read_back() {
        // Written into a temp folder rather than the real data folder, so a test
        // run cannot clobber the admin's config.
        let folder = std::env::temp_dir().join(format!(
            "pumpkintab-storage-{}-{}",
            std::process::id(),
            "one-file"
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("the temp folder can be created");

        let loaded = load(folder.to_str().expect("a path"));
        assert!(loaded.created, "the file did not exist, so it is created");
        assert_eq!(loaded.error, None);
        assert!(Path::new(folder.to_str().expect("a path"))
            .join(CONFIG_FILE)
            .exists());

        let _ = std::fs::remove_dir_all(&folder);
    }

    #[test]
    fn a_placeholder_key_is_not_accepted_as_a_file_key() {
        // `Config` is strict about unknown keys, so a file with one is reported
        // rather than read as a config that happens to lack a section.
        let folder = std::env::temp_dir().join(format!(
            "pumpkintab-storage-{}-{}",
            std::process::id(),
            "strict"
        ));
        let _ = std::fs::remove_dir_all(&folder);
        std::fs::create_dir_all(&folder).expect("the temp folder can be created");
        std::fs::write(
            Path::new(folder.to_str().expect("a path")).join(CONFIG_FILE),
            "tab-list:\n  format: '%player%'\n  nonsense: true\n",
        )
        .expect("writes");

        let loaded = load(folder.to_str().expect("a path"));
        assert!(
            loaded.error.as_deref().is_some_and(|e| e.contains("nonsense")),
            "{:?}",
            loaded.error
        );

        let _ = std::fs::remove_dir_all(&folder);
    }
}
