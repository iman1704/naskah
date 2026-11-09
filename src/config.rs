//! Load Naskah's configuration file and resolve the notes directory, editor
//! command, and remote-image switch.

use std::env;
use std::fs;
use std::io::{self, ErrorKind};
use std::path::{Path, PathBuf};

use serde::Deserialize;
use thiserror::Error;

const DEFAULT_CONTENT: &str = "\
# Naskah configuration.

# Directory to browse. Empty uses the current working directory.
notes_dir = \"\"

# External editor command. Empty falls back to $EDITOR.
editor = \"\"

# Fetch images referenced by http/https URLs. Local images still render.
fetch_remote_images = true
";

/// Raw TOML values. Unknown keys are ignored.
#[derive(Debug, Deserialize)]
#[serde(default)]
struct RawConfig {
    notes_dir: String,
    editor: String,
    #[serde(default = "default_true")]
    fetch_remote_images: bool,
}

impl Default for RawConfig {
    fn default() -> Self {
        Self {
            notes_dir: String::new(),
            editor: String::new(),
            fetch_remote_images: true,
        }
    }
}

fn default_true() -> bool {
    true
}

/// Resolved settings used by the rest of Naskah.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Config {
    /// Absolute directory that holds the notes.
    pub notes_dir: PathBuf,
    /// Editor command from the config or `$EDITOR`; `None` when neither is set.
    pub editor: Option<String>,
    /// Whether images referenced by `http`/`https` URLs may be fetched.
    pub fetch_remote_images: bool,
}

impl Config {
    /// Load the standard config file, creating it with defaults on first run.
    pub fn load() -> Result<Self, ConfigError> {
        let xdg_config_home = env::var_os("XDG_CONFIG_HOME").map(PathBuf::from);
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|path| !path.as_os_str().is_empty());

        let path = config_path_from(xdg_config_home.as_deref(), home.as_deref())?;
        let cwd = env::current_dir().map_err(ConfigError::CurrentDir)?;
        let editor_env = env::var("EDITOR").ok();

        Self::load_from(&path, &cwd, home.as_deref(), editor_env.as_deref())
    }

    /// Load from an explicit path and resolution inputs.
    ///
    /// This is the testable form of [`Config::load`]: `path` is the config
    /// file, `cwd` resolves relative and empty `notes_dir` values, `home`
    /// expands a leading `~`, and `editor_env` stands in for `$EDITOR`.
    pub fn load_from(
        path: &Path,
        cwd: &Path,
        home: Option<&Path>,
        editor_env: Option<&str>,
    ) -> Result<Self, ConfigError> {
        let RawConfig {
            notes_dir,
            editor,
            fetch_remote_images,
        } = read_or_create(path)?;

        let editor = {
            let configured = editor.trim();
            let from_env = editor_env.unwrap_or_default().trim();
            if !configured.is_empty() {
                Some(configured.to_owned())
            } else if !from_env.is_empty() {
                Some(from_env.to_owned())
            } else {
                None
            }
        };

        Ok(Self {
            notes_dir: resolve_notes_dir(notes_dir.trim(), cwd, home)?,
            editor,
            fetch_remote_images,
        })
    }
}

/// Errors from loading and resolving Naskah's configuration.
#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not determine the config directory; set HOME or XDG_CONFIG_HOME")]
    NoConfigDir,

    #[error("could not determine the current directory: {0}")]
    CurrentDir(#[source] io::Error),

    #[error("could not read the config file {}: {source}", .path.display())]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not create the config directory {}: {source}", .path.display())]
    CreateDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not write the config file {}: {source}", .path.display())]
    Write {
        path: PathBuf,
        #[source]
        source: io::Error,
    },

    #[error("could not parse the config file {}: {source}", .path.display())]
    Parse {
        path: PathBuf,
        #[source]
        source: toml::de::Error,
    },

    #[error("could not expand `~` because HOME is not set")]
    NoHome,
}

/// Build the standard config path from XDG and home values.
///
/// Relative `XDG_CONFIG_HOME` values are ignored, as the XDG spec requires an
/// absolute path.
fn config_path_from(
    xdg_config_home: Option<&Path>,
    home: Option<&Path>,
) -> Result<PathBuf, ConfigError> {
    let base = xdg_config_home
        .filter(|path| path.is_absolute())
        .map(Path::to_path_buf)
        .or_else(|| home.map(|home| home.join(".config")))
        .ok_or(ConfigError::NoConfigDir)?;
    Ok(base.join("naskah").join("naskah.toml"))
}

fn read_or_create(path: &Path) -> Result<RawConfig, ConfigError> {
    match fs::read_to_string(path) {
        Ok(contents) => parse(path, &contents),
        Err(error) if error.kind() == ErrorKind::NotFound => {
            create_default_file(path)?;
            parse(path, DEFAULT_CONTENT)
        }
        Err(source) => Err(ConfigError::Read {
            path: path.to_path_buf(),
            source,
        }),
    }
}

fn create_default_file(path: &Path) -> Result<(), ConfigError> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|source| ConfigError::CreateDir {
            path: parent.to_path_buf(),
            source,
        })?;
    }

    fs::write(path, DEFAULT_CONTENT).map_err(|source| ConfigError::Write {
        path: path.to_path_buf(),
        source,
    })
}

fn parse(path: &Path, contents: &str) -> Result<RawConfig, ConfigError> {
    toml::from_str(contents).map_err(|source| ConfigError::Parse {
        path: path.to_path_buf(),
        source,
    })
}

fn resolve_notes_dir(value: &str, cwd: &Path, home: Option<&Path>) -> Result<PathBuf, ConfigError> {
    if value.is_empty() {
        return Ok(cwd.to_path_buf());
    }

    let path = expand_home(value, home)?;
    Ok(if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    })
}

fn expand_home(value: &str, home: Option<&Path>) -> Result<PathBuf, ConfigError> {
    if value == "~" {
        return home.map(Path::to_path_buf).ok_or(ConfigError::NoHome);
    }

    if let Some(rest) = value.strip_prefix("~/") {
        return Ok(home.ok_or(ConfigError::NoHome)?.join(rest));
    }

    Ok(PathBuf::from(value))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xdg_config_home_takes_precedence() {
        let path = config_path_from(Some(Path::new("/xdg")), Some(Path::new("/home")))
            .expect("config path");

        assert_eq!(path, PathBuf::from("/xdg/naskah/naskah.toml"));
    }

    #[test]
    fn home_is_used_when_xdg_is_unset_or_relative() {
        let expected = PathBuf::from("/home/.config/naskah/naskah.toml");

        assert_eq!(
            config_path_from(None, Some(Path::new("/home"))).expect("config path"),
            expected
        );
        assert_eq!(
            config_path_from(Some(Path::new("relative")), Some(Path::new("/home")))
                .expect("config path"),
            expected
        );
    }

    #[test]
    fn missing_config_directory_is_an_error() {
        assert!(matches!(
            config_path_from(None, None),
            Err(ConfigError::NoConfigDir)
        ));
    }
}
