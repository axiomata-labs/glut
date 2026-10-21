//! Draws a frame: the header, the stream (or Glimm's welcome), an open question, the input and a line of keys.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::transcript::wrap;
use crate::{glimm, session, theme};

/// The most rows the input grows to before it scrolls.
const INPUT_ROWS: usize = 8;
/// The most rows of its target an ask shows.
const ASK_ROWS: usize = 12;

pub fn draw(frame: &mut Frame, app: &mut App) {
    let area = frame.area();
    let input_width = usize::from(area.width.saturating_sub(2));
    let (rows, cursor) = app.input.layout(input_width);
    let input_height = rows.len().clamp(1, INPUT_ROWS) as u16 + 1;
    let ask_rows = app.ask.as_ref().map(|open| {
        let most = usize::from(area.height / 3).clamp(3, ASK_ROWS);
        asked_rows(
            &open.request.target,
            usize::from(area.width.saturating_sub(4)),
            most,
        )
    });
    let ask_height = ask_rows.as_ref().map_or(0, |rows| rows.len() as u16 + 3);
    let [header, stream, ask, input, keys] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(ask_height),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .areas(area);
    draw_header(frame, app, header);
    if app.transcript.blocks.is_empty() {
        draw_welcome(frame, app, stream);
    } else {
        draw_stream(frame, app, stream);
    }
    if let Some(rows) = ask_rows {
        draw_ask(frame, app, ask, rows);
    }
    draw_input(frame, app, input, &rows, cursor);
    draw_keys(frame, app, keys);
    if app.picker.is_some() {
        draw_picker(frame, app, stream);
    }
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let working = app.working() || app.ask.is_some();
    let spark_style = if working {
        theme::glow()
    } else {
        Style::default().fg(theme::EMBER)
    };
    let left = vec![
        Span::styled(
            format!(" {} ", glimm::spark(app.working(), app.frame)),
            spark_style,
        ),
        Span::styled("Glut", theme::strong()),
        Span::raw("  "),
        Span::raw(format!(
            "{} · {}",
            app.engine.provider,
            app.engine.model_name()
        )),
    ];
    let mut left = left;
    if let Some(effort) = app.engine.endpoint.effort {
        left.push(Span::styled(format!(" · {effort}"), theme::quiet()));
    }
    left.push(Span::styled(format!("  {}", app.folder), theme::quiet()));
    let mut meter = format!(
        "{} in · {} out",
        tokens(app.usage.input_tokens),
        tokens(app.usage.output_tokens)
    );
    if let Some((window, _)) = app.engine.window {
        meter.push_str(&format!(
            " · {}%",
            app.context_used * 100 / u64::from(window.max(1))
        ));
    }
    if let Some(cost) = app.cost {
        meter.push_str(&format!(" · ${cost:.4}"));
    }
    meter.push(' ');
    let used: usize = left.iter().map(|span| span.content.width()).sum();
    let gap = usize::from(area.width).saturating_sub(used + meter.width());
    let mut spans = left;
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(meter, theme::quiet()));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// A token count as people read it: 840, 12.3k, 1.2M.
fn tokens(count: u64) -> String {
    match count {
        0..1_000 => count.to_string(),
        1_000..1_000_000 => format!("{:.1}k", count as f64 / 1_000.0),
        _ => format!("{:.1}M", count as f64 / 1_000_000.0),
    }
}

fn draw_welcome(frame: &mut Frame, app: &mut App, area: Rect) {
    app.rows.clear();
    let mut lines: Vec<Line> = Vec::new();
    for row in glimm::rows(false, app.frame) {
        lines.push(Line::from(Span::styled(row, theme::glow())));
    }
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("Glut", theme::strong())));
    lines.push(Line::from(Span::styled(
        format!("{} · {}", app.engine.provider, app.engine.model_name()),
        theme::quiet(),
    )));
    lines.push(Line::from(Span::styled(app.folder.clone(), theme::quiet())));
    let height = lines.len() as u16;
    let top = area.y + area.height.saturating_sub(height) / 2;
    let place = Rect {
        y: top,
        height: height.min(area.height),
        ..area
    };
    frame.render_widget(Paragraph::new(lines).centered(), place);
}

fn draw_stream(frame: &mut Frame, app: &mut App, area: Rect) {
    let width = area.width.saturating_sub(1).max(8);
    let height = usize::from(area.height);
    // Only what can be on the screen is laid out: blocks from the end until the screen and the scroll are covered.
    let wanted = height + app.scroll;
    let mut taken: Vec<(usize, Line<'static>)> = Vec::new();
    let mut total = 0;
    for at in (0..app.transcript.blocks.len()).rev() {
        let lines = app.transcript.blocks[at].lines(width);
        total += lines.len();
        if taken.len() < wanted {
            for line in lines.iter().rev() {
                taken.push((at, line.clone()));
            }
        }
    }
    taken.reverse();
    app.stream_lines = total;
    app.stream_height = height;
    // A stream that shrank (tool lines folded) cannot stay scrolled further than it is long.
    app.scroll = app.scroll.min(total.saturating_sub(height));
    let end = taken.len().saturating_sub(app.scroll.min(taken.len()));
    let start = end.saturating_sub(height);
    let shown = &taken[start..end];
    app.rows = shown
        .iter()
        .enumerate()
        .map(|(row, (block, _))| (area.y + row as u16, *block))
        .collect();
    // Short streams sit at the bottom, next to the input, like a terminal's.
    let pad = height.saturating_sub(shown.len());
    let mut lines: Vec<Line> = vec![Line::default(); pad];
    lines.extend(shown.iter().map(|(_, line)| line.clone()));
    for row in &mut app.rows {
        row.0 += pad as u16;
    }
    frame.render_widget(Paragraph::new(lines), Rect { width, ..area });
    if app.scroll > 0 {
        let mark = format!(" ↓ {} more ", app.scroll);
        let place = Rect {
            x: area.x + area.width.saturating_sub(mark.width() as u16 + 1),
            y: area.y + area.height.saturating_sub(1),
            width: mark.width() as u16,
            height: 1,
        };
        frame.render_widget(Paragraph::new(Span::styled(mark, theme::glow())), place);
    }
}

fn draw_ask(frame: &mut Frame, app: &App, area: Rect, rows: Vec<String>) {
    let Some(open) = &app.ask else { return };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::glow())
        .title(Span::styled(
            format!(
                " Glut asks: {} ",
                crate::transcript::clean(&open.request.tool)
            ),
            theme::glow(),
        ));
    let mut lines: Vec<Line> = rows
        .into_iter()
        .map(|row| Line::from(Span::raw(row)))
        .collect();
    lines.push(Line::from(vec![
        Span::styled("y", theme::glow().add_modifier(Modifier::BOLD)),
        Span::raw(" allow   "),
        Span::styled("a", theme::glow().add_modifier(Modifier::BOLD)),
        Span::raw(" allow for this session   "),
        Span::styled("n", theme::glow().add_modifier(Modifier::BOLD)),
        Span::raw(" refuse"),
    ]));
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// What an ask shows of its target in `most` rows: all of it, or its head, how much is left out, and its tail — the
/// end of a long command is where a harmless start hides what it really does.
fn asked_rows(target: &str, width: usize, most: usize) -> Vec<String> {
    let rows = wrap(target, width);
    if rows.len() <= most {
        return rows;
    }
    let tail = 2;
    let head = most.saturating_sub(tail + 1).max(1);
    let mut shown: Vec<String> = rows[..head].to_vec();
    shown.push(format!("… {} more rows …", rows.len() - head - tail));
    shown.extend_from_slice(&rows[rows.len() - tail..]);
    shown
}

fn draw_input(frame: &mut Frame, app: &App, area: Rect, rows: &[String], cursor: (usize, usize)) {
    let [rule, text] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(usize::from(area.width)),
            theme::quiet(),
        )),
        rule,
    );
    let visible = usize::from(text.height);
    let first = cursor.0.saturating_sub(visible.saturating_sub(1));
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
        .map(|(at, row)| {
            let mark = if at == 0 { "› " } else { "  " };
            Line::from(vec![
                Span::styled(mark, theme::glow()),
                Span::raw(row.clone()),
            ])
        })
        .collect();
    frame.render_widget(Paragraph::new(lines), text);
    if app.ask.is_none() && app.picker.is_none() {
        frame.set_cursor_position(Position {
            x: text.x + 2 + cursor.1 as u16,
            y: text.y + (cursor.0 - first) as u16,
        });
    }
}

fn draw_keys(frame: &mut Frame, app: &App, area: Rect) {
    let mode = app.mode();
    // The mode is where the owner looks before sending: auto glows, it does the most without asking.
    let mode_style = match mode {
        glut::permissions::Mode::Auto => theme::glow().add_modifier(Modifier::BOLD),
        glut::permissions::Mode::Plan => Style::default().fg(theme::OWNER),
        _ => theme::quiet(),
    };
    let hints = if app.ask.is_some() {
        "y allow · a always · n refuse · Ctrl+C refuse and clear"
    } else if app.picker.is_some() {
        "type to filter · ↑↓ choose · Enter take · Esc close"
    } else if app.working() {
        "Glut works · Esc stops · PgUp/PgDn scroll · Ctrl+O tool results"
    } else {
        "Enter send · Shift+Enter new line · Shift+Tab mode · /help · Ctrl+O tool results · Ctrl+C quit"
    };
    let line = Line::from(vec![
        Span::styled(format!(" {} ", session::mode_label(mode)), mode_style),
        Span::styled(format!(" {hints}"), theme::quiet()),
    ]);
    frame.render_widget(Paragraph::new(line), area);
}

/// The picker over the stream: its title, what was typed, and the entries that match, the chosen one marked.
fn draw_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Some(picker) = &app.picker else { return };
    let width = area.width.saturating_sub(4).min(100);
    let height = area.height.saturating_sub(2).min(20);
    let place = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + area.height.saturating_sub(height),
        width,
        height,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::glow())
        .title(Span::styled(format!(" {} ", picker.title), theme::glow()));
    let shown = picker.shown();
    let rows = usize::from(height.saturating_sub(3));
    let first = picker.selected.saturating_sub(rows.saturating_sub(1));
    let mut lines = vec![Line::from(vec![
        Span::styled("› ", theme::glow()),
        Span::raw(picker.query.clone()),
    ])];
    for (at, item) in shown.iter().enumerate().skip(first).take(rows) {
        let chosen = at == picker.selected;
        let mark = if chosen { "▸ " } else { "  " };
        let style = if chosen {
            theme::glow()
        } else {
            Style::default()
        };
        lines.push(Line::from(vec![
            Span::styled(mark, theme::glow()),
            Span::styled(item.label.clone(), style),
            Span::styled(format!("  {}", item.detail), theme::quiet()),
        ]));
    }
    if shown.is_empty() {
        lines.push(Line::from(Span::styled(
            "  nothing matches",
            theme::quiet(),
        )));
    }
    frame.render_widget(Clear, place);
    frame.render_widget(Paragraph::new(lines).block(block), place);
}

#[cfg(test)]
mod tests {
    use super::{asked_rows, tokens};

    #[test]
    fn a_long_ask_shows_its_head_what_is_left_out_and_its_tail() {
        let target: String = (1..=20)
            .map(|n| format!("line{n}"))
            .collect::<Vec<_>>()
            .join("\n");
        let rows = asked_rows(&target, 40, 6);
        assert_eq!(
            rows,
            [
                "line1",
                "line2",
                "line3",
                "… 15 more rows …",
                "line19",
                "line20"
            ]
        );
        assert_eq!(asked_rows("ls -la", 40, 6), ["ls -la"]);
    }

    #[test]
    fn token_counts_read_short() {
        assert_eq!(tokens(840), "840");
        assert_eq!(tokens(12_345), "12.3k");
        assert_eq!(tokens(1_250_000), "1.2M");
    }
}
