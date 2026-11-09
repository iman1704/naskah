use std::fs;
use std::path::PathBuf;
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use naskah::app::{App, Msg};
use naskah::browser::Entry;
use naskah::ui;
use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratatui::buffer::Buffer;

fn entry(name: &str, is_dir: bool, modified: Option<SystemTime>) -> Entry {
    Entry {
        name: name.to_owned(),
        path: PathBuf::from("/notes").join(name),
        is_dir,
        modified,
    }
}

fn folder(name: &str) -> Entry {
    entry(name, true, None)
}

fn note(name: &str) -> Entry {
    entry(name, false, Some(SystemTime::now()))
}

fn press(app: &mut App, code: KeyCode) {
    app.update(Msg::Key(KeyEvent::new(code, KeyModifiers::NONE)));
}

fn render(app: &mut App, width: u16, height: u16) -> String {
    app.update(Msg::Resize { width, height });
    let backend = TestBackend::new(width, height);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| ui::draw(frame, app))
        .expect("draw frame");
    buffer_text(terminal.backend().buffer())
}

fn buffer_text(buffer: &Buffer) -> String {
    buffer.content().iter().map(|cell| cell.symbol()).collect()
}

#[test]
fn renders_header_sections_notes_and_dates() {
    let mut app = App::new(
        PathBuf::from("/notes"),
        vec![folder("Projects"), note("First note.md"), note("Second.md")],
    );

    let text = render(&mut app, 80, 24);

    assert!(text.contains("Naskah"));
    assert!(text.contains("FOLDERS"));
    assert!(text.contains("NOTES"));
    assert!(text.contains("Projects"));
    assert!(text.contains("First note"));
    assert!(text.contains("Second"));
    assert!(!text.contains("First note.md"));
    assert!(text.contains("2 notes"));
    assert!(text.contains("1/3"));
    assert!(text.contains("Today"));
}

#[test]
fn renders_the_empty_state() {
    let mut app = App::new(PathBuf::from("/notes"), Vec::new());

    let text = render(&mut app, 80, 24);

    assert!(text.contains("no markdown notes here — a new note · A new folder"));
    assert!(text.contains("0 notes"));
    assert!(text.contains("0/0"));
}

#[test]
fn renders_the_help_overlay() {
    let mut app = App::new(PathBuf::from("/notes"), vec![note("note.md")]);
    press(&mut app, KeyCode::Char('?'));

    let text = render(&mut app, 80, 24);

    assert!(text.contains("Help"));
    assert!(text.contains("move down"));
    assert!(text.contains("close help"));
}

#[test]
fn renders_selection_clipboard_and_visual_indicators() {
    let mut app = App::new(
        PathBuf::from("/notes"),
        vec![note("First.md"), note("Second.md"), note("Third.md")],
    );

    press(&mut app, KeyCode::Char(' '));
    press(&mut app, KeyCode::Char('j'));
    press(&mut app, KeyCode::Char(' '));
    let text = render(&mut app, 80, 24);
    assert!(text.contains("✓"));
    assert!(text.contains("2 sel"));

    press(&mut app, KeyCode::Char('y'));
    let text = render(&mut app, 80, 24);
    assert!(text.contains("⧉"));
    assert!(text.contains("copy 2"));

    press(&mut app, KeyCode::Char('x'));
    let text = render(&mut app, 80, 24);
    assert!(text.contains("✂"));
    assert!(!text.contains("✓"));

    press(&mut app, KeyCode::Char('v'));
    let text = render(&mut app, 80, 24);
    assert!(text.contains("VISUAL"));
    assert!(text.contains("j/k extend"));
}

#[test]
fn renders_the_prompt_with_placeholder_and_typed_value() {
    let mut app = App::new(PathBuf::from("/notes"), vec![note("note.md")]);
    press(&mut app, KeyCode::Char('a'));

    let text = render(&mut app, 80, 24);
    assert!(text.contains("New note: "));
    assert!(text.contains("note.md"));

    press(&mut app, KeyCode::Char('h'));
    press(&mut app, KeyCode::Char('i'));
    let text = render(&mut app, 80, 24);
    assert!(text.contains("New note: "));
    assert!(text.contains("hi"));
    assert!(!text.contains("? help"));
}

#[test]
fn renders_the_trash_confirmation() {
    let mut app = App::new(PathBuf::from("/notes"), vec![note("First.md")]);
    press(&mut app, KeyCode::Char('d'));

    let text = render(&mut app, 80, 24);
    assert!(text.contains("Trash \"First.md\"? (y/N)"));
    assert!(!text.contains("? help"));
}

#[test]
fn renders_status_and_error_rows() {
    let mut app = App::new(PathBuf::from("/notes"), vec![note("note.md")]);
    app.status = Some("saved".to_owned());

    let text = render(&mut app, 80, 24);
    assert!(text.contains("saved"));
    assert!(!text.contains("? help"));

    app.status = None;
    app.error = Some("boom".to_owned());
    let text = render(&mut app, 80, 24);
    assert!(text.contains("! boom"));
}

fn reader_app(markdown: &str) -> (tempfile::TempDir, App) {
    let temp = tempfile::tempdir().expect("scratch directory");
    let path = temp.path().join("note.md");
    fs::write(&path, markdown).expect("write note");
    let entry = Entry {
        name: "note.md".to_owned(),
        path: path.clone(),
        is_dir: false,
        modified: Some(SystemTime::now()),
    };
    let app = App::new(temp.path().to_path_buf(), vec![entry]);
    (temp, app)
}

#[test]
fn renders_markdown_in_the_reader_with_header_and_help() {
    let (_temp, mut app) =
        reader_app("# Reader Title\n\nReader **body** text with a [link](https://example.com).\n");
    app.update(Msg::Resize {
        width: 80,
        height: 24,
    });
    press(&mut app, KeyCode::Enter);

    let text = render(&mut app, 80, 24);

    assert!(text.contains("Reader Title"));
    assert!(text.contains("Reader body text"));
    assert!(text.contains("example.com"));
    assert!(text.contains("100%"));
    assert!(text.contains("scroll"));
    assert!(text.contains("back"));
    assert!(!text.contains("NOTES"));
}

#[test]
fn reader_help_row_shows_watch_and_editor_errors() {
    let (_temp, mut app) = reader_app("# Title\n\nA short note.\n");
    app.update(Msg::Resize {
        width: 80,
        height: 24,
    });
    press(&mut app, KeyCode::Enter);

    app.error = Some("could not start editor".to_owned());
    let text = render(&mut app, 80, 24);
    assert!(text.contains("! could not start editor"));
    assert!(!text.contains("scroll"));

    app.error = None;
    app.watcher_error = Some("watch failed".to_owned());
    let text = render(&mut app, 80, 24);
    assert!(text.contains("! watch failed"));
    assert!(!text.contains("scroll"));
}

#[test]
fn scrolled_reader_shows_the_end_of_the_note() {
    let markdown = (0..60)
        .map(|index| format!("Paragraph {index:02}"))
        .collect::<Vec<_>>()
        .join("\n\n");
    let (_temp, mut app) = reader_app(&markdown);
    app.update(Msg::Resize {
        width: 80,
        height: 12,
    });
    press(&mut app, KeyCode::Enter);
    press(&mut app, KeyCode::Char('G'));

    let text = render(&mut app, 80, 12);

    assert!(text.contains("Paragraph 59"));
    assert!(!text.contains("Paragraph 00"));
    assert!(text.contains("100%"));
}

#[test]
fn narrow_reader_renders_without_panicking() {
    let (_temp, mut app) = reader_app("# Title\n\nA short note.\n");
    app.update(Msg::Resize {
        width: 20,
        height: 8,
    });
    press(&mut app, KeyCode::Enter);

    let text = render(&mut app, 20, 8);

    assert!(text.contains("Title"));
}

#[test]
fn reader_with_a_kitty_picker_renders_images_without_panicking() {
    use naskah::reader::{ImagePicker, ImageProtocol};

    let temp = tempfile::tempdir().expect("scratch directory");
    let picture = image::RgbImage::from_fn(20, 120, |x, y| {
        image::Rgb([(x % 256) as u8, (y % 256) as u8, 128])
    });
    picture
        .save(temp.path().join("tall.png"))
        .expect("write png");
    let path = temp.path().join("note.md");
    fs::write(&path, "![Tall image](tall.png)\n").expect("write note");
    let entry = Entry {
        name: "note.md".to_owned(),
        path: path.clone(),
        is_dir: false,
        modified: Some(SystemTime::now()),
    };
    let mut app = App::new(temp.path().to_path_buf(), vec![entry]);
    let mut picker = ImagePicker::halfblocks();
    picker.set_protocol_type(ImageProtocol::Kitty);
    app.image_picker = Some(picker);
    app.update(Msg::Resize {
        width: 80,
        height: 24,
    });
    press(&mut app, KeyCode::Enter);

    let text = render(&mut app, 80, 24);

    assert!(
        app.reader
            .as_ref()
            .is_some_and(|reader| reader.has_images())
    );
    assert!(text.contains("100%"));
    assert!(text.contains("back"));
}

#[test]
fn narrow_layout_truncates_without_panicking() {
    let mut app = App::new(
        PathBuf::from("/a/very/long/path/to/some/notes/directory"),
        vec![
            folder("A folder with a very long name"),
            note("A note with an extremely long title that should truncate.md"),
        ],
    );

    let text = render(&mut app, 30, 8);

    assert!(text.contains("Naskah"));
    assert!(text.contains("…"));
}

#[test]
fn scrolled_list_shows_the_last_entries() {
    let mut app = App::new(
        PathBuf::from("/notes"),
        (0..20)
            .map(|index| note(&format!("Note {index:02}.md")))
            .collect(),
    );
    app.update(Msg::Resize {
        width: 80,
        height: 10,
    });
    press(&mut app, KeyCode::Char('G'));

    let text = render(&mut app, 80, 10);

    assert!(text.contains("Note 19"));
    assert!(!text.contains("Note 00"));
}
