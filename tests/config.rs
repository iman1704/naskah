use std::fs;
use std::path::Path;

use naskah::config::{Config, ConfigError};
use tempfile::tempdir;

fn write_config(path: &Path, contents: &str) {
    fs::create_dir_all(path.parent().expect("config parent")).expect("create config directory");
    fs::write(path, contents).expect("write config");
}

#[test]
fn creates_a_missing_config_with_defaults() {
    let temp = tempdir().expect("temp dir");
    let config_path = temp.path().join("config/naskah/naskah.toml");
    let cwd = temp.path().join("notes");

    let config = Config::load_from(&config_path, &cwd, None, None).expect("load config");

    assert_eq!(config.notes_dir, cwd);
    assert_eq!(config.editor, None);
    assert!(config.fetch_remote_images);

    let contents = fs::read_to_string(&config_path).expect("read created config");
    assert!(contents.contains("notes_dir = \"\""));
    assert!(contents.contains("editor = \"\""));
    assert!(contents.contains("fetch_remote_images = true"));
}

#[test]
fn remote_images_are_enabled_by_default_and_can_be_disabled() {
    let temp = tempdir().expect("temp dir");
    let config_path = temp.path().join("naskah.toml");
    let cwd = temp.path().join("work");

    write_config(&config_path, "");
    let config = Config::load_from(&config_path, &cwd, None, None).expect("load config");
    assert!(config.fetch_remote_images);

    write_config(&config_path, "fetch_remote_images = false\n");
    let config = Config::load_from(&config_path, &cwd, None, None).expect("load config");
    assert!(!config.fetch_remote_images);
}

#[test]
fn resolves_empty_relative_and_home_notes_dirs() {
    let temp = tempdir().expect("temp dir");
    let config_path = temp.path().join("naskah.toml");
    let cwd = temp.path().join("work");
    let home = temp.path().join("home");

    write_config(&config_path, "");
    assert_eq!(
        Config::load_from(&config_path, &cwd, Some(&home), None)
            .expect("load config")
            .notes_dir,
        cwd
    );

    write_config(&config_path, "notes_dir = \"notes\"\n");
    assert_eq!(
        Config::load_from(&config_path, &cwd, Some(&home), None)
            .expect("load config")
            .notes_dir,
        cwd.join("notes")
    );

    write_config(&config_path, "notes_dir = \"~/notes\"\n");
    assert_eq!(
        Config::load_from(&config_path, &cwd, Some(&home), None)
            .expect("load config")
            .notes_dir,
        home.join("notes")
    );
}

#[test]
fn ignores_unknown_keys_without_rewriting_the_file() {
    let temp = tempdir().expect("temp dir");
    let config_path = temp.path().join("naskah.toml");
    let cwd = temp.path().join("work");
    let original = "notes_dir = \"notes\"\npager = \"less\"\nfuture = true\n";
    write_config(&config_path, original);

    let config = Config::load_from(&config_path, &cwd, None, None).expect("load config");

    assert_eq!(config.notes_dir, cwd.join("notes"));
    assert_eq!(
        fs::read_to_string(&config_path).expect("read config"),
        original
    );
}

#[test]
fn prefers_the_configured_editor_then_the_environment() {
    let temp = tempdir().expect("temp dir");
    let config_path = temp.path().join("naskah.toml");
    let cwd = temp.path().join("work");

    write_config(&config_path, "editor = \"code --wait\"\n");
    let config = Config::load_from(&config_path, &cwd, None, Some("vim")).expect("load config");
    assert_eq!(config.editor.as_deref(), Some("code --wait"));

    write_config(&config_path, "");
    let config = Config::load_from(&config_path, &cwd, None, Some("vim")).expect("load config");
    assert_eq!(config.editor.as_deref(), Some("vim"));
}

#[test]
fn reports_invalid_toml() {
    let temp = tempdir().expect("temp dir");
    let config_path = temp.path().join("naskah.toml");
    write_config(&config_path, "notes_dir = [\n");

    let error = Config::load_from(&config_path, temp.path(), None, None).expect_err("invalid TOML");

    assert!(matches!(error, ConfigError::Parse { .. }));
}

#[test]
fn reports_missing_home_for_tilde_paths() {
    let temp = tempdir().expect("temp dir");
    let config_path = temp.path().join("naskah.toml");
    write_config(&config_path, "notes_dir = \"~/notes\"\n");

    let error = Config::load_from(&config_path, temp.path(), None, None).expect_err("missing HOME");

    assert!(matches!(error, ConfigError::NoHome));
}
