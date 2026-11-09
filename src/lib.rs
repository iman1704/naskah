//! Naskah — a TUI markdown note browser.
//!
//! The binary lives in `main.rs`. This library exposes the modules so
//! integration tests can exercise Naskah without launching a terminal.

pub mod app;
pub mod browser;
pub mod config;
pub mod editor;
pub mod error;
pub mod files;
pub mod images;
pub mod reader;
pub mod svg;
pub mod ui;
pub mod watch;
pub mod zellij_images;
