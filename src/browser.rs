//! Directory listing for the note browser.

use std::ffi::OsStr;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use thiserror::Error;

/// One browsable item in a directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// File name without its directory.
    pub name: String,
    /// Full path (`dir.join(name)`).
    pub path: PathBuf,
    /// Whether the entry is a directory (symlinks are followed).
    pub is_dir: bool,
    /// Last modification time, when the filesystem reports one.
    pub modified: Option<SystemTime>,
}

/// Errors from listing a directory.
#[derive(Debug, Error)]
pub enum BrowserError {
    #[error("could not read directory {}: {source}", .path.display())]
    ReadDir {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
}

/// List the visible folders and markdown notes directly inside `dir`.
///
/// Hidden names and entries whose metadata cannot be read (including dangling
/// symlinks) are skipped. Directories come first, then files. Each group is
/// sorted case-insensitively by name, with the exact name as a tie-breaker.
pub fn list(dir: &Path) -> Result<Vec<Entry>, BrowserError> {
    let read_dir = fs::read_dir(dir).map_err(|source| BrowserError::ReadDir {
        path: dir.to_path_buf(),
        source,
    })?;

    let mut entries = Vec::new();
    for item in read_dir {
        let item = item.map_err(|source| BrowserError::ReadDir {
            path: dir.to_path_buf(),
            source,
        })?;

        let name = item.file_name();
        let display = name.to_string_lossy();
        if display.starts_with('.') {
            continue;
        }

        let path = item.path();
        let Ok(metadata) = fs::metadata(&path) else {
            continue;
        };
        let is_dir = metadata.is_dir();
        if !is_dir && !(metadata.is_file() && is_markdown(&name)) {
            continue;
        }

        entries.push(Entry {
            name: display.into_owned(),
            path,
            is_dir,
            modified: metadata.modified().ok(),
        });
    }

    entries
        .sort_by_cached_key(|entry| (!entry.is_dir, entry.name.to_lowercase(), entry.name.clone()));

    Ok(entries)
}

fn is_markdown(name: &OsStr) -> bool {
    Path::new(name)
        .extension()
        .and_then(OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("md"))
}
