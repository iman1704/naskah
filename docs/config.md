# `config`

Source: `src/config.rs`

## Purpose

Loads Naskah's TOML configuration and resolves the settings the app needs at startup: the notes directory, the external editor command, and whether remote images may be fetched.

## What it handles

- Uses `$XDG_CONFIG_HOME/naskah/naskah.toml` when `XDG_CONFIG_HOME` is set to an absolute path, otherwise `$HOME/.config/naskah/naskah.toml`. Relative XDG values are ignored.
- Creates the config file and any missing parent directories with commented defaults on first run.
- Parses TOML with serde. Missing keys use defaults, and unknown keys (including the legacy `pager`) are ignored.
- Never rewrites an existing config file.
- Resolves `notes_dir`: empty means the current working directory, a leading `~` or `~/` expands to `$HOME`, and relative paths are joined to the current working directory so the result is absolute.
- Resolves `editor`: the trimmed config value wins, then a non-empty `$EDITOR`, otherwise `None`.
- Reads `fetch_remote_images` (default `true`). When false, the app disables `http`/`https` image fetching; local images still render.
- Returns typed `ConfigError` values for unreadable, unwritable, or invalid config files and for missing HOME when `~` expansion is needed.

## What it does not handle

- No CLI overrides for config values.
- No check that `notes_dir` exists or is a directory. Listing returns its own error.
- No parsing or splitting of the editor command. That belongs to `src/editor.rs`.
- No hot reload of the config file while the app runs.

## Dependencies

- `std::env` — read `XDG_CONFIG_HOME`, `HOME`, `EDITOR`, and the current directory.
- `std::fs` / `std::io` — read and create the config file.
- `std::path` — build and resolve paths.
- `serde` — deserialize the TOML values.
- `toml` — parse the config file.
- `thiserror` — typed errors.

## Public API

```rust
pub struct Config {
    pub notes_dir: PathBuf,
    pub editor: Option<String>,
    pub fetch_remote_images: bool,
}

impl Config {
    pub fn load() -> Result<Config, ConfigError>;
    pub fn load_from(
        path: &Path,
        cwd: &Path,
        home: Option<&Path>,
        editor_env: Option<&str>,
    ) -> Result<Config, ConfigError>;
}

pub enum ConfigError {
    NoConfigDir,
    CurrentDir(std::io::Error),
    Read { path: PathBuf, source: std::io::Error },
    CreateDir { path: PathBuf, source: std::io::Error },
    Write { path: PathBuf, source: std::io::Error },
    Parse { path: PathBuf, source: toml::de::Error },
    NoHome,
}
```

- `Config.notes_dir` is always absolute.
- `Config.editor` is `None` when neither the config file nor `$EDITOR` sets an editor.
- `Config.fetch_remote_images` defaults to `true` when the key is absent.
- `load()` reads the process environment. `load_from()` takes the config path and resolution inputs explicitly, which keeps filesystem tests independent of global environment variables.

## Example

```rust
let config = Config::load()?;
let entries = browser::list(&config.notes_dir)?;
```
