//! The external editor command.
//!
//! Naskah never edits text itself: `e` hands the cursor entry to the editor
//! from the config file (falling back to `$EDITOR`). The command string is
//! split on whitespace so values like `code --wait` keep working. Running the
//! editor blocks until it exits; the event loop suspends the terminal around
//! [`run`] and refreshes the listing afterwards.

use std::path::Path;
use std::process::Command;

use thiserror::Error;

/// Errors from building or running the editor command.
#[derive(Debug, Error)]
pub enum EditorError {
    /// Neither the config file nor `$EDITOR` names an editor.
    #[error("no editor configured; set editor in naskah.toml or $EDITOR")]
    NoEditor,

    /// The editor program could not be started.
    #[error("could not start editor {program:?}: {source}")]
    Start {
        program: String,
        #[source]
        source: std::io::Error,
    },

    /// The editor exited with a non-zero status.
    #[error("editor exited with {status}")]
    Failed { status: String },
}

/// Split `command` into its program and arguments on whitespace.
///
/// Returns `None` when the command is blank. Quoting is not supported; use a
/// wrapper script when an argument contains spaces.
pub fn split(command: &str) -> Option<(String, Vec<String>)> {
    let mut parts = command.split_whitespace();
    let program = parts.next()?.to_owned();
    let args: Vec<String> = parts.map(str::to_owned).collect();
    Some((program, args))
}

/// Build the editor [`Command`] for `path`, appending the note path after the
/// configured arguments.
pub fn command(editor: Option<&str>, path: &Path) -> Result<Command, EditorError> {
    let configured = editor.unwrap_or_default();
    let (program, args) = split(configured).ok_or(EditorError::NoEditor)?;
    let mut command = Command::new(program);
    command.args(args).arg(path);
    Ok(command)
}

/// Run the editor on `path` and wait for it to exit.
///
/// The caller suspends the terminal before calling and restores it after, then
/// refreshes the listing. A non-zero exit still returns an error after the
/// editor has closed.
pub fn run(editor: Option<&str>, path: &Path) -> Result<(), EditorError> {
    let mut command = command(editor, path)?;
    let program = command.get_program().to_string_lossy().into_owned();
    let status = command
        .status()
        .map_err(|source| EditorError::Start { program, source })?;
    if status.success() {
        Ok(())
    } else {
        Err(EditorError::Failed {
            status: status_string(&status),
        })
    }
}

fn status_string(status: &std::process::ExitStatus) -> String {
    match status.code() {
        Some(code) => format!("status {code}"),
        None => "a signal".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_separates_the_program_from_its_arguments() {
        assert_eq!(
            split("code --wait"),
            Some(("code".to_owned(), vec!["--wait".to_owned()]))
        );
    }

    #[test]
    fn split_collapses_extra_whitespace() {
        assert_eq!(
            split("  vim   -p  "),
            Some(("vim".to_owned(), vec!["-p".to_owned()]))
        );
    }

    #[test]
    fn split_rejects_a_blank_command() {
        assert_eq!(split(""), None);
        assert_eq!(split("   "), None);
    }

    #[test]
    fn command_appends_the_note_path() {
        let command = command(Some("code --wait"), Path::new("/notes/a.md")).expect("command");

        assert_eq!(command.get_program(), "code");
        let args: Vec<_> = command
            .get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect();
        assert_eq!(args, vec!["--wait", "/notes/a.md"]);
    }

    #[test]
    fn missing_editor_is_an_error() {
        assert!(matches!(
            command(None, Path::new("/notes/a.md")),
            Err(EditorError::NoEditor)
        ));
        assert!(matches!(
            command(Some("   "), Path::new("/notes/a.md")),
            Err(EditorError::NoEditor)
        ));
    }

    #[test]
    fn unknown_program_is_a_start_error() {
        let error = run(
            Some("naskah-missing-editor-binary"),
            Path::new("/notes/a.md"),
        )
        .expect_err("starting must fail");

        assert!(matches!(error, EditorError::Start { .. }));
    }

    #[test]
    fn non_zero_exit_is_a_failure() {
        let error = run(Some("false"), Path::new("/notes/a.md")).expect_err("exit must fail");

        assert!(matches!(error, EditorError::Failed { .. }));
    }
}
