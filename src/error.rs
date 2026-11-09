//! Top-level startup errors reported to stderr by the binary.

use thiserror::Error;

use crate::browser::BrowserError;
use crate::config::ConfigError;

/// A failure that stops Naskah before the browser starts.
#[derive(Debug, Error)]
pub enum Error {
    /// Naskah was started without an interactive terminal.
    #[error("must be run in an interactive terminal")]
    NotATerminal,

    /// The configuration file could not be loaded or resolved.
    #[error(transparent)]
    Config(#[from] ConfigError),

    /// The initial directory listing could not be read.
    #[error(transparent)]
    Browser(#[from] BrowserError),

    /// The terminal could not be initialized, driven, or restored.
    #[error("terminal error: {0}")]
    Terminal(#[from] std::io::Error),
}
