//! Status-row rendering for prompts and the trash confirmation.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;
use unicode_width::UnicodeWidthStr;

use crate::app::{Confirm, Prompt};
use crate::ui::{text, theme};

/// Draw the active prompt: its label, the typed value with a block cursor,
/// and a dim placeholder while the value is empty.
pub fn draw_prompt(frame: &mut Frame, prompt: &Prompt, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let label = format!(" {}", prompt.label());
    let avail = (area.width as usize).saturating_sub(UnicodeWidthStr::width(label.as_str()));
    let mut spans = vec![Span::styled(label, theme::brand())];
    if prompt.value.is_empty() {
        spans.push(Span::styled(
            text::truncate_right(prompt.placeholder(), avail),
            theme::placeholder(),
        ));
    } else {
        spans.extend(value_spans(prompt, avail));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// Split the value around the cursor so the cursor cell stands out, keeping
/// the tail visible when the value is wider than the row.
fn value_spans(prompt: &Prompt, avail: usize) -> Vec<Span<'static>> {
    let before = &prompt.value[..prompt.cursor];
    let mut after = prompt.value[prompt.cursor..].chars();
    let at = after.next().unwrap_or(' ');
    let rest: String = after.collect();

    let before_width = UnicodeWidthStr::width(before);
    let at_width = UnicodeWidthStr::width(at.to_string().as_str());
    if before_width + at_width + UnicodeWidthStr::width(rest.as_str()) <= avail {
        return vec![
            Span::raw(before.to_owned()),
            Span::styled(at.to_string(), theme::prompt_cursor()),
            Span::raw(rest),
        ];
    }
    let visible = text::truncate_left(before, avail.saturating_sub(1));
    vec![
        Span::raw(visible),
        Span::styled(at.to_string(), theme::prompt_cursor()),
    ]
}

/// Draw the `Trash "name"? (y/N)` confirmation.
pub fn draw_confirm(frame: &mut Frame, confirm: &Confirm, area: Rect) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let line = format!(" {}", confirm.question());
    frame.render_widget(Paragraph::new(Span::styled(line, theme::warning())), area);
}
