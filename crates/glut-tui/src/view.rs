//! Draws a frame: the header, the stream (or Glimm's welcome), an open question, the input and a line of keys.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Position, Rect};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph};
use unicode_width::UnicodeWidthStr;

use crate::app::App;
use crate::transcript::wrap;
use crate::{glimm, select, session, theme};

/// The most rows the input grows to before it scrolls.
const INPUT_ROWS: usize = 8;
/// The most rows of its target an ask shows.
const ASK_ROWS: usize = 12;
/// The side panel takes this part of the screen (a third, the owner's choice), never less than its narrowest width;
/// and the screen it needs beside the stream.
const PANEL_SHARE: u16 = 3;
const PANEL_MIN_WIDTH: u16 = 32;
pub const PANEL_MIN_SCREEN: u16 = 100;

/// The side panel's width on a screen `width` columns wide: a third of it, never narrower than its minimum.
fn panel_width(width: u16) -> u16 {
    (width / PANEL_SHARE).max(PANEL_MIN_WIDTH)
}

/// What a session's drawing has around it: the whole screen (one pane), or a pane among others (T36a).
#[derive(Debug, Clone, Default)]
pub struct Chrome {
    /// The pane's number, shown first in its footer; `None` alone on the screen.
    pub number: Option<u8>,
    /// The pane the keys go to: its footer glows, the cursor is its.
    pub active: bool,
    /// The other panes and background agents waiting for the owner's word, each said with how to reach it ("pane 2
    /// asks — ⌃P", "reviewer asks — /show reviewer"): the active pane's footer says so.
    pub others_ask: Vec<String>,
}

impl Chrome {
    fn whole() -> Self {
        Chrome {
            number: None,
            active: true,
            others_ask: Vec::new(),
        }
    }

    fn alone(&self) -> bool {
        self.number.is_none()
    }
}

/// One session alone on the whole screen — what the tests draw.
#[cfg(test)]
pub fn draw(frame: &mut Frame, app: &mut App) {
    draw_in(frame, app, frame.area(), &Chrome::whole());
}

/// The screen's panes (T36a): one is drawn as ever; several share the header, stand side by side or one above the
/// other, each with its own footer, and the active one's side panel takes the right where there is room.
pub fn draw_panes(frame: &mut Frame, panes: &mut crate::panes::Panes) {
    // Who waits for the owner's word elsewhere, and how to get there (T36a, T36e).
    let asking: Vec<(u64, String)> = panes
        .list
        .iter()
        .filter(|pane| {
            // A message held in a background agent waits for an Enter no screen shows (T36e).
            pane.app.waits_for_owner() || (pane.background && !pane.app.held.is_empty())
        })
        .map(|pane| {
            let said = match (&pane.name, pane.background) {
                (Some(name), true) if !pane.app.waits_for_owner() => {
                    format!("{name} holds a message — /show {name}")
                }
                (Some(name), true) => format!("{name} asks — /show {name}"),
                _ => format!("pane {} asks — ⌃P", pane.number),
            };
            (pane.id, said)
        })
        .collect();
    let others_ask = |key: u64| -> Vec<String> {
        asking
            .iter()
            .filter(|(other, _)| *other != key)
            .map(|(_, said)| said.clone())
            .collect()
    };
    if !panes.split() {
        let active = panes.active;
        let area = frame.area();
        let pane = &mut panes.list[active];
        pane.area = area;
        let chrome = Chrome {
            others_ask: others_ask(pane.id),
            ..Chrome::whole()
        };
        draw_in(frame, &mut pane.app, area, &chrome);
        return draw_peek(frame, panes, area);
    }
    let [header, body] =
        Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(frame.area());
    let active = panes.active;
    draw_header(frame, &panes.list[active].app, header);
    let panel = panel_width(body.width);
    // The panel only where every pane keeps its minimum beside it.
    let beside = panes.needs(SPLIT_PANE_MIN);
    let room = body.width >= beside + panel;
    let shows_panel = room && panes.list[active].app.panel == Some(true);
    let body = if shows_panel {
        let [body, side] =
            Layout::horizontal([Constraint::Min(1), Constraint::Length(panel)]).areas(body);
        let app = &mut panes.list[active].app;
        let cwd = app.parts.config.cwd.clone();
        app.project_todos.refresh(&cwd);
        draw_panel(frame, app, side);
        body
    } else {
        body
    };
    let (areas, lines) = panes.areas(body);
    // The active pane last: its cursor is the one the terminal shows. Background agents stand nowhere.
    let mut order: Vec<usize> = (0..panes.list.len())
        .filter(|at| *at != active && !panes.list[*at].background)
        .collect();
    order.push(active);
    for at in order {
        let pane = &mut panes.list[at];
        pane.area = areas[at];
        let chrome = Chrome {
            number: Some(pane.number),
            active: at == active,
            others_ask: if at == active {
                others_ask(pane.id)
            } else {
                Vec::new()
            },
        };
        draw_in(frame, &mut pane.app, areas[at], &chrome);
        pane.app.panel_drawn = at == active && shows_panel;
        pane.app.panel_fits = room;
    }
    // A quiet line parts panes side by side.
    for line in lines {
        for y in line.y..line.y + line.height {
            frame.render_widget(
                Paragraph::new(Span::styled("│", theme::quiet())),
                Rect::new(line.x, y, 1, 1),
            );
        }
    }
    let area = panes.list[active].area;
    draw_peek(frame, panes, area);
}

/// `/peek` (T36e): a background agent's stream over the active pane's area, only to read — its name and how to close
/// it in the frame.
fn draw_peek(frame: &mut Frame, panes: &mut crate::panes::Panes, area: Rect) {
    let Some(id) = panes.peek else { return };
    let Some(pane) = panes.list.iter_mut().find(|pane| pane.id == id) else {
        panes.peek = None;
        return;
    };
    if area.width < 20 || area.height < 6 {
        // Too small to read: no peek swallowing the keys unseen.
        panes.peek = None;
        return;
    }
    let name = pane
        .name
        .clone()
        .unwrap_or_else(|| format!("pane {}", pane.number));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::quiet())
        .title(Span::styled(
            format!(" {name} — in the background · ⎋ closes · /show {name} brings it forward "),
            theme::quiet(),
        ));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    let chrome = Chrome {
        number: None,
        active: false,
        others_ask: Vec::new(),
    };
    draw_in(frame, &mut pane.app, inner, &chrome);
}

/// The narrowest a pane beside another is let be before the side panel stays away.
const SPLIT_PANE_MIN: u16 = 60;

/// One session in `area`: alone, the whole screen with its header and side panel; among panes, without either.
fn draw_in(frame: &mut Frame, app: &mut App, area: Rect, chrome: &Chrome) {
    let input_width = usize::from(area.width.saturating_sub(2));
    let (rows, cursor) = app.input.layout(input_width);
    let input_height = rows.len().clamp(1, INPUT_ROWS) as u16 + 1;
    // The plan's ask is its dialog over the stream (T19), not a card below it.
    let plan_asking = app.plan_asks();
    let ask_rows = app.ask.as_ref().filter(|_| !plan_asking).map(|open| {
        let most = usize::from(area.height / 3).clamp(3, ASK_ROWS);
        asked_rows(
            &open.request.target,
            usize::from(area.width.saturating_sub(4 + ask_glimm_width(area.width))),
            most,
        )
    });
    // The target, then a row per choice: once, each scope, refuse — at least as tall as the waiting Glimm beside it.
    let ask_height = ask_rows
        .as_ref()
        .zip(app.ask.as_ref().filter(|_| !plan_asking))
        .map_or(0, |(rows, open)| {
            let inner = (rows.len() + 2 + open.request.scopes.len().min(9)) as u16;
            let glimm = if ask_glimm_width(area.width) > 0 {
                glimm::cells(glimm::Size::Small).1
            } else {
                0
            };
            inner.max(glimm) + 2
        });
    let waiting = waiting_lines(app, usize::from(area.width));
    let [header, stream, ask, queue, input, keys] = Layout::vertical([
        Constraint::Length(if chrome.alone() { 1 } else { 0 }),
        Constraint::Min(1),
        Constraint::Length(ask_height),
        Constraint::Length(waiting.len() as u16),
        Constraint::Length(input_height),
        Constraint::Length(1),
    ])
    .areas(area);
    if !waiting.is_empty() {
        frame.render_widget(Paragraph::new(waiting), queue);
    }
    if chrome.alone() {
        draw_header(frame, app, header);
    }
    // The side panel (T28) takes the right of the stream where the screen is wide enough for both; among panes it is
    // the screen's, drawn beside them.
    app.stream_width = stream.x + stream.width;
    if chrome.alone() {
        app.panel_fits = stream.width >= PANEL_MIN_SCREEN;
    }
    let stream = if chrome.alone() && app.panel == Some(true) && app.panel_fits {
        // The to-dos are looked at once per frame the panel shows: their count heads the tab, the model may change
        // them.
        let cwd = app.parts.config.cwd.clone();
        app.project_todos.refresh(&cwd);
        let [stream, panel] = Layout::horizontal([
            Constraint::Min(40),
            Constraint::Length(panel_width(stream.width)),
        ])
        .areas(stream);
        draw_panel(frame, app, panel);
        app.stream_width = stream.x + stream.width;
        app.panel_drawn = true;
        stream
    } else {
        app.panel_drawn = false;
        stream
    };
    // Before the first message there are only notes (the guides read, a provider's word): Glimm welcomes above them.
    if app.welcoming() {
        let width = stream.width.saturating_sub(1).max(8);
        let thinking = app.transcript.thinking;
        let notes: usize = app
            .transcript
            .blocks
            .iter_mut()
            .map(|block| block.lines(width, thinking, 0).len())
            .sum();
        let notes = (notes as u16).min(stream.height / 2);
        let [welcome, below] =
            Layout::vertical([Constraint::Min(1), Constraint::Length(notes)]).areas(stream);
        draw_welcome(frame, app, welcome);
        if notes > 0 {
            draw_stream(frame, app, below);
        }
    } else {
        draw_stream(frame, app, stream);
    }
    if let Some(rows) = ask_rows {
        draw_ask(frame, app, ask, rows);
    }
    draw_input(frame, app, input, &rows, cursor, chrome.active);
    let cwd = app.parts.config.cwd.clone();
    // One row: a line break in a hostile HEAD would part the width counted from the width drawn.
    let branch = app
        .branch
        .name(&cwd)
        .map(|name| crate::transcript::clean(name).replace('\n', " "));
    draw_keys(frame, app, keys, branch, chrome);
    if app.picker.is_some() {
        draw_picker(frame, app, stream);
    } else if app.keys_sheet.is_some() {
        draw_keys_sheet(frame, app, stream);
    } else if app.review.open {
        draw_review(frame, app, stream);
    } else if app.plan_view.is_some() {
        draw_plan(frame, app, stream);
    } else if app.agent_view.is_some() {
        draw_agent(frame, app, stream);
    } else if app.picture_view.is_some() {
        draw_picture(frame, app, stream);
    } else {
        draw_suggestions(frame, app, stream);
    }
}

/// The review (T17) over the stream: framed in the glow, it waits for the owner's decisions.
fn draw_review(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 20 || area.height < 8 {
        return;
    }
    let (done, all) = app.review.progress();
    let scope = match app.review.scope {
        Some(crate::review::Scope::Session) => "this session",
        _ => "the last turn",
    };
    let since = crate::transcript::cut(&crate::transcript::clean(&app.review.since), 40);
    let title = crate::transcript::cut(
        &format!(" Review · {scope}, since before «{since}» · {done} of {all} files reviewed "),
        usize::from(area.width).saturating_sub(4),
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::glow())
        .title(Span::styled(title, theme::glow()));
    let inner = block.inner(area);
    let lines = app
        .review
        .lines(usize::from(inner.width), usize::from(inner.height));
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// The plan (T19) over the stream: while Glut asks, framed in the glow with the choices below it; else only to read.
fn draw_plan(frame: &mut Frame, app: &mut App, area: Rect) {
    // Drawn on any screen: while it asks, it is the only place the question shows.
    if area.width < 4 || area.height < 3 {
        return;
    }
    let asking = app.plan_view.as_ref().is_some_and(|view| view.asking);
    let style = if asking {
        theme::glow()
    } else {
        theme::quiet()
    };
    let title = if asking {
        " Glut's plan — approve it, or send it back "
    } else {
        " Glut's plan "
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(Span::styled(title, style));
    let inner = block.inner(area);
    app.overlay_rows = usize::from(inner.height);
    let modes = app.plan_modes();
    let auto_unsandboxed = !session::sandboxed(&app.parts);
    // `b` hands the plan over only where there is no question to which pane.
    let others = (app.others.len() == 1).then(|| app.others[0]);
    let (Some(plan), Some(view)) = (&app.plan, &mut app.plan_view) else {
        return;
    };
    view.elsewhere = others;
    let lines = crate::plan::lines(
        plan,
        &app.todos,
        view,
        &modes,
        auto_unsandboxed,
        usize::from(inner.width),
        usize::from(inner.height),
    );
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// An image shown whole over the stream (T46): as large as the stream holds it, its proportions kept, its line in
/// the frame.
fn draw_picture(frame: &mut Frame, app: &mut App, area: Rect) {
    let Some(image) = app.picture_view.clone() else {
        return;
    };
    if area.width < 12 || area.height < 6 {
        return;
    }
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::glow())
        .title(Span::styled(
            format!(" {} · ⎋ closes · o opens it ", image.label()),
            theme::glow(),
        ));
    let inner = block.inner(area);
    frame.render_widget(Clear, area);
    frame.render_widget(block, area);
    if crate::pictures::drawing() == crate::pictures::Drawing::Off {
        return;
    }
    let (columns, rows) =
        crate::pictures::fitted(image.width, image.height, inner.width, inner.height);
    let place = Rect {
        x: inner.x + (inner.width - columns) / 2,
        y: inner.y + (inner.height - rows) / 2,
        width: columns,
        height: rows,
    };
    match crate::pictures::lines(&image, columns, rows) {
        Some(lines) => frame.render_widget(Paragraph::new(lines), place),
        None => frame.render_widget(
            Paragraph::new(Span::styled(
                format!("image #{} cannot be drawn: it is gone", image.number),
                theme::fault(),
            )),
            inner,
        ),
    }
}

/// A subagent's own stream (T18) over the session's: the frame glows while it works.
fn draw_agent(frame: &mut Frame, app: &mut App, area: Rect) {
    if area.width < 20 || area.height < 6 {
        return;
    }
    let thinking = app.transcript.thinking;
    let tick = app.frame;
    let Some(mut view) = app.agent_view.take() else {
        return;
    };
    let depth = view.path.len();
    let Some(agent) = app.transcript.agent(&view.path) else {
        // Its stream is gone (a rewind, a new session): nothing to show.
        return;
    };
    let style = if agent.state == crate::transcript::ToolState::Running {
        theme::glow()
    } else {
        theme::quiet()
    };
    let title = crate::agents::title(agent, depth, usize::from(area.width).saturating_sub(4));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(style)
        .title(Span::styled(title, style));
    let inner = block.inner(area);
    let lines = crate::agents::lines(
        agent,
        &mut view,
        inner.width.saturating_sub(1).max(8),
        usize::from(inner.height),
        thinking,
        tick,
    );
    app.overlay_rows = usize::from(inner.height);
    app.agent_view = Some(view);
    frame.render_widget(Clear, area);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

fn draw_header(frame: &mut Frame, app: &App, area: Rect) {
    let working = app.working() || app.ask.is_some();
    let spark_style = if working {
        theme::glow()
    } else {
        Style::default().fg(theme::c().ember)
    };
    // The status lives in the footer (T39): the header names Glut and the folder.
    let mut spans = vec![
        Span::styled(
            format!(" {} ", glimm::spark(app.working(), app.frame)),
            spark_style,
        ),
        Span::styled("Glut", theme::strong()),
    ];
    let room = usize::from(area.width).saturating_sub(left_width(&spans) + 2);
    if room > 4 {
        spans.push(Span::styled(
            crate::transcript::cut(&format!("  {}", app.folder), room),
            theme::quiet(),
        ));
    }
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

/// The most waiting messages shown above the input; the rest are counted.
const WAITING_ROWS: usize = 4;

/// What waits above the input (T10): messages sent to steer the turn, not taken in yet, and the ones queued for
/// after it — one line each, cut to the width.
fn waiting_lines(app: &App, width: usize) -> Vec<Line<'static>> {
    let steering = app
        .steering
        .as_ref()
        .map(glut::Inbox::waiting)
        .unwrap_or_default();
    let mut all: Vec<(bool, String)> = steering.into_iter().map(|text| (true, text)).collect();
    all.extend(app.queue.iter().map(|text| (false, text.clone())));
    // The mark, the gap and the longest label beside the text.
    let room = width.saturating_sub(26).max(8);
    let working = app.working();
    let mut lines: Vec<Line<'static>> = all
        .iter()
        .take(WAITING_ROWS)
        .map(|(steer, text)| {
            let first = crate::transcript::clean(text.lines().next().unwrap_or(""));
            let (mark, word) = match (steer, working) {
                (true, _) => (Span::styled("↪ ", theme::glow()), "before its next step"),
                (false, true) => (Span::styled("⏭ ", theme::quiet()), "after this turn"),
                (false, false) => (Span::styled("⏭ ", theme::quiet()), "⏎ sends it"),
            };
            Line::from(vec![
                mark,
                Span::raw(crate::transcript::cut(&first, room)),
                Span::styled(format!("  {word}"), theme::quiet()),
            ])
        })
        .collect();
    if all.len() > WAITING_ROWS {
        lines.push(Line::from(Span::styled(
            format!("  … {} more waiting", all.len() - WAITING_ROWS),
            theme::quiet(),
        )));
    }
    lines
}

/// How many cells `spans` take.
fn left_width(spans: &[Span]) -> usize {
    spans.iter().map(|span| span.content.width()).sum()
}

fn draw_welcome(frame: &mut Frame, app: &mut App, area: Rect) {
    app.rows.clear();
    let mut lines: Vec<Line> = glimm::lines(glimm::Size::Large, glimm::Mood::Resting, app.frame);
    lines.push(Line::default());
    lines.push(Line::from(Span::styled("Glut", theme::strong())));
    lines.push(Line::from(Span::styled(
        format!("{} · {}", app.engine.provider, app.engine.model_name()),
        theme::quiet(),
    )));
    lines.push(Line::from(Span::styled(app.folder.clone(), theme::quiet())));
    // The very first start (TUI9c): the keys to begin with, and `/init` where the folder has no guide yet.
    if app.first_start {
        lines.push(Line::default());
        let mut first = vec![
            "⌃K all the keys · / all the commands".to_owned(),
            "⇧⇥ plan first, or build".to_owned(),
        ];
        if !app.parts.config.cwd.join("AGENTS.md").exists() {
            first
                .push("/init writes AGENTS.md: what Glut should know about this folder".to_owned());
        }
        for line in first {
            lines.push(Line::from(Span::styled(
                crate::motion::spoken(&line).into_owned(),
                theme::glow(),
            )));
        }
    }
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
    // A selection belongs to the width it was made at: laid out anew, its lines are other lines.
    if app
        .selection
        .is_some_and(|selection| selection.width != width)
    {
        app.selection = None;
    }
    let texts = app.search.is_some();
    let chosen = app.reading.map(|reading| reading.block);
    // The reading mode's block is scrolled into view first: whole where it fits, else its head at the top. Only the
    // blocks' lengths are needed for that (each laid out once and kept), not their lines.
    if let Some(reading) = &mut app.reading
        && std::mem::take(&mut reading.jump)
        && reading.block < app.transcript.blocks.len()
    {
        let after: usize = app.transcript.blocks[reading.block + 1..]
            .iter_mut()
            .map(|block| block.lines(width, app.transcript.thinking, app.frame).len())
            .sum();
        let length = app.transcript.blocks[reading.block]
            .lines(width, app.transcript.thinking, app.frame)
            .len();
        let top = after + length;
        if top > app.scroll + height || length > height {
            app.scroll = top.saturating_sub(height);
        } else if after < app.scroll {
            app.scroll = after;
        }
    }
    // Each block's lines are made once and kept: their count is cheap, their text kept beside them (T47).
    let thinking = app.transcript.thinking;
    let total: usize = app
        .transcript
        .blocks
        .iter_mut()
        .map(|block| block.lines(width, thinking, app.frame).len())
        .sum();
    // The search finds its matches when its query or the stream changed — not at every frame — and scrolls to the
    // one shown before the screen's lines are taken.
    if texts {
        app.search_texts = true;
        let layouts = crate::transcript::layouts();
        let changed = app.search.as_ref().is_some_and(|search| {
            search.fresh || search.searched.as_ref() != Some(&(search.query.clone(), layouts))
        });
        if changed {
            let mut all: Vec<String> = Vec::with_capacity(total);
            for block in &mut app.transcript.blocks {
                all.extend_from_slice(block.texts(width, thinking, app.frame));
            }
            if let Some(search) = &mut app.search {
                search.matches = select::matches(&search.query, &all);
                search.searched = Some((search.query.clone(), layouts));
            }
        }
    }
    // A search closed gives its texts up: kept, they would double what a long stream holds.
    if !texts && std::mem::take(&mut app.search_texts) {
        app.transcript
            .blocks
            .iter_mut()
            .for_each(crate::transcript::Block::forget_texts);
    }
    if let Some(search) = &mut app.search {
        if search.fresh {
            search.fresh = false;
            search.current = search.matches.len().checked_sub(1);
            search.jump = true;
        }
        search.current = search.current.filter(|at| *at < search.matches.len());
        // The match shown is scrolled to the middle of the stream.
        if std::mem::take(&mut search.jump)
            && let Some((line, _, _)) = search.current.map(|at| search.matches[at])
        {
            app.scroll = total.saturating_sub(line + 1 + height / 2);
        }
    }
    // Only what can be on the screen is taken: lines from the end until the screen and the scroll are covered.
    let wanted = height + app.scroll.min(total.saturating_sub(height));
    let mut taken: Vec<(usize, Line<'static>)> = Vec::new();
    for at in (0..app.transcript.blocks.len()).rev() {
        if taken.len() >= wanted {
            break;
        }
        let lines = app.transcript.blocks[at].lines(width, thinking, app.frame);
        for line in lines.iter().rev() {
            taken.push((at, line.clone()));
        }
    }
    taken.reverse();
    app.stream_lines = total;
    app.stream_height = height;
    app.stream_x = area.x;
    app.stream_layout = width;
    // A stream that shrank (tool lines folded) cannot stay scrolled further than it is long.
    app.scroll = app.scroll.min(total.saturating_sub(height));
    let end = taken.len().saturating_sub(app.scroll.min(taken.len()));
    let start = end.saturating_sub(height);
    let shown = &taken[start..end];
    // The stream's line of the first one taken: the taken lines are the stream's last.
    let first_line = total - taken.len() + start;
    // Short streams sit at the bottom, next to the input, like a terminal's.
    let pad = height.saturating_sub(shown.len());
    app.rows = shown
        .iter()
        .enumerate()
        .map(|(row, (block, _))| (area.y + (pad + row) as u16, *block, first_line + row))
        .collect();
    let mut lines: Vec<Line> = vec![Line::default(); pad];
    lines.extend(shown.iter().map(|(_, line)| line.clone()));
    let place = Rect { width, ..area };
    frame.render_widget(Paragraph::new(lines), place);
    mark_stream(frame, app, place);
    // The reading mode's block: its rows faintly underlaid across the stream, so it is seen where its text is however
    // wide the window — what has a background of its own (code, a match, a selection) keeps it —, and a bar in the
    // column the stream leaves free at its right.
    if let Some(chosen) = chosen {
        let palette = theme::c();
        let tint = theme::mix(palette.ink, palette.quiet, 0.25);
        for (row, block, _) in &app.rows {
            if *block == chosen {
                for x in area.x..area.x + width {
                    if let Some(cell) = frame.buffer_mut().cell_mut(Position { x, y: *row })
                        && cell.bg == ratatui::style::Color::Reset
                    {
                        cell.bg = tint;
                    }
                }
                let bar = Rect {
                    x: area.x + width,
                    y: *row,
                    width: 1,
                    height: 1,
                };
                frame.render_widget(Paragraph::new(Span::styled("▐", theme::glow())), bar);
            }
        }
    }
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

/// The selection and the search's matches over the stream drawn: the selection reversed, the matches on a dim
/// amber, the match shown in the glow.
fn mark_stream(frame: &mut Frame, app: &App, area: Rect) {
    let paint = |frame: &mut Frame, row: u16, from: usize, to: usize, style: Style| {
        let right = area.x + area.width;
        for col in from..to {
            let Ok(col) = u16::try_from(col) else { break };
            let x = area.x.saturating_add(col);
            if x >= right {
                break;
            }
            if let Some(cell) = frame.buffer_mut().cell_mut(Position { x, y: row }) {
                cell.set_style(style);
            }
        }
    };
    for (row, _, line) in &app.rows {
        if let Some(search) = &app.search {
            for (at, (found, from, to)) in search.matches.iter().enumerate() {
                if found == line {
                    let style = if search.current == Some(at) {
                        Style::default().bg(theme::c().glow).fg(theme::c().ink)
                    } else {
                        Style::default().bg(theme::c().found)
                    };
                    paint(frame, *row, *from, *to, style);
                }
            }
        }
        if let Some((from, to)) = app
            .selection
            .and_then(|selection| selection.columns(*line, usize::from(area.width)))
        {
            paint(
                frame,
                *row,
                from,
                to,
                Style::default().add_modifier(Modifier::REVERSED),
            );
        }
    }
}

fn draw_ask(frame: &mut Frame, app: &App, area: Rect, rows: Vec<String>) {
    let Some(open) = &app.ask else { return };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::glow())
        .title(Span::styled(
            match &open.request.from {
                // T40: beside others a subagent's ask says whose it is.
                Some(from) => format!(
                    " ⬡ {} asks: {} ",
                    crate::transcript::cut(&crate::transcript::clean(from), 40),
                    crate::transcript::clean(&open.request.tool)
                ),
                None => format!(
                    " Glut asks: {} ",
                    crate::transcript::clean(&open.request.tool)
                ),
            },
            theme::glow(),
        ));
    let mut lines: Vec<Line> = rows
        .into_iter()
        .map(|row| Line::from(Span::raw(row)))
        .collect();
    let key = |key: String| {
        Span::styled(
            format!(" {key}  "),
            theme::glow().add_modifier(Modifier::BOLD),
        )
    };
    let width = usize::from(area.width.saturating_sub(4));
    lines.push(Line::from(vec![
        key("y".into()),
        Span::raw("allow this once"),
    ]));
    for (at, scope) in open.request.scopes.iter().take(9).enumerate() {
        let scope =
            crate::transcript::cut(&crate::transcript::clean(scope), width.saturating_sub(30));
        lines.push(Line::from(vec![
            key((at + 1).to_string()),
            Span::raw("allow "),
            Span::styled(scope, Style::default().add_modifier(Modifier::BOLD)),
            Span::styled(" for this session", theme::quiet()),
        ]));
    }
    lines.push(Line::from(vec![key("n".into()), Span::raw("refuse")]));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    let beside = ask_glimm_width(area.width);
    if beside == 0 {
        frame.render_widget(Paragraph::new(lines), inner);
        return;
    }
    // Glimm waits beside the question, looking up at the owner.
    let [glimm_area, text] =
        Layout::horizontal([Constraint::Length(beside), Constraint::Min(1)]).areas(inner);
    let glimm_lines = glimm::lines(glimm::Size::Small, glimm::Mood::Waiting, app.frame);
    frame.render_widget(Paragraph::new(glimm_lines), glimm_area);
    frame.render_widget(Paragraph::new(lines), text);
}

/// The narrowest screen the waiting Glimm stands beside a question on; a narrower one keeps every column for the
/// question.
const ASK_GLIMM_MIN_SCREEN: u16 = 60;

/// How many columns the waiting Glimm takes in an ask box on a screen `width` wide: itself and a gap, or none.
fn ask_glimm_width(width: u16) -> u16 {
    // The reader mode has no Glimm: the question takes every column.
    if width >= ASK_GLIMM_MIN_SCREEN && !crate::motion::reader() {
        glimm::cells(glimm::Size::Small).0 + 2
    } else {
        0
    }
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

fn draw_input(
    frame: &mut Frame,
    app: &App,
    area: Rect,
    rows: &[String],
    cursor: (usize, usize),
    active: bool,
) {
    let [rule, text] = Layout::vertical([Constraint::Length(1), Constraint::Min(1)]).areas(area);
    frame.render_widget(
        Paragraph::new(Span::styled(
            "─".repeat(usize::from(area.width)),
            theme::quiet(),
        )),
        rule,
    );
    // The search takes the input's place while it is open; the input waits below it, unchanged.
    if let Some(search) = &app.search {
        let found = match (search.current, search.matches.len()) {
            (_, 0) if search.query.is_empty() => String::new(),
            (_, 0) => "  no match".to_owned(),
            (Some(at), count) => format!("  {} of {count}", at + 1),
            (None, count) => format!("  {count} matches"),
        };
        let query = crate::transcript::clean(&search.query);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("search ", theme::glow()),
                Span::raw(query.clone()),
                Span::styled(found, theme::quiet()),
            ])),
            text,
        );
        if active {
            frame.set_cursor_position(Position {
                x: text.x + 7 + query.width() as u16,
                y: text.y,
            });
        }
        return;
    }
    let visible = usize::from(text.height);
    let first = cursor.0.saturating_sub(visible.saturating_sub(1));
    // A line that runs as the owner's command says so before Enter: `!` for Glut to see, `‼` for the owner alone.
    let command = crate::app::owners_command(&app.input.text());
    let first_mark = match command {
        Some((true, _)) => Span::styled("‼ ", theme::glow().add_modifier(Modifier::BOLD)),
        Some((false, _)) => Span::styled("! ", theme::glow().add_modifier(Modifier::BOLD)),
        None => Span::styled("› ", theme::glow()),
    };
    let lines: Vec<Line> = rows
        .iter()
        .enumerate()
        .skip(first)
        .take(visible)
        .map(|(at, row)| {
            let mark = if at == 0 {
                first_mark.clone()
            } else {
                Span::raw("  ")
            };
            Line::from(vec![mark, Span::raw(row.clone())])
        })
        .collect();
    let mut lines = lines;
    // What a typed command still takes, faint after the cursor: its arguments, or the rest of the word begun.
    if rows.len() == 1
        && app.input.at_end()
        && let Some(ghost) = crate::commands::ghost(&app.input.text())
        && let Some(line) = lines.last_mut()
    {
        line.spans.push(Span::styled(
            ghost,
            theme::quiet().add_modifier(Modifier::DIM),
        ));
    }
    frame.render_widget(Paragraph::new(lines), text);
    if active && app.ask.is_none() && app.picker.is_none() {
        frame.set_cursor_position(Position {
            x: text.x + 2 + cursor.1 as u16,
            y: text.y + (cursor.0 - first) as u16,
        });
    }
}

/// The footer's hints while Glut works, and while it waits for the next message.
const WORKING_HINTS: &str =
    "Glut works · ⏎ steers it · ⇥ queues for after · ⌥↑ takes back · ⎋ stops";
const USUAL_HINTS: &str =
    "⏎ send · ⇧⏎ new line · ⇧⇥ mode · ⌃K keys · @ file · !cmd · ⌃F search · ⌃C quit";

/// The footer (T39): the status as segments — the vi state, the mode, the model, the context, the cost, the branch —
/// and the key hints, the moment's or the usual ones.
fn draw_keys(frame: &mut Frame, app: &App, area: Rect, branch: Option<String>, chrome: &Chrome) {
    use crate::footer::Segment;
    // The reader mode's footer is plain: Powerline's glyphs say nothing to a screen reader (TUI9b).
    let separators = if crate::motion::reader() {
        crate::footer::Separators::Plain
    } else {
        app.separators
    };
    let mode = app.mode();
    let mut segments = Vec::new();
    let asks = app.waits_for_owner();
    // Among panes the number comes first (T36a): the active one glows, one that waits for the owner's word while
    // the keys are elsewhere is the fault colour, so it is seen from the other pane.
    // The pane's role beside its number (T36c): "2 reviewer"; alone, a segment of its own after the mode.
    let role = app.role.as_deref().map(crate::transcript::clean);
    if let Some(number) = chrome.number {
        let label = match &role {
            Some(role) => format!("{number} {role}"),
            None => number.to_string(),
        };
        segments.push(if chrome.active {
            Segment::marked(label, theme::c().glow, 0)
        } else if asks {
            Segment::marked(label, theme::c().fault, 0)
        } else {
            Segment::calm(label, 0)
        });
    }
    // The vi mode's state first, where a vi user looks for it.
    if let Some(vi) = &app.vi {
        segments.push(match vi.mode {
            crate::vi::Mode::Normal => Segment::marked("NORMAL", theme::c().glow, 0),
            crate::vi::Mode::Insert => Segment::calm("INSERT", 0),
        });
    }
    // The mode is where the owner looks before sending: auto glows, it does the most without asking.
    segments.push(match mode {
        glut::permissions::Mode::Auto => {
            Segment::marked(session::mode_label(mode), theme::c().glow, 0)
        }
        glut::permissions::Mode::Plan => {
            Segment::marked(session::mode_label(mode), theme::c().owner, 0)
        }
        _ => Segment::calm(session::mode_label(mode), 0),
    });
    if let (None, Some(role)) = (chrome.number, &role) {
        segments.push(Segment::calm(role.clone(), 0));
    }
    let mut model = format!("{} · {}", app.engine.provider, app.engine.model_name());
    if let Some(effort) = app.engine.endpoint.effort {
        model.push_str(&format!(" · {effort}"));
    }
    segments.push(Segment::calm(
        crate::transcript::clean(&model).replace('\n', " "),
        1,
    ));
    let window = app.meter_window();
    let mut context = crate::footer::context(
        window.used,
        window.window.map(|(size, _)| size),
        window.compacts_at,
    );
    // T36e: whether the provider still holds the cache — only once it has said it caches at all.
    if app.cache_seen {
        let warm = app
            .warm_until
            .is_some_and(|until| until > std::time::Instant::now());
        context.text.push_str(if warm { " ●" } else { " ○" });
    }
    segments.push(context);
    // Held still (TUI9b), "working" counts its seconds where the flame would flicker.
    if crate::motion::still()
        && let Some(since) = app.busy_since.filter(|_| app.working())
    {
        // After a pane's number, before everything else.
        segments.insert(
            usize::from(chrome.number.is_some()),
            Segment::marked(
                format!("working {}s", since.elapsed().as_secs()),
                theme::c().glow,
                0,
            ),
        );
    }
    // TUI10: how fast the model writes — live while its answer streams (guessed from its characters), the last
    // turn's own count once it rests.
    let rate = if app.working() {
        app.meter
            .live_rate()
            .map(|rate| format!("~{rate:.0} tok/s"))
    } else {
        app.meter.last_rate().map(|rate| format!("{rate:.0} tok/s"))
    };
    if let Some(rate) = rate {
        segments.push(Segment::calm(rate, 2));
    }
    if let Some(cost) = app.cost {
        segments.push(Segment::calm(format!("${cost:.4}"), 2));
    }
    if let Some(branch) = branch {
        segments.push(Segment::calm(
            format!("{} {branch}", separators.branch_sign()),
            3,
        ));
    }
    let scopes = app
        .ask
        .as_ref()
        .map_or(0, |open| open.request.scopes.len().min(9));
    // An open question's keys come before any toast.
    let hints = if let Some(toast) = app.toast().filter(|_| app.ask.is_none()) {
        toast.to_owned()
    } else if let Some((alone, _)) = crate::app::owners_command(&app.input.text()) {
        if alone {
            "⏎ runs this in your shell, for you alone (!!)".to_owned()
        } else {
            "⏎ runs this in your shell and shows Glut its output (!)".to_owned()
        }
    } else if app.search.is_some() {
        "⏎ or ↑ the match above · ⇧⏎ or ↓ the one below · ⎋ close".to_owned()
    } else if app.plan_asks() {
        "Glut waits for your word on its plan · ⌃C refuse and clear".to_owned()
    } else if app.ask.is_some() && scopes > 0 {
        let keys = if scopes == 1 {
            "1".to_owned()
        } else {
            format!("1–{scopes}")
        };
        format!("y once · {keys} for this session · n refuse · ⌃C refuse and clear")
    } else if app.ask.is_some() {
        "y once · n refuse · ⌃C refuse and clear".to_owned()
    } else if app.reading.is_some() {
        "↑↓ a block · ⏎ folds · c copies · g G the ends · / search · ⎋ back".to_owned()
    } else if app.plan_view.is_some() || app.keys_sheet.is_some() {
        "↑↓ PgUp PgDn scroll · ⎋ closes".to_owned()
    } else if app.agent_view.is_some() {
        "a subagent's own stream · ↑↓ scroll · a number opens its subagent · ⎋ back".to_owned()
    } else if app.picker.is_some() {
        "type to filter · ↑↓ choose · ⏎ take · ⎋ close".to_owned()
    } else if app.working() {
        WORKING_HINTS.to_owned()
    } else {
        USUAL_HINTS.to_owned()
    };
    // A pane the keys are not in shows only what waits there; the active one says which other pane asks.
    let hints = if !chrome.active {
        if asks {
            "asks — ⌃P answers".to_owned()
        } else {
            String::new()
        }
    } else if !chrome.others_ask.is_empty() {
        format!("{} · {hints}", chrome.others_ask.join(" · "))
    } else {
        hints
    };
    // Any other hint is the moment's: it goes after the segments that may.
    let moment = hints != WORKING_HINTS && hints != USUAL_HINTS;
    let line = crate::footer::line(
        segments,
        &crate::motion::spoken(&hints),
        usize::from(area.width),
        separators,
        moment,
    );
    frame.render_widget(Paragraph::new(line), area);
}

/// The panel width from which the To-dos tab shows its count beside its name.
const TABS_WITH_COUNT: u16 = 46;

/// The side panel: its tabs — the plan the run keeps (done quiet, under way glowing, open plain) and the gate, live
/// (T15) — under Glimm, who heads it with the session's mood.
fn draw_panel(frame: &mut Frame, app: &mut App, area: Rect) {
    use crate::app::Tab;
    use glut::planning::TodoStatus;
    let open = app.project_todos.open();
    // The count stands beside the name only where the panel has room for it.
    let todos_title = if open > 0 && area.width >= TABS_WITH_COUNT {
        format!("To-dos {open}")
    } else {
        "To-dos".to_owned()
    };
    // The panel with the keys (TUI9a) shows it in its edge.
    let edge = if app.panel_focus {
        theme::glow()
    } else {
        theme::quiet()
    };
    let block = Block::default()
        .borders(Borders::LEFT)
        .border_style(edge)
        .title(tab_titles(
            &[
                (Tab::Plan, "Plan"),
                (Tab::Gate, "Gate"),
                (Tab::Meter, "Meter"),
                (Tab::Agents, "Agents"),
                (Tab::Todos, &todos_title),
                (Tab::Keys, "Keys"),
            ],
            app.tab,
            usize::from(area.width.saturating_sub(1)),
        ));
    let width = usize::from(area.width.saturating_sub(4)).max(8);
    // Glimm heads the panel with the session's mood: working, waiting for the owner, pleased or gone out.
    let mut lines: Vec<Line> = vec![Line::default()];
    for mut row in glimm::lines(glimm::Size::Small, app.mood(), app.frame) {
        row.spans.insert(0, Span::raw(" "));
        lines.push(row);
    }
    if app.tab == Tab::Meter {
        lines.push(Line::default());
        // T42: a guide grown too large heads the meter, in orange, as long as it is.
        for warning in app.guide_warnings() {
            for row in wrap(&warning, width) {
                lines.push(Line::from(Span::styled(format!(" {row}"), theme::glow())));
            }
            lines.push(Line::default());
        }
        let room = usize::from(area.height).saturating_sub(lines.len() + 2);
        let body = app
            .meter
            .lines(&app.meter_window(), width + 2, PANEL_BODY_MOST);
        lines.extend(scrolled(body, &mut app.panel_scroll, room));
        lines.push(Line::default());
        lines.extend(panel_keys(app, width));
        frame.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }
    if app.tab == Tab::Agents {
        lines.push(Line::default());
        let keys = panel_keys(app, width);
        // T36e: the agents in the background first — sessions of their own, reached by name.
        if !app.background.is_empty() {
            lines.push(Line::from(Span::styled(
                " In the background",
                theme::strong(),
            )));
            for agent in &app.background {
                for row in wrap(agent, width) {
                    lines.push(Line::from(Span::raw(format!(" {row}"))));
                }
            }
            for row in wrap(
                "/peek <name> reads one · /show <name> brings it forward · /send <name> …",
                width,
            ) {
                lines.push(Line::from(Span::styled(format!(" {row}"), theme::quiet())));
            }
            lines.push(Line::default());
        }
        let list = crate::agents::overview(&app.transcript);
        if list.is_empty() {
            for row in wrap(
                "No subagent yet. When Glut hands a piece of work to one, it shows here: what it does now, its calls \
                 and tokens.",
                width,
            ) {
                lines.push(Line::from(Span::styled(format!(" {row}"), theme::quiet())));
            }
        } else {
            let room = usize::from(area.height).saturating_sub(lines.len() + keys.len() + 3);
            lines.extend(crate::agents::overview_lines(
                &list,
                app.agents_chosen(),
                width + 2,
                room,
            ));
        }
        lines.push(Line::default());
        lines.extend(keys);
        frame.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }
    if app.tab == Tab::Keys {
        lines.push(Line::default());
        let keys = panel_keys(app, width);
        let sheet = crate::commands::sheet(width + 2);
        // The tab names' row and the gap above the keys come off too.
        let room = usize::from(area.height).saturating_sub(lines.len() + keys.len() + 2);
        // Scrolled no further than its end: the wheel past it does not leave rows to scroll back first.
        app.keys_scroll = app.keys_scroll.min(sheet.len().saturating_sub(room));
        lines.extend(sheet.into_iter().skip(app.keys_scroll).take(room));
        lines.push(Line::default());
        lines.extend(keys);
        frame.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }
    if app.tab == Tab::Todos {
        lines.push(Line::default());
        let keys = panel_keys(app, width);
        let todos = &app.project_todos;
        if let Some(fault) = &todos.fault {
            for row in wrap(&format!("{} cannot be read: {fault}", todos.name()), width) {
                lines.push(Line::from(Span::styled(format!(" {row}"), theme::fault())));
            }
        } else if todos.open() + todos.done() == 0 {
            let text = match todos.path() {
                Some(_) => format!(
                    "Nothing to do in {}. /todo <text> (or a here) notes the next idea at its end.",
                    todos.name()
                ),
                None => format!(
                    "No {} yet. /todo <text> (or a here) notes an idea and creates it: the to-dos that last \
                     between sessions, yours and Glut's.",
                    todos.name()
                ),
            };
            for row in wrap(&text, width) {
                lines.push(Line::from(Span::styled(format!(" {row}"), theme::quiet())));
            }
        }
        let room = usize::from(area.height).saturating_sub(lines.len() + keys.len() + 3);
        if todos.fault.is_none() {
            lines.extend(todos.lines(width + 1, room));
        }
        lines.push(Line::default());
        lines.extend(keys);
        frame.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }
    if app.tab == Tab::Gate {
        lines.push(Line::default());
        let (mode, sandbox, granted, rules) = app.guard_facts();
        let guard = crate::watch::Guard {
            mode: &mode,
            sandbox,
            limits: app.parts.config.refusals,
            granted: &granted,
            rules,
        };
        // The last row is the panel's hint.
        let room = usize::from(area.height).saturating_sub(lines.len() + 2);
        let body = app.watch.lines(&guard, width + 2, PANEL_BODY_MOST);
        lines.extend(scrolled(body, &mut app.panel_scroll, room));
        lines.push(Line::default());
        lines.extend(panel_keys(app, width));
        frame.render_widget(Paragraph::new(lines).block(block), area);
        return;
    }
    // The plan and its task list scroll below Glimm, above the hint (TUI9a).
    let header = lines.len();
    let mut body: Vec<Line> = Vec::new();
    if let Some(plan) = &app.plan {
        body.push(Line::default());
        // The plan takes at most half of what is left; the task list and the hint keep the rest.
        let most = usize::from(area.height).saturating_sub(header + 4) / 2;
        body.extend(crate::plan::head(plan, width + 2, most.max(2)));
    }
    if app.todos.is_empty() {
        // With a plan above, no hint for a list that may never come.
        if app.plan.is_none() {
            body.push(Line::default());
            for row in wrap(
                "No plan yet. For work of several steps Glut writes its task list here, and ticks it off as it goes.",
                width,
            ) {
                body.push(Line::from(Span::styled(format!(" {row}"), theme::quiet())));
            }
        }
    } else {
        let done = app
            .todos
            .iter()
            .filter(|item| item.status == TodoStatus::Done)
            .count();
        body.push(Line::default());
        body.push(Line::from(Span::styled(
            format!(" {done} of {} done", app.todos.len()),
            theme::quiet(),
        )));
        body.push(Line::default());
        body.extend(crate::plan::todo_lines(&app.todos, width + 1));
    }
    let keys = panel_keys(app, width);
    let room = usize::from(area.height).saturating_sub(header + keys.len() + 3);
    lines.extend(scrolled(body, &mut app.panel_scroll, room));
    lines.push(Line::default());
    lines.extend(keys);
    frame.render_widget(Paragraph::new(lines).block(block), area);
}

/// How many rows of the Gate or Meter tab are laid out to scroll through: all of them, sliced once (TUI9a) — asked
/// for fewer, a tab that fills its rows in pairs would leave a scroll of one row nothing to move.
const PANEL_BODY_MOST: usize = 2_000;

/// `room` rows of `body` from `scroll` on — scrolled no further than its end, so the keys past it leave nothing to
/// scroll back first.
fn scrolled(body: Vec<Line<'static>>, scroll: &mut usize, room: usize) -> Vec<Line<'static>> {
    *scroll = (*scroll).min(body.len().saturating_sub(room));
    body.into_iter().skip(*scroll).take(room).collect()
}

/// The panel's tab names in `width` cells: each with room around it and a dot between where they fit; else the
/// others close together; else the others by their first letter — the shown one always whole, none cut.
fn tab_titles(
    tabs: &[(crate::app::Tab, &str)],
    shown: crate::app::Tab,
    width: usize,
) -> Line<'static> {
    let fits = |names: &[String], gap: &str| {
        names.iter().map(|name| name.width()).sum::<usize>()
            + gap.width() * names.len().saturating_sub(1)
            <= width
    };
    let named = |short: bool, pad: bool| -> Vec<String> {
        tabs.iter()
            .map(|(tab, name)| {
                if *tab == shown {
                    format!(" {name} ")
                } else if short {
                    name.chars().next().map(String::from).unwrap_or_default()
                } else if pad {
                    format!(" {name} ")
                } else {
                    (*name).to_owned()
                }
            })
            .collect()
    };
    let (names, gap) = [(false, true, "·"), (false, false, " "), (true, false, " ")]
        .into_iter()
        .map(|(short, pad, gap)| (named(short, pad), gap))
        .find(|(names, gap)| fits(names, gap))
        .unwrap_or_else(|| (named(true, false), " "));
    let mut spans = Vec::new();
    for ((tab, _), name) in tabs.iter().zip(names) {
        if !spans.is_empty() {
            spans.push(Span::styled(gap, theme::quiet()));
        }
        spans.push(if *tab == shown {
            Span::styled(name, theme::glow().add_modifier(Modifier::BOLD))
        } else {
            Span::styled(name, theme::quiet())
        });
    }
    Line::from(spans)
}

/// The panel's last rows: the shown tab's key hides it, the others' turn to them; the Agents list's own keys while it
/// has them.
fn panel_keys(app: &App, width: usize) -> Vec<Line<'static>> {
    use crate::app::Tab;
    // With the panel's focus, ←→ go round the tabs (TUI9a).
    let tabs = if app.panel_focus {
        "←→ tabs · "
    } else {
        ""
    };
    let text = if app.tab == Tab::Agents && app.agents_list.is_some() {
        format!("{tabs}↑↓ choose · ⏎ opens · x stops it · ⎋ back to the input · ⌃X A hides")
    } else if app.tab == Tab::Todos && app.project_todos.keys {
        format!(
            "{tabs}↑↓ choose · Space ticks · a adds · o editor · ⎋ back to the input · ⌃Y hides"
        )
    } else if app.panel_focus {
        "←→ tabs · ↑↓ PgUp PgDn scroll · ⎋ back to the input".to_owned()
    } else {
        [
            (Tab::Plan, "⌃B", "plan"),
            (Tab::Gate, "⌃G", "gate"),
            (Tab::Meter, "⌃N", "meter"),
            (Tab::Agents, "⌃X A", "agents"),
            (Tab::Todos, "⌃Y", "to-dos"),
            (Tab::Keys, "⌃K", "keys"),
        ]
        .iter()
        .map(|(tab, key, name)| {
            if *tab == app.tab {
                format!("{key} hides")
            } else {
                format!("{key} {name}")
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
    };
    wrap(&crate::motion::spoken(&text), width)
        .into_iter()
        .map(|row| Line::from(Span::styled(format!(" {row}"), theme::quiet())))
        .collect()
}

/// The commands a typed `/` fits, in a small list just above the input, the chosen one marked; a lone `/` shows
/// them under their topics (T38), the headings never chosen.
fn draw_suggestions(frame: &mut Frame, app: &App, area: Rect) {
    let suggestions = app.suggestions();
    if suggestions.is_empty() || area.height < 3 {
        return;
    }
    let grouped = app.input.text() == "/";
    let chosen = app.suggested.min(suggestions.len() - 1);
    // Every row: a topic's heading (`None`) or a command by its place in the list.
    let mut rows: Vec<Option<usize>> = Vec::new();
    for (at, command) in suggestions.iter().enumerate() {
        if grouped && (at == 0 || suggestions[at - 1].topic != command.topic) {
            rows.push(None);
        }
        rows.push(Some(at));
    }
    let shown = rows.len().min(usize::from(area.height) - 2);
    let height = shown as u16 + 2;
    // As wide as the longest help text needs (mark, name, gap, text and the frame), as far as the screen goes.
    let needed = suggestions
        .iter()
        .map(|command| command.what.width() + 18)
        .max()
        .unwrap_or(40);
    let width = area.width.min(needed as u16);
    let place = Rect {
        x: area.x,
        y: area.y + area.height - height,
        width,
        height,
    };
    let chosen_row = rows
        .iter()
        .position(|row| *row == Some(chosen))
        .unwrap_or(0);
    let first = chosen_row.saturating_sub(shown.saturating_sub(1));
    let lines: Vec<Line> = rows
        .iter()
        .skip(first)
        .take(shown)
        .enumerate()
        .map(|(seen, row)| match row {
            None => {
                // The heading names the topic of the command below it.
                let topic = rows[first + seen + 1..]
                    .iter()
                    .find_map(|row| *row)
                    .map(|at| suggestions[at].topic.label())
                    .unwrap_or_default();
                Line::from(Span::styled(
                    format!("  {topic}"),
                    theme::glow().add_modifier(Modifier::BOLD),
                ))
            }
            Some(at) => {
                let command = suggestions[*at];
                let style = if *at == chosen {
                    theme::glow()
                } else {
                    Style::default()
                };
                Line::from(vec![
                    Span::styled(if *at == chosen { "▸ " } else { "  " }, theme::glow()),
                    Span::styled(format!("/{:<10}", command.name), style),
                    Span::styled(format!(" {}", command.what), theme::quiet()),
                ])
            }
        })
        .collect();
    // Framed as the pickers are (/model): the glow, it waits for the owner's choice.
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::glow())
        .title(Span::styled(
            " Commands · ⇥ completes · ⏎ runs ",
            theme::glow(),
        ));
    frame.render_widget(Clear, place);
    frame.render_widget(Paragraph::new(lines).block(block), place);
}

/// The Keys sheet over the stream (T38), where the window has no room for the panel: only to read, framed quiet.
fn draw_keys_sheet(frame: &mut Frame, app: &mut App, area: Rect) {
    // Too small to show: closed rather than left taking keys unseen.
    if area.width < 20 || area.height < 4 {
        app.keys_sheet = None;
        return;
    }
    let width = area.width.min(80);
    let place = Rect { width, ..area };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::quiet())
        .title(Span::styled(" Keys · ⎋ closes ", theme::quiet()));
    let inner = block.inner(place);
    let rows = usize::from(inner.height);
    app.overlay_rows = rows;
    let sheet = crate::commands::sheet(usize::from(inner.width).saturating_sub(1));
    let most = sheet.len().saturating_sub(rows);
    let scroll = app.keys_sheet.map_or(0, |scroll| scroll.min(most));
    app.keys_sheet = Some(scroll);
    let lines: Vec<Line> = sheet.into_iter().skip(scroll).take(rows).collect();
    frame.render_widget(Clear, place);
    frame.render_widget(Paragraph::new(lines).block(block), place);
}

/// The picker over the stream: its title, what was typed, and the entries that match, the chosen one marked.
fn draw_picker(frame: &mut Frame, app: &App, area: Rect) {
    let Some(picker) = &app.picker else { return };
    let width = area.width.saturating_sub(4).min(100);
    let height = area.height.saturating_sub(4).min(20);
    // Two rows above the input: the stream's last line, often what led to the picker, stays readable.
    let place = Rect {
        x: area.x + (area.width.saturating_sub(width)) / 2,
        y: area.y + area.height.saturating_sub(height + 2),
        width,
        height,
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(theme::glow())
        .title(Span::styled(format!(" {} ", picker.title), theme::glow()));
    let block = match picker.sort {
        Some(sort) => {
            let free = if picker.free_only {
                " · free only"
            } else {
                ""
            };
            block.title_bottom(Span::styled(
                format!(" sorted by {}{free} · ⌃S sort · ⌃F free ", sort.label()),
                theme::quiet(),
            ))
        }
        None => block,
    };
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
        let style = match (chosen, item.dim) {
            (true, _) => theme::glow(),
            (false, true) => theme::quiet().add_modifier(Modifier::DIM),
            (false, false) => Style::default(),
        };
        let detail = if item.dim {
            // Not allowed by the account (OpenRouter's settings), or of an API Glut does not speak yet (a gateway's).
            format!("  {} · not usable here", item.detail)
        } else {
            format!("  {}", item.detail)
        };
        lines.push(Line::from(vec![
            Span::styled(mark, theme::glow()),
            Span::styled(item.label.clone(), style),
            Span::styled(detail, theme::quiet()),
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
    use super::{asked_rows, panel_width, tab_titles};
    use crate::meter::tokens;
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn the_tab_names_close_up_before_they_would_be_cut_and_the_shown_one_stays_whole() {
        use crate::app::Tab;
        let tabs = [
            (Tab::Plan, "Plan"),
            (Tab::Gate, "Gate"),
            (Tab::Meter, "Meter"),
            (Tab::Agents, "Agents"),
            (Tab::Todos, "To-dos"),
            (Tab::Keys, "Keys"),
        ];
        let text = |width| {
            tab_titles(&tabs, Tab::Keys, width)
                .spans
                .iter()
                .map(|span| span.content.to_string())
                .collect::<String>()
        };
        assert_eq!(text(60), " Plan · Gate · Meter · Agents · To-dos · Keys ");
        assert_eq!(text(40), "Plan Gate Meter Agents To-dos  Keys ");
        assert_eq!(text(31), "P G M A T  Keys ");
        for width in [31, 40, 60] {
            assert!(text(width).width() <= width);
        }
    }

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
        assert_eq!(panel_width(90), 32, "never narrower than its minimum");
        assert_eq!(panel_width(210), 70, "a third of the screen");
    }

    #[test]
    fn token_counts_read_short() {
        assert_eq!(tokens(840), "840");
        assert_eq!(tokens(12_345), "12.3k");
        assert_eq!(tokens(1_250_000), "1.2M");
    }
}
