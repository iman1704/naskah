use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use naskah::files::{self, trash};
use tempfile::TempDir;

/// Serializes tests that point `XDG_DATA_HOME` at a scratch directory.
static ENV_GUARD: Mutex<()> = Mutex::new(());

fn dir() -> TempDir {
    tempfile::tempdir().expect("scratch directory")
}

fn note(dir: &Path, name: &str) -> PathBuf {
    let path = dir.join(name);
    fs::write(&path, "# note").expect("write note");
    path
}

#[test]
fn rejects_bad_names_and_accepts_good_ones() {
    for name in ["", "   ", ".", "..", ".hidden", "a/b", "a\\b", "a\x07b"] {
        assert!(files::validate_name(name).is_err(), "{name:?} should fail");
    }
    assert!(files::validate_name(&"a".repeat(256)).is_err());
    for name in ["note", "note.md", "note.MD", "my note!", "a-b_c"] {
        assert!(files::validate_name(name).is_ok(), "{name:?} should pass");
    }
    assert_eq!(files::clean_name("  spaced  ").unwrap(), "spaced");
}

#[test]
fn create_note_appends_md_and_suffixes_collisions() {
    let dir = dir();

    let first = files::create_note(dir.path(), "hello").expect("create");
    assert_eq!(first, dir.path().join("hello.md"));

    let kept = files::create_note(dir.path(), "caps.MD").expect("create");
    assert_eq!(kept.file_name().unwrap(), "caps.MD");

    let second = files::create_note(dir.path(), "hello").expect("create");
    assert_eq!(second, dir.path().join("hello.1.md"));
}

#[test]
fn create_folder_suffixes_collisions() {
    let dir = dir();

    let first = files::create_folder(dir.path(), "projects").expect("create");
    assert!(first.is_dir());

    let second = files::create_folder(dir.path(), "projects").expect("create");
    assert_eq!(second, dir.path().join("projects.1"));
}

#[test]
fn rename_keeps_md_and_ignores_unchanged_names() {
    let dir = dir();
    let source = note(dir.path(), "old.md");

    let renamed = files::rename(&source, "new").expect("rename");
    assert_eq!(renamed, dir.path().join("new.md"));
    assert!(!source.exists());

    let same = files::rename(&renamed, "new.md").expect("rename");
    assert_eq!(same, renamed);

    note(dir.path(), "taken.md");
    let suffixed = files::rename(&renamed, "taken").expect("rename");
    assert_eq!(suffixed, dir.path().join("taken.1.md"));

    let folder = dir.path().join("docs");
    fs::create_dir(&folder).expect("mkdir");
    let moved = files::rename(&folder, "papers").expect("rename folder");
    assert_eq!(moved, dir.path().join("papers"));
}

#[test]
fn copy_duplicates_files_folders_and_symlinks() {
    let dir = dir();
    let target = dir.path().join("target");
    fs::create_dir(&target).expect("mkdir");
    let source = note(dir.path(), "a.md");
    symlink(&source, dir.path().join("link.md")).expect("symlink");

    let copied = files::copy_entry(&source, &target).expect("copy file");
    assert_eq!(copied, target.join("a.md"));
    assert_eq!(fs::read_to_string(&copied).unwrap(), "# note");

    let copied = files::copy_entry(&dir.path().join("link.md"), &target).expect("link");
    assert_eq!(
        fs::read_link(&copied).unwrap(),
        fs::read_link(dir.path().join("link.md")).unwrap()
    );

    let folder = dir.path().join("docs");
    fs::create_dir(&folder).expect("mkdir");
    note(&folder, "inner.md");
    let copied = files::copy_entry(&folder, &target).expect("copy folder");
    assert!(copied.join("inner.md").exists());

    let again = files::copy_entry(&source, &target).expect("copy again");
    assert_eq!(again, target.join("a.1.md"));
}

#[test]
fn copy_and_move_refuse_a_folder_into_itself() {
    let dir = dir();
    let folder = dir.path().join("docs");
    fs::create_dir(&folder).expect("mkdir");

    assert!(files::copy_entry(&folder, &folder).is_err());
    assert!(files::copy_entry(&folder, &folder.join("child")).is_err());
    assert!(files::move_entry(&folder, &folder).is_err());
}

#[test]
fn move_relocates_and_rejects_same_directory_moves() {
    let dir = dir();
    let source = note(dir.path(), "a.md");
    let target = dir.path().join("target");
    fs::create_dir(&target).expect("mkdir");

    let moved = files::move_entry(&source, &target).expect("move");
    assert_eq!(moved, target.join("a.md"));
    assert!(!source.exists());

    assert!(files::move_entry(&moved, &target).is_err());
}

#[test]
fn trash_moves_entries_into_a_flat_suffixed_trash() {
    let _guard = ENV_GUARD.lock().unwrap();
    let data_home = dir();
    unsafe { std::env::set_var("XDG_DATA_HOME", data_home.path()) };

    let dir = dir();
    let first = note(dir.path(), "gone.md");
    let trashed = trash::trash(&first).expect("trash");
    assert_eq!(trashed, trash::trash_dir().unwrap().join("gone.md"));
    assert!(!first.exists());

    let second = note(dir.path(), "gone.md");
    let suffixed = trash::trash(&second).expect("trash again");
    assert_eq!(suffixed, trash::trash_dir().unwrap().join("gone.1.md"));

    let missing = trash::trash(&dir.path().join("missing.md"));
    assert!(missing.unwrap_err().is_not_found());
}

#[test]
fn trash_dir_falls_back_to_home() {
    let _guard = ENV_GUARD.lock().unwrap();
    let home = dir();
    unsafe { std::env::remove_var("XDG_DATA_HOME") };
    unsafe { std::env::set_var("HOME", home.path()) };

    assert_eq!(
        trash::trash_dir().unwrap(),
        home.path().join(".local/share/naskah/trash")
    );
}
