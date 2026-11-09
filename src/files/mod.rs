//! Name validation and synchronous file operations for notes and folders.
//!
//! Everything here runs synchronously; a large copy can pause the UI in this
//! milestone. Names are validated before any filesystem call, and collisions
//! gain numeric suffixes (`name.1.md`, `name.1` for folders).

use std::fs;
use std::io::{self, ErrorKind};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, symlink};
use std::path::{Path, PathBuf};

use thiserror::Error;

pub mod trash;

pub use trash::{trash, trash_dir};

/// A `.md` extension, matched case-insensitively.
const NOTE_EXT: &str = "md";
/// Maximum name length in bytes.
const MAX_NAME_LEN: usize = 255;

/// Errors from validating names and operating on notes and folders.
#[derive(Debug, Error)]
pub enum Error {
    /// The name breaks one of the naming rules; the message says which.
    #[error("{0}")]
    InvalidName(String),

    /// The operation was refused (for example, copying a folder into itself).
    #[error("{0}")]
    Refused(String),

    /// Neither `XDG_DATA_HOME` nor `HOME` locates the trash directory.
    #[error("could not determine the trash directory; set HOME or XDG_DATA_HOME")]
    NoTrashDir,

    /// The filesystem call failed; `context` names the operation.
    #[error("{context}: {source}")]
    Io {
        context: &'static str,
        #[source]
        source: io::Error,
    },
}

impl Error {
    /// Whether the failure is a missing source, which callers drop quietly.
    pub fn is_not_found(&self) -> bool {
        matches!(self, Self::Io { source, .. } if source.kind() == ErrorKind::NotFound)
    }

    fn refused(message: impl Into<String>) -> Self {
        Self::Refused(message.into())
    }
}

/// Wrap an [`io::Error`] with the operation that caused it.
fn io_err(context: &'static str) -> impl FnOnce(io::Error) -> Error {
    move |source| Error::Io { context, source }
}

/// Check that `name` is a valid single path component, ignoring surrounding
/// whitespace. The name must not be empty, `.`, `..`, hidden, contain path
/// separators or control characters, or exceed 255 bytes.
pub fn validate_name(name: &str) -> Result<(), Error> {
    let name = name.trim();
    if name.is_empty() {
        return Err(Error::InvalidName("name cannot be empty".to_owned()));
    }
    if name == "." || name == ".." {
        return Err(Error::InvalidName(format!("name cannot be {name:?}")));
    }
    if name.starts_with('.') {
        return Err(Error::InvalidName(
            "name cannot start with a dot".to_owned(),
        ));
    }
    if name.contains(['/', '\\']) {
        return Err(Error::InvalidName(
            "name cannot contain path separators".to_owned(),
        ));
    }
    if name.chars().any(|c| c.is_control()) {
        return Err(Error::InvalidName(
            "name cannot contain control characters".to_owned(),
        ));
    }
    if name.len() > MAX_NAME_LEN {
        return Err(Error::InvalidName(format!(
            "name cannot be longer than {MAX_NAME_LEN} bytes"
        )));
    }
    Ok(())
}

/// Trim `name` and check it with [`validate_name`].
pub fn clean_name(name: &str) -> Result<String, Error> {
    validate_name(name)?;
    Ok(name.trim().to_owned())
}

/// Append `.md` unless `name` already has that extension.
fn with_note_ext(name: &str) -> String {
    if Path::new(name)
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case(NOTE_EXT))
    {
        name.to_owned()
    } else {
        format!("{name}.md")
    }
}

/// Whether `path` exists, counting dangling symlinks as existing.
fn exists(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(_) => Ok(true),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Return `path` when it is free, otherwise insert `.1`, `.2`, ... before the
/// extension (files) or at the end (directories).
fn unique_path(path: &Path, is_dir: bool) -> io::Result<PathBuf> {
    if !exists(path)? {
        return Ok(path.to_path_buf());
    }
    let parent = path.parent().unwrap_or_else(|| Path::new(""));
    let base = path.file_name().unwrap_or_default();
    let (stem, ext) = if is_dir {
        (base.to_os_string(), None)
    } else {
        let stem = Path::new(base).file_stem().unwrap_or(base).to_os_string();
        let ext = Path::new(base).extension().map(|ext| ext.to_os_string());
        (stem, ext)
    };
    for index in 1.. {
        let mut candidate = stem.clone();
        candidate.push(format!(".{index}"));
        if let Some(ext) = &ext
            && !ext.is_empty()
        {
            candidate.push(".");
            candidate.push(ext);
        }
        let candidate = parent.join(candidate);
        if !exists(&candidate)? {
            return Ok(candidate);
        }
    }
    unreachable!("numeric suffixes never run out")
}

/// Whether `child` is `parent` itself or lives inside it.
pub fn is_within(parent: &Path, child: &Path) -> bool {
    child == parent || child.starts_with(parent)
}

/// Create an empty markdown note in `dir`, appending `.md` when missing and
/// adding a numeric suffix when the name is taken. Returns the created path.
pub fn create_note(dir: &Path, name: &str) -> Result<PathBuf, Error> {
    let path = unique_path(&dir.join(with_note_ext(&clean_name(name)?)), false)
        .map_err(io_err("create note"))?;
    fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(io_err("create note"))?;
    Ok(path)
}

/// Create a folder in `dir`, adding a numeric suffix when the name is taken.
/// Returns the created path.
pub fn create_folder(dir: &Path, name: &str) -> Result<PathBuf, Error> {
    let path = unique_path(&dir.join(clean_name(name)?), true).map_err(io_err("create folder"))?;
    fs::create_dir(&path).map_err(io_err("create folder"))?;
    Ok(path)
}

/// Rename `source` within its directory, preserving `.md` for notes and
/// adding a numeric suffix when the target name is taken. Renaming to the
/// current name is a no-op returning `source` unchanged.
pub fn rename(source: &Path, name: &str) -> Result<PathBuf, Error> {
    let mut name = clean_name(name)?;
    let info = fs::symlink_metadata(source).map_err(io_err("rename"))?;
    if !info.is_dir()
        && source
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case(NOTE_EXT))
    {
        name = with_note_ext(&name);
    }
    let dest = source.with_file_name(&name);
    if dest == source {
        return Ok(source.to_path_buf());
    }
    match fs::symlink_metadata(&dest) {
        Ok(dest_info) if same_file(&info, &dest_info) => {
            // A case-only rename on a case-insensitive filesystem.
            fs::rename(source, &dest).map_err(io_err("rename"))?;
            return Ok(dest);
        }
        Ok(_) => {}
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(source) => return Err(io_err("rename")(source)),
    }
    let dest = unique_path(&dest, info.is_dir()).map_err(io_err("rename"))?;
    fs::rename(source, &dest).map_err(io_err("rename"))?;
    Ok(dest)
}

/// Copy `src` into `dst_dir`, recursively for directories and preserving
/// symlinks as links. Copying a folder into itself or a descendant is
/// refused. Returns the copied path.
pub fn copy_entry(src: &Path, dst_dir: &Path) -> Result<PathBuf, Error> {
    let info = fs::symlink_metadata(src).map_err(io_err("copy"))?;
    let base = src
        .file_name()
        .ok_or_else(|| Error::refused(format!("copy: {} has no file name", src.display())))?;
    if info.is_dir() && is_within(src, dst_dir) {
        return Err(Error::refused(format!(
            "copy: cannot copy {base:?} into itself",
            base = base.to_string_lossy()
        )));
    }
    let dest = unique_path(&dst_dir.join(base), info.is_dir()).map_err(io_err("copy"))?;
    copy_path(src, &dest).map_err(|source| {
        let _ = fs::remove_dir_all(&dest);
        io_err("copy")(source)
    })?;
    Ok(dest)
}

/// Move `src` into `dst_dir`, falling back to copy-then-remove across
/// filesystems. Moving a folder into itself or a descendant is refused, as is
/// moving an entry that already lives in `dst_dir`. Returns the moved path.
pub fn move_entry(src: &Path, dst_dir: &Path) -> Result<PathBuf, Error> {
    let info = fs::symlink_metadata(src).map_err(io_err("move"))?;
    let base = src
        .file_name()
        .ok_or_else(|| Error::refused(format!("move: {} has no file name", src.display())))?;
    if src.parent().is_some_and(|parent| parent == dst_dir) {
        return Err(Error::refused(format!(
            "move: {base:?} is already in that folder",
            base = base.to_string_lossy()
        )));
    }
    if info.is_dir() && is_within(src, dst_dir) {
        return Err(Error::refused(format!(
            "move: cannot move {base:?} into itself",
            base = base.to_string_lossy()
        )));
    }
    let dest = unique_path(&dst_dir.join(base), info.is_dir()).map_err(io_err("move"))?;
    move_path(src, &dest).map_err(io_err("move"))?;
    Ok(dest)
}

/// Copy one entry, recursing into directories and recreating symlinks.
fn copy_path(src: &Path, dst: &Path) -> io::Result<()> {
    let info = fs::symlink_metadata(src)?;
    let kind = info.file_type();
    if kind.is_symlink() {
        symlink(fs::read_link(src)?, dst)
    } else if kind.is_dir() {
        fs::DirBuilder::new().mode(0o755).create(dst)?;
        for item in fs::read_dir(src)? {
            let item = item?;
            copy_path(&item.path(), &dst.join(item.file_name()))?;
        }
        fs::set_permissions(dst, info.permissions())
    } else if kind.is_file() {
        fs::copy(src, dst)?;
        fs::set_permissions(dst, info.permissions())
    } else {
        Err(io::Error::other(format!(
            "unsupported file type {}",
            src.display()
        )))
    }
}

/// Rename `src` to `dst`, falling back to copy-then-remove when the two paths
/// are on different filesystems.
fn move_path(src: &Path, dst: &Path) -> io::Result<()> {
    if fs::rename(src, dst).is_ok() {
        return Ok(());
    }
    if let Err(error) = copy_path(src, dst) {
        let _ = fs::remove_dir_all(dst);
        return Err(error);
    }
    fs::remove_dir_all(src).map_err(|error| {
        io::Error::other(format!(
            "copied to {} but could not remove source: {error}",
            dst.display()
        ))
    })
}

/// Whether two metadata records describe the same file.
#[cfg(unix)]
fn same_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino()
}

/// Fallback for platforms without device/inode numbers.
#[cfg(not(unix))]
fn same_file(_a: &fs::Metadata, _b: &fs::Metadata) -> bool {
    false
}
