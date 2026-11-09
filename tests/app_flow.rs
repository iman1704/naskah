use std::fs;
use std::path::PathBuf;
use std::sync::Mutex;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use naskah::app::{App, Effect, Msg};
use naskah::browser::{BrowserError, Entry};

/// Serializes tests that point `XDG_DATA_HOME` at a scratch directory.
static ENV_GUARD: Mutex<()> = Mutex::new(());

fn entry(name: &str, is_dir: bool) -> Entry {
    entry_at(name, &format!("/notes/{name}"), is_dir)
}

fn entry_at(name: &str, path: &str, is_dir: bool) -> Entry {
    Entry {
        name: name.to_owned(),
        path: PathBuf::from(path),
        is_dir,
        modified: None,
    }
}

fn app_with(entries: Vec<Entry>) -> App {
    App::new(PathBuf::from("/notes"), entries)
}

fn press(app: &mut App, code: KeyCode) -> Vec<Effect> {
    app.update(Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)))
}

fn type_text(app: &mut App, text: &str) {
    for c in text.chars() {
        press(app, KeyCode::Char(c));
    }
}

fn press_key(app: &mut App, code: KeyCode, modifiers: KeyModifiers) -> Vec<Effect> {
    app.update(Msg::Key(KeyEvent::new(code, modifiers)))
}

/// Build an app rooted at a scratch directory containing `names` (`"docs/"`
/// creates a folder, anything else an empty note).
fn temp_app(names: &[&str]) -> (tempfile::TempDir, App) {
    let temp = tempfile::tempdir().expect("scratch directory");
    let mut entries = Vec::new();
    for name in names {
        if let Some(folder) = name.strip_suffix('/') {
            fs::create_dir(temp.path().join(folder)).expect("mkdir");
            entries.push(Entry {
                name: folder.to_owned(),
                path: temp.path().join(folder),
                is_dir: true,
                modified: None,
            });
        } else {
            fs::write(temp.path().join(name), "# note").expect("write note");
            entries.push(Entry {
                name: name.to_string(),
                path: temp.path().join(name),
                is_dir: false,
                modified: None,
            });
        }
    }
    entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then(a.name.cmp(&b.name)));
    let app = App::new(temp.path().to_path_buf(), entries);
    (temp, app)
}

#[test]
fn moves_between_entries_and_clamps_at_the_ends() {
    let mut app = app_with(vec![
        entry("one.md", false),
        entry("two.md", false),
        entry("three.md", false),
    ]);

    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.cursor, 2);

    press(&mut app, KeyCode::Char('k'));
    assert_eq!(app.cursor, 1);

    press(&mut app, KeyCode::Char('G'));
    assert_eq!(app.cursor, 2);
    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.cursor, 0);
}

#[test]
fn page_keys_move_by_the_viewport_height() {
    let mut app = app_with(
        (0..20)
            .map(|index| entry(&format!("note-{index}.md"), false))
            .collect(),
    );
    app.resize(80, 8);

    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.cursor, 5);
    press(&mut app, KeyCode::PageDown);
    assert_eq!(app.cursor, 10);
    press(&mut app, KeyCode::PageUp);
    assert_eq!(app.cursor, 5);
}

#[test]
fn scrolling_tracks_section_rows_and_resize_clamps() {
    let mut app = app_with(
        (0..20)
            .map(|index| entry(&format!("note-{index}.md"), false))
            .collect(),
    );
    app.resize(80, 8);

    press(&mut app, KeyCode::Char('G'));
    assert_eq!(app.cursor, 19);
    assert_eq!(app.scroll, 16);

    app.resize(80, 24);
    assert_eq!(app.scroll, 0);
}

#[test]
fn help_overlay_toggles_and_esc_closes_it() {
    let mut app = app_with(vec![entry("note.md", false)]);

    press(&mut app, KeyCode::Char('?'));
    assert!(app.help_visible);

    press(&mut app, KeyCode::Char('j'));
    assert!(app.help_visible);

    press(&mut app, KeyCode::Esc);
    assert!(!app.help_visible);
}

#[test]
fn space_toggles_selection_and_esc_clears_it() {
    let mut app = app_with(vec![entry("one.md", false), entry("two.md", false)]);

    press(&mut app, KeyCode::Char(' '));
    assert_eq!(app.selection.len(), 1);
    assert!(app.entry_selected(0));

    press(&mut app, KeyCode::Char(' '));
    assert!(app.selection.is_empty());

    press(&mut app, KeyCode::Char(' '));
    let effects = press(&mut app, KeyCode::Esc);
    assert!(effects.is_empty());
    assert!(app.selection.is_empty());
    assert_eq!(app.status.as_deref(), Some("cancelled"));
}

#[test]
fn esc_with_nothing_to_clear_reports_nothing() {
    let mut app = app_with(vec![entry("note.md", false)]);

    press(&mut app, KeyCode::Esc);

    assert_eq!(app.status, None);
}

#[test]
fn esc_closes_help_before_clearing_selection() {
    let mut app = app_with(vec![entry("note.md", false)]);
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('?'));

    press(&mut app, KeyCode::Esc);
    assert!(!app.help_visible);
    assert_eq!(app.selection.len(), 1);

    press(&mut app, KeyCode::Esc);
    assert!(app.selection.is_empty());
}

#[test]
fn visual_range_extends_with_movement_and_commits_with_v() {
    let mut app = app_with(vec![
        entry("one.md", false),
        entry("two.md", false),
        entry("three.md", false),
    ]);

    press(&mut app, KeyCode::Char('v'));
    assert!(app.visual);

    press(&mut app, KeyCode::Char('j'));
    assert!(app.entry_selected(0));
    assert!(app.entry_selected(1));
    assert!(!app.entry_selected(2));

    press(&mut app, KeyCode::Char('v'));
    assert!(!app.visual);
    assert_eq!(app.selection.len(), 2);
}

#[test]
fn space_is_ignored_while_visual_is_active() {
    let mut app = app_with(vec![entry("note.md", false)]);

    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char(' '));

    assert!(app.visual);
    assert!(app.selection.is_empty());
}

#[test]
fn other_keys_commit_the_visual_range_first() {
    let mut app = app_with(vec![
        entry("one.md", false),
        entry("two.md", false),
        entry("three.md", false),
    ]);

    press(&mut app, KeyCode::Char('v'));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('G'));

    assert!(!app.visual);
    assert_eq!(app.selection.len(), 2);
    assert_eq!(app.cursor, 2);
}

#[test]
fn copy_and_cut_queue_the_selection_and_keep_it() {
    let mut app = app_with(vec![entry("one.md", false), entry("two.md", false)]);
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char(' '));

    press(&mut app, KeyCode::Char('y'));
    assert_eq!(app.clipboard.entries.len(), 2);
    assert!(!app.clipboard.cut);
    assert_eq!(app.status.as_deref(), Some("copied 2 items"));
    assert_eq!(app.selection.len(), 2);

    press(&mut app, KeyCode::Char('x'));
    assert!(app.clipboard.cut);
    assert_eq!(app.status.as_deref(), Some("cut 2 items"));
    assert_eq!(app.selection.len(), 2);
}

#[test]
fn copy_falls_back_to_the_cursor_entry() {
    let mut app = app_with(vec![entry("note.md", false)]);

    press(&mut app, KeyCode::Char('y'));

    assert_eq!(app.clipboard.entries.len(), 1);
    assert_eq!(app.status.as_deref(), Some("copied note.md"));
}

#[test]
fn clipboard_survives_navigation_but_selection_does_not() {
    let mut app = app_with(vec![entry("folder", true), entry("note.md", false)]);
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('y'));
    assert!(app.clipboard.active());

    app.update(Msg::DirectoryLoaded {
        path: PathBuf::from("/notes/folder"),
        entries: vec![entry_at("inner", "/notes/folder/inner", true)],
        reselect: None,
    });

    assert!(app.clipboard.active());
    assert!(app.selection.is_empty());
    assert!(!app.visual);

    press(&mut app, KeyCode::Esc);
    assert!(!app.clipboard.active());
    assert_eq!(app.status.as_deref(), Some("cancelled"));
}

#[test]
fn q_and_ctrl_c_quit() {
    let mut app = app_with(Vec::new());
    press(&mut app, KeyCode::Char('q'));
    assert!(app.should_quit);

    let mut app = app_with(Vec::new());
    app.update(Msg::Key(KeyEvent::new(
        KeyCode::Char('c'),
        KeyModifiers::CONTROL,
    )));
    assert!(app.should_quit);
}

#[test]
fn opening_a_note_shows_the_reader_and_back_keys_return_to_the_browser() {
    let (_temp, mut app) = temp_app(&["note.md"]);
    app.resize(80, 24);

    let effects = press(&mut app, KeyCode::Enter);

    assert!(effects.is_empty());
    let reader = app.reader.as_ref().expect("reader opened");
    assert_eq!(reader.path(), app.entries[0].path);

    press(&mut app, KeyCode::Esc);
    assert!(app.reader.is_none());
    assert_eq!(app.cursor, 0);
    assert!(!app.should_quit);
}

#[test]
fn reader_keys_scroll_and_clamp_without_leaving_the_note() {
    let (temp, mut app) = temp_app(&["note.md"]);
    let markdown = (0..60)
        .map(|index| format!("Paragraph {index:02}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    fs::write(temp.path().join("note.md"), markdown).expect("write long note");
    app.resize(80, 12);
    press(&mut app, KeyCode::Enter);

    press(&mut app, KeyCode::Char('j'));
    assert_eq!(app.reader.as_ref().expect("reader").scroll(), 1);

    press(&mut app, KeyCode::PageDown);
    assert!(app.reader.as_ref().expect("reader").scroll() > 1);

    press(&mut app, KeyCode::Char('G'));
    let reader = app.reader.as_ref().expect("reader");
    assert_eq!(reader.scroll(), reader.max_scroll());
    assert_eq!(reader.percent(), 100);

    press(&mut app, KeyCode::Char('g'));
    assert_eq!(app.reader.as_ref().expect("reader").scroll(), 0);

    press(&mut app, KeyCode::Char('q'));
    assert!(app.reader.is_none());
    assert!(!app.should_quit);
}

#[test]
fn ctrl_c_quits_from_the_reader() {
    let (_temp, mut app) = temp_app(&["note.md"]);
    app.resize(80, 24);
    press(&mut app, KeyCode::Enter);

    press_key(&mut app, KeyCode::Char('c'), KeyModifiers::CONTROL);

    assert!(app.should_quit);
}

#[test]
fn e_in_the_reader_requests_the_editor_for_the_open_note() {
    let (temp, mut app) = temp_app(&["note.md"]);
    app.resize(80, 24);
    press(&mut app, KeyCode::Enter);

    let effects = press(&mut app, KeyCode::Char('e'));

    assert!(app.reader.is_some());
    assert_eq!(
        effects,
        vec![Effect::OpenEditor {
            path: temp.path().join("note.md"),
        }]
    );
}

#[test]
fn reader_keys_clear_the_error_but_keep_the_watcher_error() {
    let (_temp, mut app) = temp_app(&["note.md"]);
    app.resize(80, 24);
    press(&mut app, KeyCode::Enter);
    app.error = Some("editor failed".to_owned());
    app.watcher_error = Some("watch failed".to_owned());

    press(&mut app, KeyCode::Char('j'));

    assert!(app.reader.is_some());
    assert!(app.error.is_none());
    assert_eq!(app.watcher_error.as_deref(), Some("watch failed"));
}

#[test]
fn note_changed_reloads_the_open_note() {
    let (temp, mut app) = temp_app(&["note.md"]);
    app.resize(80, 24);
    press(&mut app, KeyCode::Enter);
    let path = temp.path().join("note.md");

    fs::write(&path, "# Updated\n").expect("rewrite note");
    let effects = app.update(Msg::NoteChanged { path: path.clone() });

    assert!(effects.is_empty());
    assert!(app.reader.is_some());
    assert!(app.error.is_none());

    fs::remove_file(&path).expect("remove note");
    app.update(Msg::NoteChanged { path });

    assert!(app.reader.is_none());
    assert!(app.error.is_some());
}

#[test]
fn a_note_that_cannot_be_read_stays_in_the_browser() {
    let mut app = app_with(vec![entry("missing.md", false)]);
    app.resize(80, 24);

    press(&mut app, KeyCode::Enter);

    assert!(app.reader.is_none());
    assert!(
        app.error
            .as_deref()
            .is_some_and(|error| error.contains("missing.md"))
    );
}

#[test]
fn opening_a_folder_requests_a_new_listing() {
    let mut app = app_with(vec![entry("folder", true)]);

    let effects = press(&mut app, KeyCode::Char('l'));

    assert_eq!(
        effects,
        [Effect::LoadDirectory {
            path: PathBuf::from("/notes/folder"),
            reselect: None,
        }]
    );
}

#[test]
fn parent_navigation_stops_at_the_notes_root() {
    let mut app = app_with(Vec::new());

    assert!(press(&mut app, KeyCode::Char('h')).is_empty());

    app.update(Msg::DirectoryLoaded {
        path: PathBuf::from("/notes/sub"),
        entries: Vec::new(),
        reselect: None,
    });
    let effects = press(&mut app, KeyCode::Char('h'));

    assert_eq!(
        effects,
        [Effect::LoadDirectory {
            path: PathBuf::from("/notes"),
            reselect: Some(PathBuf::from("/notes/sub")),
        }]
    );
}

#[test]
fn loading_a_parent_reselects_the_child_it_came_from() {
    let mut app = app_with(Vec::new());

    app.update(Msg::DirectoryLoaded {
        path: PathBuf::from("/notes"),
        entries: vec![
            entry_at("alpha", "/notes/alpha", true),
            entry_at("beta", "/notes/beta", true),
        ],
        reselect: Some(PathBuf::from("/notes/beta")),
    });

    assert_eq!(app.cursor, 1);
    assert_eq!(app.notes_dir, PathBuf::from("/notes"));
}

#[test]
fn a_failed_listing_keeps_the_current_entries_and_shows_an_error() {
    let mut app = app_with(vec![entry("note.md", false)]);
    let error = BrowserError::ReadDir {
        path: PathBuf::from("/notes/missing"),
        source: std::io::Error::new(std::io::ErrorKind::NotFound, "missing"),
    };

    app.update(Msg::DirectoryLoadFailed { error });

    assert_eq!(app.entries.len(), 1);
    assert_eq!(app.notes_dir, PathBuf::from("/notes"));
    assert!(app.error.is_some());
}

#[test]
fn new_note_prompt_creates_a_file_and_reselects_it() {
    let (temp, mut app) = temp_app(&["a.md"]);

    press(&mut app, KeyCode::Char('a'));
    assert!(app.prompt.is_some());
    type_text(&mut app, "hello");
    let effects = press(&mut app, KeyCode::Enter);

    assert!(temp.path().join("hello.md").exists());
    assert_eq!(app.status.as_deref(), Some("created hello.md"));
    assert_eq!(
        effects,
        [Effect::LoadDirectory {
            path: temp.path().to_path_buf(),
            reselect: Some(temp.path().join("hello.md")),
        }]
    );

    let entries = naskah::browser::list(temp.path()).expect("relist");
    app.update(Msg::DirectoryLoaded {
        path: temp.path().to_path_buf(),
        entries,
        reselect: Some(temp.path().join("hello.md")),
    });
    assert_eq!(app.entries[app.cursor].name, "hello.md");
    assert!(app.prompt.is_none());
}

#[test]
fn new_folder_prompt_creates_a_folder_with_a_slashed_status() {
    let (temp, mut app) = temp_app(&["a.md"]);

    press(&mut app, KeyCode::Char('A'));
    type_text(&mut app, "docs");
    press(&mut app, KeyCode::Enter);

    assert!(temp.path().join("docs").is_dir());
    assert_eq!(app.status.as_deref(), Some("created docs/"));
}

#[test]
fn prompt_validation_keeps_the_prompt_open_and_esc_cancels() {
    let (_temp, mut app) = temp_app(&["a.md"]);

    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Enter);

    assert!(app.prompt.is_some());
    assert_eq!(app.error.as_deref(), Some("name cannot be empty"));

    press(&mut app, KeyCode::Esc);
    assert!(app.prompt.is_none());
    assert_eq!(app.error, None);
    assert_eq!(app.status, None);
}

#[test]
fn typing_q_into_a_prompt_does_not_quit() {
    let (_temp, mut app) = temp_app(&["a.md"]);

    press(&mut app, KeyCode::Char('a'));
    press(&mut app, KeyCode::Char('q'));

    assert!(!app.should_quit);
    assert_eq!(
        app.prompt.as_ref().map(|prompt| prompt.value.as_str()),
        Some("q")
    );
}

#[test]
fn rename_uses_the_prefill_and_follows_clipboard_paths() {
    let (temp, mut app) = temp_app(&["a.md", "b.md"]);

    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('y'));
    press(&mut app, KeyCode::Char('k'));

    press(&mut app, KeyCode::Char('R'));
    assert_eq!(
        app.prompt.as_ref().map(|prompt| prompt.value.as_str()),
        Some("a.md")
    );
    press(&mut app, KeyCode::Home);
    type_text(&mut app, "z-");
    let effects = press(&mut app, KeyCode::Enter);

    assert!(temp.path().join("z-a.md").exists());
    assert!(!temp.path().join("a.md").exists());
    assert_eq!(app.status.as_deref(), Some("renamed a.md to z-a.md"));
    assert_eq!(
        effects,
        [Effect::LoadDirectory {
            path: temp.path().to_path_buf(),
            reselect: Some(temp.path().join("z-a.md")),
        }]
    );
    assert_eq!(app.clipboard.entries[0].path, temp.path().join("z-a.md"));
    assert_eq!(app.clipboard.entries[1].path, temp.path().join("b.md"));
}

#[test]
fn rename_unchanged_reports_without_refreshing() {
    let (_temp, mut app) = temp_app(&["a.md"]);

    press(&mut app, KeyCode::Char('R'));
    let effects = press(&mut app, KeyCode::Enter);

    assert!(effects.is_empty());
    assert_eq!(app.status.as_deref(), Some("name unchanged"));
}

#[test]
fn trash_confirmation_cancels_and_confirms() {
    let _guard = ENV_GUARD.lock().unwrap();
    let data_home = tempfile::tempdir().expect("data home");
    unsafe { std::env::set_var("XDG_DATA_HOME", data_home.path()) };
    let (temp, mut app) = temp_app(&["a.md", "b.md"]);

    press(&mut app, KeyCode::Char('d'));
    assert!(app.confirm.is_some());
    press(&mut app, KeyCode::Char('n'));
    assert!(app.confirm.is_none());
    assert_eq!(app.status.as_deref(), Some("cancelled"));
    assert!(temp.path().join("a.md").exists());

    press(&mut app, KeyCode::Char('d'));
    let effects = press_key(&mut app, KeyCode::Char('Y'), KeyModifiers::SHIFT);

    assert!(!temp.path().join("a.md").exists());
    assert!(data_home.path().join("naskah/trash/a.md").exists());
    assert_eq!(app.status.as_deref(), Some("trashed a.md"));
    assert_eq!(
        effects,
        [Effect::LoadDirectory {
            path: temp.path().to_path_buf(),
            reselect: Some(temp.path().join("b.md")),
        }]
    );
}

#[test]
fn trash_moves_several_entries_with_a_count_status() {
    let _guard = ENV_GUARD.lock().unwrap();
    let data_home = tempfile::tempdir().expect("data home");
    unsafe { std::env::set_var("XDG_DATA_HOME", data_home.path()) };
    let (temp, mut app) = temp_app(&["a.md", "b.md"]);

    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('d'));
    assert_eq!(
        app.confirm.as_ref().map(|confirm| confirm.question()),
        Some("Trash 2 items? (y/N)".to_owned())
    );
    let effects = press(&mut app, KeyCode::Char('y'));

    assert!(!temp.path().join("a.md").exists());
    assert!(!temp.path().join("b.md").exists());
    assert_eq!(app.status.as_deref(), Some("trashed 2 items"));
    assert_eq!(
        effects,
        [Effect::LoadDirectory {
            path: temp.path().to_path_buf(),
            reselect: None,
        }]
    );
}

#[test]
fn paste_copy_collides_and_keeps_the_clipboard_for_repeated_pastes() {
    let (temp, mut app) = temp_app(&["a.md"]);

    press(&mut app, KeyCode::Char('y'));
    let effects = press(&mut app, KeyCode::Char('p'));

    assert!(temp.path().join("a.1.md").exists());
    assert_eq!(app.clipboard.entries.len(), 1);
    assert_eq!(app.status.as_deref(), Some("copied a.md to a.1.md"));
    assert_eq!(
        effects,
        [Effect::LoadDirectory {
            path: temp.path().to_path_buf(),
            reselect: Some(temp.path().join("a.1.md")),
        }]
    );
}

#[test]
fn paste_cut_moves_into_another_directory_and_clears_the_clipboard() {
    let (temp, mut app) = temp_app(&["a.md", "sub/"]);

    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char('x'));
    app.update(Msg::DirectoryLoaded {
        path: temp.path().join("sub"),
        entries: Vec::new(),
        reselect: None,
    });
    let effects = press(&mut app, KeyCode::Char('p'));

    assert!(!temp.path().join("a.md").exists());
    assert!(temp.path().join("sub/a.md").exists());
    assert!(!app.clipboard.active());
    assert_eq!(app.status.as_deref(), Some("moved a.md"));
    assert_eq!(
        effects,
        [Effect::LoadDirectory {
            path: temp.path().join("sub"),
            reselect: Some(temp.path().join("sub/a.md")),
        }]
    );
}

#[test]
fn cut_paste_into_the_same_folder_reports_already_here() {
    let (_temp, mut app) = temp_app(&["a.md"]);

    press(&mut app, KeyCode::Char('x'));
    press(&mut app, KeyCode::Char('p'));

    assert_eq!(
        app.status.as_deref(),
        Some("a.md is already in this folder")
    );
    assert!(app.clipboard.active());
}

#[test]
fn paste_reports_a_missing_source_and_drops_it() {
    let (temp, mut app) = temp_app(&["a.md"]);

    press(&mut app, KeyCode::Char('y'));
    fs::remove_file(temp.path().join("a.md")).expect("remove source");
    let effects = press(&mut app, KeyCode::Char('p'));

    assert!(effects.is_empty());
    assert!(!app.clipboard.active());
    let status = app.status.as_deref().expect("partial summary");
    assert!(status.starts_with("copied 0 of 1 items: "), "{status}");
}

#[test]
fn paste_continues_after_a_failure_with_a_partial_summary() {
    let (temp, mut app) = temp_app(&["a.md", "b.md"]);

    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('y'));
    fs::remove_file(temp.path().join("a.md")).expect("remove source");
    let effects = press(&mut app, KeyCode::Char('p'));

    assert!(temp.path().join("b.1.md").exists());
    assert_eq!(app.clipboard.entries.len(), 1);
    let status = app.status.as_deref().expect("partial summary");
    assert!(status.starts_with("copied 1 of 2 items: "), "{status}");
    assert_eq!(effects.len(), 1);
}

#[test]
fn e_opens_the_cursor_note_in_the_editor() {
    let mut app = app_with(vec![entry("note.md", false)]);

    let effects = press(&mut app, KeyCode::Char('e'));

    assert_eq!(
        effects,
        vec![Effect::OpenEditor {
            path: PathBuf::from("/notes/note.md"),
        }]
    );
}

#[test]
fn e_refuses_folders_and_empty_listings() {
    let mut app = app_with(vec![entry("docs", true)]);
    press(&mut app, KeyCode::Char('e'));
    assert_eq!(app.status.as_deref(), Some("cannot edit a folder"));

    let mut empty = app_with(vec![]);
    press(&mut empty, KeyCode::Char('e'));
    assert_eq!(empty.status.as_deref(), Some("nothing selected"));
}

#[test]
fn watcher_events_refresh_only_the_current_directory() {
    let mut app = app_with(vec![entry("a.md", false)]);

    let stale = app.update(Msg::WatcherEvent {
        dir: PathBuf::from("/other"),
    });
    assert!(stale.is_empty());

    let effects = app.update(Msg::WatcherEvent {
        dir: PathBuf::from("/notes"),
    });
    assert_eq!(
        effects,
        vec![Effect::RefreshDirectory {
            dir: PathBuf::from("/notes"),
        }]
    );
}

#[test]
fn failed_refresh_keeps_the_listing_until_navigation() {
    let mut app = app_with(vec![entry("a.md", false)]);

    app.update(Msg::RefreshFailed {
        error: "watcher error".to_owned(),
    });

    assert_eq!(app.entries.len(), 1);
    assert_eq!(app.watcher_error.as_deref(), Some("watcher error"));

    app.update(Msg::DirectoryLoaded {
        path: PathBuf::from("/notes"),
        entries: vec![entry("a.md", false)],
        reselect: None,
    });
    assert!(app.watcher_error.is_none());
}
