use std::fs;

use naskah::browser;
use tempfile::tempdir;

#[test]
fn lists_folders_first_then_markdown_notes() {
    let temp = tempdir().expect("temp dir");
    let dir = temp.path();

    fs::create_dir(dir.join("beta")).expect("create beta");
    fs::create_dir(dir.join("Alpha")).expect("create Alpha");
    fs::create_dir(dir.join("folder.md")).expect("create folder.md");
    fs::create_dir(dir.join(".hidden")).expect("create hidden dir");
    fs::write(dir.join("note.md"), "note").expect("write note");
    fs::write(dir.join("UPPER.MD"), "note").expect("write UPPER");
    fs::write(dir.join("image.png"), "image").expect("write image");
    fs::write(dir.join(".hidden.md"), "hidden").expect("write hidden note");
    fs::write(dir.join("draft.txt"), "draft").expect("write draft");

    let entries = browser::list(dir).expect("list directory");
    let names: Vec<&str> = entries.iter().map(|entry| entry.name.as_str()).collect();

    assert_eq!(names, ["Alpha", "beta", "folder.md", "note.md", "UPPER.MD"]);
    assert!(entries[0].is_dir);
    assert!(entries[2].is_dir);
    assert!(!entries[3].is_dir);
    assert_eq!(entries[3].path, dir.join("note.md"));
    assert!(entries[3].modified.is_some());
}

#[test]
fn empty_directory_returns_no_entries() {
    let temp = tempdir().expect("temp dir");

    let entries = browser::list(temp.path()).expect("list directory");

    assert!(entries.is_empty());
}

#[test]
fn missing_directory_is_an_error() {
    let temp = tempdir().expect("temp dir");

    let error = browser::list(&temp.path().join("missing")).expect_err("missing directory");

    assert!(matches!(error, browser::BrowserError::ReadDir { .. }));
}

#[cfg(unix)]
#[test]
fn follows_symlinked_directories_and_skips_dangling_links() {
    use std::os::unix::fs::symlink;

    let temp = tempdir().expect("temp dir");
    let dir = temp.path();
    fs::create_dir(dir.join("real")).expect("create real directory");
    symlink(dir.join("real"), dir.join("linked")).expect("link directory");
    symlink(dir.join("missing.md"), dir.join("broken.md")).expect("link missing note");

    let entries = browser::list(dir).expect("list directory");

    assert!(
        entries
            .iter()
            .any(|entry| entry.name == "linked" && entry.is_dir)
    );
    assert!(!entries.iter().any(|entry| entry.name == "broken.md"));
}
