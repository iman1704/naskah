//! Screen regions for the browser layout.

use ratatui::layout::{Constraint, Layout, Rect};

/// The four horizontal regions of the browser.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Areas {
    pub header: Rect,
    pub message: Rect,
    pub list: Rect,
    pub status: Rect,
}

/// Split the terminal into header, message, list, and status rows.
pub fn split(area: Rect) -> Areas {
    let [header, message, list, status] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Min(0),
        Constraint::Length(1),
    ])
    .areas(area);

    Areas {
        header,
        message,
        list,
        status,
    }
}
