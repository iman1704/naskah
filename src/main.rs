//! Thin entrypoint: parse the CLI, load config, run the browser, and report
//! startup errors to stderr.

use std::io::{self, IsTerminal, Write as _};
use std::path::Path;
use std::process::ExitCode;
use std::time::Duration;

use clap::Parser;
use crossterm::event::{self, Event, KeyEventKind};
use naskah::app::{App, Effect, Msg};
use naskah::error::Error;
use naskah::reader::{ImageCapability, ImagePicker, ImageProtocol, detect_image_picker};
use naskah::watch::{FolderWatcher, WatchMessage};
use naskah::zellij_images::{self, DEFAULT_CELL};
use naskah::{browser, config, editor, svg, ui};

const POLL_TIMEOUT: Duration = Duration::from_millis(250);

/// Faster poll while reader images are loading, so completions from the image
/// worker reach the screen promptly.
const IMAGE_POLL_TIMEOUT: Duration = Duration::from_millis(25);

/// A TUI markdown note browser.
#[derive(Debug, Parser)]
#[command(version, about)]
struct Cli {}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("naskah: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Error> {
    Cli::parse();
    ensure_terminal()?;

    let config = config::Config::load()?;
    let entries = browser::list(&config.notes_dir)?;
    let mut app = App::new(config.notes_dir, entries);
    app.images.set_fetch_remote(config.fetch_remote_images);
    let editor_command = config.editor;

    // SVG text needs the system font database, which is slow to scan on the
    // first image. Warm it while the browser is already usable.
    let _ = std::thread::Builder::new()
        .name("naskah-svg-fonts".to_owned())
        .spawn(svg::warm_fonts);

    let mut watcher = match FolderWatcher::spawn(&app.notes_dir) {
        Ok(watcher) => Some(watcher),
        Err(error) => {
            app.refresh_failed(error.to_string());
            None
        }
    };

    let mut terminal = match ratatui::try_init() {
        Ok(terminal) => terminal,
        Err(error) => {
            ratatui::restore();
            return Err(error.into());
        }
    };
    let probe = detect_image_picker().ok();
    let zellij = in_zellij();
    app.image_picker = select_image_picker(probe.clone(), zellij);
    app.zellij_painter = zellij;
    app.zellij_cell = probe
        .map(|picker| {
            let size = picker.font_size();
            (size.width, size.height)
        })
        .unwrap_or(DEFAULT_CELL);
    let result = run_terminal(
        &mut terminal,
        &mut app,
        &mut watcher,
        editor_command.as_deref(),
    );
    ratatui::try_restore()?;
    result
}

/// Choose the reader image picker from a terminal probe.
///
/// The probe is authoritative outside Zellij: a Kitty result is used as-is,
/// and a probe that saw the Kitty capability but fell back (for example when
/// the terminal did not answer the cell-size query) is upgraded back to Kitty.
/// Inside Zellij the text fallback is always kept: Zellij's Kitty support does
/// not resolve the unicode placeholders `ratatui-image` displays through, so
/// any Kitty picker would leave blank gaps where images belong. Alt text and
/// unicode math stay readable instead.
fn select_image_picker(probed: Option<ImagePicker>, in_zellij: bool) -> Option<ImagePicker> {
    if in_zellij {
        return None;
    }
    if let Some(picker) = probed {
        if picker.protocol_type() == ImageProtocol::Kitty {
            return Some(picker);
        }
        if picker.capabilities().contains(&ImageCapability::Kitty) {
            let mut picker = picker;
            picker.set_protocol_type(ImageProtocol::Kitty);
            return Some(picker);
        }
    }
    None
}

/// Whether the app runs inside a Zellij multiplexer pane.
fn in_zellij() -> bool {
    std::env::var_os("ZELLIJ").is_some() || std::env::var_os("ZELLIJ_SESSION_NAME").is_some()
}

/// Naskah needs an interactive terminal for input and drawing.
fn ensure_terminal() -> Result<(), Error> {
    if std::io::stdin().is_terminal() && std::io::stdout().is_terminal() {
        Ok(())
    } else {
        Err(Error::NotATerminal)
    }
}

fn run_terminal(
    terminal: &mut ratatui::DefaultTerminal,
    app: &mut App,
    watcher: &mut Option<FolderWatcher>,
    editor_command: Option<&str>,
) -> Result<(), Error> {
    terminal.hide_cursor()?;
    let size = terminal.size()?;
    app.resize(size.width, size.height);
    let mut watch_attempted = app.notes_dir.clone();
    // Redraw only after something changed. Re-sending Kitty image placements
    // on every poll wakes the terminal compositor for no visible change, which
    // feels laggy on image-heavy notes.
    let mut needs_redraw = true;

    while !app.should_quit {
        // Drain every image that arrived together, then refresh and redraw
        // once so a batch of completions costs one rebuild.
        if !app.images.drain_completions().is_empty() {
            app.refresh_reader_images();
            needs_redraw = true;
        }
        if needs_redraw {
            terminal.draw(|frame| ui::draw(frame, app))?;
            needs_redraw = false;
            paint_zellij_images(app)?;
        }
        let had_reader = app.reader.is_some();

        // Drain live folder reports before polling input so external changes
        // appear promptly. Stale reports are dropped by the app.
        let reports: Vec<WatchMessage> = watcher
            .as_ref()
            .map(FolderWatcher::drain)
            .unwrap_or_default();
        let has_reports = !reports.is_empty();
        let mut effects = Vec::new();
        for message in reports {
            let msg = match message {
                WatchMessage::Changed { dir } => Msg::WatcherEvent { dir },
                WatchMessage::NoteChanged { path } => Msg::NoteChanged { path },
                WatchMessage::Failed { error } => Msg::RefreshFailed { error },
            };
            effects.extend(app.update(msg));
        }
        if has_reports || !effects.is_empty() {
            needs_redraw = true;
        }
        apply_effects(app, effects, editor_command, terminal)?;

        let timeout = if app.images.has_pending() {
            IMAGE_POLL_TIMEOUT
        } else {
            POLL_TIMEOUT
        };
        if event::poll(timeout)? {
            match event::read()? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    let effects = app.update(Msg::Key(key));
                    needs_redraw = true;
                    apply_effects(app, effects, editor_command, terminal)?;
                }
                Event::Resize(width, height) => {
                    app.update(Msg::Resize { width, height });
                    needs_redraw = true;
                }
                _ => {}
            }
        }

        if rewatch(app, watcher, &mut watch_attempted) {
            needs_redraw = true;
        }

        // The painter's ids die with the reader, so delete the last frame
        // when the reader closed. Quitting needs no cleanup: restoring the
        // terminal clears the alternate screen the images live on.
        if had_reader && app.reader.is_none() {
            let ids = std::mem::take(&mut app.zellij_ids);
            if !ids.is_empty() {
                let mut stdout = io::stdout();
                zellij_images::delete_images(&mut stdout, &ids)?;
                stdout.flush()?;
            }
        }
    }

    Ok(())
}

/// Paint Zellij reader images after the frame, using the same content
/// rectangle the reader drew. Records the transmitted ids so the next paint
/// or a reader close can delete them.
fn paint_zellij_images(app: &mut App) -> Result<(), Error> {
    let Some(reader) = app.reader.as_mut() else {
        return Ok(());
    };
    let scroll = reader.scroll();
    let Some(painter) = reader.zellij_images() else {
        return Ok(());
    };
    let area = ratatui::layout::Rect::new(0, 0, app.terminal_width, app.terminal_height);
    let content = ui::reader::content_rect(area);
    if content.width == 0 || content.height == 0 {
        return Ok(());
    }
    let mut stdout = io::stdout();
    let ids = painter.paint(
        &mut stdout,
        content.x,
        content.y,
        content.height as usize,
        scroll,
    )?;
    stdout.flush()?;
    app.zellij_ids = ids;
    Ok(())
}

fn apply_effects(
    app: &mut App,
    effects: Vec<Effect>,
    editor_command: Option<&str>,
    terminal: &mut ratatui::DefaultTerminal,
) -> Result<(), Error> {
    for effect in effects {
        match effect {
            Effect::LoadDirectory { path, reselect } => {
                let message = match browser::list(&path) {
                    Ok(entries) => Msg::DirectoryLoaded {
                        path,
                        entries,
                        reselect,
                    },
                    Err(error) => Msg::DirectoryLoadFailed { error },
                };
                let _ = app.update(message);
            }
            Effect::RefreshDirectory { dir } => {
                let message = match browser::list(&dir) {
                    Ok(entries) => Msg::RefreshLoaded { dir, entries },
                    Err(error) => Msg::RefreshFailed {
                        error: error.to_string(),
                    },
                };
                let _ = app.update(message);
            }
            Effect::OpenEditor { path } => {
                open_editor(app, editor_command, terminal, &path)?;
            }
        }
    }
    Ok(())
}

/// Suspend the terminal, run the editor to completion, restore the terminal,
/// and refresh the listing and note dates. When the reader was open, stay in
/// the reader and re-render the note at its reading position; a note that can
/// no longer be read closes the reader and reports in the browser message row.
/// Editor failures are shown in the message row; the listing refresh is
/// best-effort so it never hides them.
fn open_editor(
    app: &mut App,
    editor_command: Option<&str>,
    terminal: &mut ratatui::DefaultTerminal,
    path: &Path,
) -> Result<(), Error> {
    let had_reader = app.reader.is_some();
    ratatui::restore();
    let result = editor::run(editor_command, path);
    *terminal = ratatui::try_init()?;
    terminal.hide_cursor()?;

    if let Err(error) = result {
        app.error = Some(error.to_string());
    }
    let dir = app.notes_dir.clone();
    if let Ok(entries) = browser::list(&dir) {
        let _ = app.update(Msg::RefreshLoaded { dir, entries });
    }
    if had_reader {
        app.apply_note_changed(path);
    }
    Ok(())
}

/// Point the watcher at the current directory after navigation and at the open
/// note while reading. A missing watcher (failed spawn) is retried when the
/// directory changes, and a failed re-watch surfaces in the message row while
/// the old watch keeps running. Returns whether the app state changed.
fn rewatch(
    app: &mut App,
    watcher: &mut Option<FolderWatcher>,
    watch_attempted: &mut std::path::PathBuf,
) -> bool {
    let mut changed = false;
    match watcher.as_mut() {
        Some(active) if active.watched() == app.notes_dir => {}
        Some(active) => {
            if let Err(error) = active.watch(&app.notes_dir) {
                let _ = app.update(Msg::RefreshFailed {
                    error: error.to_string(),
                });
                changed = true;
            }
        }
        None => {
            if *watch_attempted != app.notes_dir {
                *watch_attempted = app.notes_dir.clone();
                match FolderWatcher::spawn(&app.notes_dir) {
                    Ok(active) => *watcher = Some(active),
                    Err(error) => {
                        let _ = app.update(Msg::RefreshFailed {
                            error: error.to_string(),
                        });
                        changed = true;
                    }
                }
            }
        }
    }
    changed | sync_note_watch(app, watcher)
}

/// Keep the file watch on the open note and drop it when the reader closes.
/// Failures surface in the message row; the directory watch keeps running.
/// Returns whether the app state changed.
fn sync_note_watch(app: &mut App, watcher: &mut Option<FolderWatcher>) -> bool {
    let Some(active) = watcher.as_mut() else {
        return false;
    };
    let wanted = app
        .reader
        .as_ref()
        .map(|reader| reader.path().to_path_buf());
    if active.watched_note() == wanted.as_deref() {
        return false;
    }
    if let Err(error) = active.watch_note(wanted.as_deref()) {
        let _ = app.update(Msg::RefreshFailed {
            error: error.to_string(),
        });
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_no_arguments() {
        assert!(Cli::try_parse_from(["naskah"]).is_ok());
    }

    #[test]
    fn rejects_positional_arguments() {
        assert!(Cli::try_parse_from(["naskah", "note.md"]).is_err());
    }

    #[test]
    fn keeps_a_probed_kitty_picker() {
        let mut picker = ImagePicker::halfblocks();
        picker.set_protocol_type(ImageProtocol::Kitty);

        let selected = select_image_picker(Some(picker), false);

        assert_eq!(
            selected.map(|picker| picker.protocol_type()),
            Some(ImageProtocol::Kitty)
        );
    }

    #[test]
    fn uses_the_text_fallback_inside_zellij() {
        let mut picker = ImagePicker::halfblocks();
        picker.set_protocol_type(ImageProtocol::Kitty);

        assert!(select_image_picker(Some(picker), true).is_none());
        assert!(select_image_picker(None, true).is_none());
    }

    #[test]
    fn keeps_the_text_fallback_outside_zellij() {
        assert!(select_image_picker(None, false).is_none());

        let selected = select_image_picker(Some(ImagePicker::halfblocks()), false);

        assert!(selected.is_none());
    }
}
