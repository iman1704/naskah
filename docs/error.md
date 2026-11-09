# `error`

Source: `src/error.rs`

## Purpose

Defines the error type for failures that stop Naskah during startup. The binary prints these errors to stderr and exits with a non-zero status.

## What it handles

- One typed `Error` enum for startup failures.
- `thiserror`-derived `Display` messages, so the binary can print `naskah: <message>` without per-variant formatting.
- The terminal check, which rejects runs without an interactive stdin and stdout.
- Configuration errors, wrapped transparently from `ConfigError`.
- Initial listing errors, wrapped transparently from `BrowserError`.
- Terminal initialization, event-loop, and restore errors from `std::io`.

## What it does not handle

- Runtime errors shown in the UI, such as a failed navigation listing. Those never reach this type.
- Logging, retries, or recovery. A startup error ends the process.
- Errors from modules that do not exist yet; their variants will be added here as those modules land.

## Dependencies

- `thiserror` — derives `Error` and `Display`.
- `crate::config` — `ConfigError`.
- `crate::browser` — `BrowserError`.
- `std::io` — terminal errors.

## Public API

```rust
pub enum Error {
    NotATerminal,
    Config(ConfigError),
    Browser(BrowserError),
    Terminal(std::io::Error),
}
```

- `Error::NotATerminal` is returned when either stdin or stdout is not a terminal.
- `Error::Config` carries configuration load failures unchanged.
- `Error::Browser` carries a failure to read the initial notes directory.
- `Error::Terminal` carries terminal setup, polling, drawing, or restore failures.
