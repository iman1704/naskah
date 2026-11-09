//! Move-to-trash deletion into Naskah's flat XDG trash directory.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use super::{Error, io_err, move_path, unique_path};

/// Naskah's flat trash directory: `$XDG_DATA_HOME/naskah/trash`, falling back
/// to `~/.local/share/naskah/trash`. The directory is not created here.
pub fn trash_dir() -> Result<PathBuf, Error> {
    let base = env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| {
            env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(".local").join("share"))
        })
        .ok_or(Error::NoTrashDir)?;
    Ok(base.join("naskah").join("trash"))
}

/// Move `path` into the trash directory, adding a numeric suffix when the
/// name is taken. Returns the path inside the trash.
pub fn trash(path: &Path) -> Result<PathBuf, Error> {
    let dir = trash_dir()?;
    fs::create_dir_all(&dir).map_err(io_err("trash"))?;
    let info = fs::symlink_metadata(path).map_err(io_err("trash"))?;
    let base = path
        .file_name()
        .ok_or_else(|| Error::refused(format!("trash: {} has no file name", path.display())))?;
    let dest = unique_path(&dir.join(base), info.is_dir()).map_err(io_err("trash"))?;
    move_path(path, &dest).map_err(io_err("trash"))?;
    Ok(dest)
}
