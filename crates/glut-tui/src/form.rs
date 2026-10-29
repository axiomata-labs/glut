//! A server's question on the screen (T67, MCP `elicitation/create`): a form of fields or a page to open, as a card
//! where the gate's questions stand. Only the owner's keys answer it, under the rules of the gate's card — nothing in
//! its first moment, no held key, ⏎ sends once every required field holds a value that fits.

use std::time::Instant;

use crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use glut::mcp::elicit::{Asked, Field, Form, Input, Kind, MAX_VALUE_CHARS, Problem, Reply, What};
use ratatui::style::{Modifier, Style};
use ratatui::text::{Line, Span};
use tokio::sync::oneshot;

use crate::theme;
use crate::transcript::{clean, cut, wrap};

/// The most rows a page's message takes, its line breaks as spaces: the host and the address come first.
const PAGE_MESSAGE_ROWS: usize = 3;
/// The most rows a form's message takes above its fields.
const FORM_MESSAGE_ROWS: usize = 6;

/// A server's question on the screen.
pub struct OpenForm {
    pub asked: Asked,
    reply: oneshot::Sender<Reply>,
    /// When the card came up: keys before the gate's settling time has passed do nothing.
    pub opened: Instant,
    /// What each field holds, in the form's order (none for a page).
    inputs: Vec<Input>,
    /// The option under the cursor of each field; only a choice's counts.
    cursors: Vec<usize>,
    /// The field the keys go to.
    focus: usize,
    /// Why ⏎ did not send: the field and what is wrong with it.
    problem: Option<(usize, Problem)>,
    /// The fields the card has shown so far: ⏎ sends only once every one was on the screen, its default with it.
    seen: Vec<bool>,
    /// ⏎ found a field not yet shown, which now has the keys.
    unseen: bool,
    /// The owner began with ⇥, ↑↓ or ⏎: until then nothing typed goes into a field — what was typed for the input as
    /// the card came up must not land in it.
    armed: bool,
}

/// The card's lines as drawn at a width.
pub struct Drawn {
    pub lines: Vec<Line<'static>>,
    /// The rows that must stay in sight: the field with the keys, what is said under it — or a page's host.
    pub keep: std::ops::Range<usize>,
    /// The row of each field.
    pub fields: Vec<usize>,
}

/// What a key did to the card.
#[derive(Debug, PartialEq)]
pub enum Step {
    Nothing,
    /// The owner answered.
    Answered(Reply),
    /// `o` on a page: open it, then the server hears an accept.
    Open,
}

impl OpenForm {
    pub fn new(asked: Asked, reply: oneshot::Sender<Reply>) -> Self {
        let (inputs, cursors) = match &asked.what {
            What::Form(form) => (
                form.fields.iter().map(Field::start).collect(),
                form.fields
                    .iter()
                    .map(|field| match (&field.kind, field.start()) {
                        (Kind::Choice { .. }, Input::Choice(Some(at))) => at,
                        _ => 0,
                    })
                    .collect(),
            ),
            What::Page { .. } | What::Secret { .. } => (Vec::new(), Vec::new()),
        };
        OpenForm {
            asked,
            reply,
            opened: Instant::now(),
            inputs,
            cursors,
            focus: 0,
            problem: None,
            seen: Vec::new(),
            unseen: false,
            armed: false,
        }
    }

    /// Whether the owner began to fill in the form.
    pub fn armed(&self) -> bool {
        self.armed || self.is_page()
    }

    /// The fields at `fields` were on the screen.
    pub fn shown(&mut self, fields: impl IntoIterator<Item = usize>) {
        let count = self.inputs.len();
        self.seen.resize(count, false);
        for at in fields {
            if let Some(seen) = self.seen.get_mut(at) {
                *seen = true;
            }
        }
    }

    /// A paste into the field with the keys, its line breaks as spaces — once the owner began.
    pub fn paste(&mut self, text: &str) {
        if !self.armed {
            return;
        }
        for c in text.chars() {
            self.type_char(if c == '\n' || c == '\r' || c == '\t' {
                ' '
            } else {
                c
            });
        }
    }

    /// Sends the owner's answer to the server.
    pub fn answer(self, reply: Reply) {
        let _ = self.reply.send(reply);
    }

    /// Whether the answer is no longer awaited: the call that asked ended.
    pub fn abandoned(&self) -> bool {
        self.reply.is_closed()
    }

    /// Whether this is a page to open rather than a form.
    pub fn is_page(&self) -> bool {
        matches!(self.asked.what, What::Page { .. })
    }

    fn form(&self) -> Option<&Form> {
        match &self.asked.what {
            What::Form(form) => Some(form),
            _ => None,
        }
    }

    /// One key of the owner's, once the card has settled. An answer comes only from a plain key pressed once — ⌃D
    /// the one chord, for a decline; a held key may type into a field but never sends, cancels or declines.
    pub fn key(&mut self, key: KeyEvent) -> Step {
        let held = key.kind == KeyEventKind::Repeat;
        let control = key.modifiers.contains(KeyModifiers::CONTROL);
        // ⌥ types what it makes (`@` on a German Mac keyboard); it answers nothing.
        if key.modifiers.contains(KeyModifiers::ALT) && !control {
            if let KeyCode::Char(c) = key.code
                && self.armed
            {
                self.type_char(c);
            }
            return Step::Nothing;
        }
        if control {
            return match key.code {
                KeyCode::Char('d') if !held => Step::Answered(Reply::Decline),
                _ => Step::Nothing,
            };
        }
        if key.code == KeyCode::Esc {
            return if held {
                Step::Nothing
            } else {
                Step::Answered(Reply::Cancel)
            };
        }
        if self.is_page() {
            return match key.code {
                KeyCode::Char('o') if !held => Step::Open,
                KeyCode::Char('n') if !held => Step::Answered(Reply::Decline),
                _ => Step::Nothing,
            };
        }
        let Some(form) = self.form() else {
            return Step::Nothing;
        };
        let count = form.fields.len();
        if !self.armed {
            if matches!(
                key.code,
                KeyCode::Enter | KeyCode::Tab | KeyCode::BackTab | KeyCode::Up | KeyCode::Down
            ) && !held
            {
                self.armed = true;
            }
            return Step::Nothing;
        }
        match key.code {
            KeyCode::Enter if !held => {
                if let Some(at) =
                    (0..count).find(|at| !self.seen.get(*at).copied().unwrap_or(false))
                {
                    self.focus = at;
                    self.unseen = true;
                    self.problem = None;
                    return Step::Nothing;
                }
                return match form.answer(&self.inputs) {
                    Ok(content) => Step::Answered(Reply::Accept(content)),
                    Err((at, problem)) => {
                        self.focus = at;
                        self.problem = Some((at, problem));
                        self.unseen = false;
                        Step::Nothing
                    }
                };
            }
            KeyCode::Tab | KeyCode::Down if count > 0 => self.focus = (self.focus + 1) % count,
            KeyCode::BackTab | KeyCode::Up if count > 0 => {
                self.focus = (self.focus + count - 1) % count
            }
            KeyCode::Left | KeyCode::Right => self.move_cursor(key.code == KeyCode::Right),
            KeyCode::Char(' ') => self.toggle(),
            KeyCode::Char(c) => self.type_char(c),
            KeyCode::Backspace => {
                if let Some(Input::Text(text)) = self.inputs.get_mut(self.focus) {
                    text.pop();
                    self.problem = None;
                }
            }
            _ => {}
        }
        Step::Nothing
    }

    fn options(&self) -> usize {
        self.form()
            .and_then(|form| form.fields.get(self.focus))
            .map_or(0, |field| match &field.kind {
                Kind::Choice { options, .. } | Kind::Choices { options, .. } => options.len(),
                _ => 0,
            })
    }

    fn move_cursor(&mut self, right: bool) {
        let options = self.options();
        if let Some(cursor) = self.cursors.get_mut(self.focus)
            && options > 0
        {
            *cursor = if right {
                (*cursor + 1).min(options - 1)
            } else {
                cursor.saturating_sub(1)
            };
        }
    }

    /// Space: ticks a yes/no, picks the option under the cursor, or types a space into a text.
    fn toggle(&mut self) {
        let cursor = self.cursors.get(self.focus).copied().unwrap_or_default();
        match self.inputs.get_mut(self.focus) {
            Some(Input::Bool(value)) => *value = !*value,
            Some(Input::Choice(picked)) => *picked = Some(cursor),
            Some(Input::Choices(picked)) => {
                if let Some(one) = picked.get_mut(cursor) {
                    *one = !*one;
                }
            }
            Some(Input::Text(_)) => return self.type_char(' '),
            None => return,
        }
        self.problem = None;
    }

    fn type_char(&mut self, c: char) {
        if let Some(Input::Text(text)) = self.inputs.get_mut(self.focus)
            && text.chars().count() < MAX_VALUE_CHARS
            && !c.is_control()
        {
            text.push(c);
            self.problem = None;
        }
    }

    /// The card's title: who asks.
    pub fn title(&self) -> String {
        let server = cut(&clean(&self.asked.server), 40);
        match &self.asked.from {
            Some(from) => format!(
                " ⬡ {} ",
                tr!(
                    form_title_from,
                    from = cut(&clean(from), 40),
                    server = server
                )
            ),
            None => format!(" ⬡ {} ", tr!(form_title, server = server)),
        }
    }

    /// The card's lines at `width` in at most `most` rows for a page; a form's are all there, the view keeps
    /// [`Drawn::keep`] in sight.
    pub fn lines(&self, width: usize, most: usize) -> Drawn {
        let mut lines: Vec<Line<'static>> = Vec::new();
        match &self.asked.what {
            What::Page { message, url } => {
                // The host first and apart, where no message can push it away or stand above it in its form.
                let host = glut::mcp::elicit::host_of(url);
                let mut head = vec![
                    Span::styled("→ ", theme::glow()),
                    Span::styled(clean(&host), theme::glow().add_modifier(Modifier::BOLD)),
                ];
                if !url.starts_with("https://") {
                    head.push(Span::styled("  ⚠ http", theme::fault()));
                }
                lines.push(Line::from(head));
                let said: Vec<String> = wrap(&message.replace(['\n', '\r'], " "), width)
                    .into_iter()
                    .take(PAGE_MESSAGE_ROWS)
                    .collect();
                // The address whole where it fits, else its head and its tail.
                let room = most.saturating_sub(2 + said.len()).max(2);
                let rows = wrap(url, width);
                let shown: Vec<String> = if rows.len() <= room {
                    rows
                } else {
                    let mut kept: Vec<String> = rows[..room - 1].to_vec();
                    kept.push(format!("… {}", rows[rows.len() - 1]));
                    kept
                };
                lines.extend(
                    shown
                        .into_iter()
                        .map(|row| Line::from(Span::styled(row, theme::quiet()))),
                );
                lines.extend(said.into_iter().map(Line::from));
                lines.push(Line::from(vec![
                    key_span("o"),
                    Span::raw(tr!(page_open_key)),
                    Span::raw("   "),
                    key_span("n"),
                    Span::raw(tr!(refuse)),
                ]));
                Drawn {
                    lines,
                    keep: 0..1,
                    fields: Vec::new(),
                }
            }
            What::Form(form) => {
                lines.extend(
                    wrap(&form.message, width)
                        .into_iter()
                        .take(FORM_MESSAGE_ROWS)
                        .map(Line::from),
                );
                let mut keep = lines.len()..lines.len() + 1;
                let mut fields = Vec::new();
                for (at, field) in form.fields.iter().enumerate() {
                    let focused = at == self.focus;
                    fields.push(lines.len());
                    if focused {
                        keep.start = lines.len();
                    }
                    lines.push(self.field_line(at, field, focused, width));
                    if focused && let Some(description) = &field.description {
                        lines.extend(
                            wrap(description, width.saturating_sub(4))
                                .into_iter()
                                .take(2)
                                .map(|row| {
                                    Line::from(Span::styled(format!("    {row}"), theme::quiet()))
                                }),
                        );
                    }
                    // What is wrong stands under its field.
                    let said = match &self.problem {
                        Some((wrong, problem)) if *wrong == at => {
                            Some(problem_text(&clean(field.label()), problem))
                        }
                        _ if focused && self.unseen => Some(tr!(form_see_all).to_owned()),
                        _ => None,
                    };
                    if let Some(said) = said {
                        lines.push(Line::from(Span::styled(
                            format!("    {}", cut(&said, width.saturating_sub(4))),
                            theme::fault(),
                        )));
                    }
                    if focused {
                        keep.end = lines.len();
                    }
                }
                Drawn {
                    lines,
                    keep,
                    fields,
                }
            }
            What::Secret { .. } => Drawn {
                lines,
                keep: 0..0,
                fields: Vec::new(),
            },
        }
    }

    fn field_line(&self, at: usize, field: &Field, focused: bool, width: usize) -> Line<'static> {
        let marker = if focused { "› " } else { "  " };
        let required = if field.required { " *" } else { "" };
        let label = cut(&clean(field.label()), (width / 3).max(8));
        let mut spans = vec![
            Span::styled(marker, theme::glow()),
            Span::styled(
                format!("{label}{required}: "),
                if focused {
                    Style::default().add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                },
            ),
        ];
        let used: usize = spans.iter().map(|span| span.content.chars().count()).sum();
        let room = width.saturating_sub(used).max(4);
        let cursor = self.cursors.get(at).copied().unwrap_or_default();
        match (&field.kind, self.inputs.get(at)) {
            (_, Some(Input::Text(text))) => {
                let shown = clean(text);
                // The end of what is typed stays in sight: a long text shows its tail.
                let count = shown.chars().count();
                let tail: String = shown
                    .chars()
                    .skip(count.saturating_sub(room.saturating_sub(1)))
                    .collect();
                spans.push(Span::raw(tail));
                if focused {
                    spans.push(Span::styled("▏", theme::glow()));
                }
            }
            (_, Some(Input::Bool(value))) => {
                spans.push(Span::raw(if *value { "[x]" } else { "[ ]" }));
            }
            (Kind::Choice { options, .. }, Some(Input::Choice(picked))) => {
                let marks: Vec<(String, bool)> = options
                    .iter()
                    .enumerate()
                    .map(|(at, option)| {
                        (
                            format!(
                                "{} {}",
                                if *picked == Some(at) { "(•)" } else { "( )" },
                                clean(&option.label)
                            ),
                            at == cursor,
                        )
                    })
                    .collect();
                spans.extend(option_spans(marks, focused, room));
            }
            (Kind::Choices { options, .. }, Some(Input::Choices(picked))) => {
                let marks: Vec<(String, bool)> = options
                    .iter()
                    .zip(picked)
                    .enumerate()
                    .map(|(at, (option, on))| {
                        (
                            format!(
                                "{} {}",
                                if *on { "[x]" } else { "[ ]" },
                                clean(&option.label)
                            ),
                            at == cursor,
                        )
                    })
                    .collect();
                spans.extend(option_spans(marks, focused, room));
            }
            _ => {}
        }
        Line::from(spans)
    }
}

/// The options of a choice in `room` cells, from the first one the cursor's still fits after — `…` where some are
/// left out —, the one under the cursor reversed while the field has the keys.
fn option_spans(marks: Vec<(String, bool)>, focused: bool, room: usize) -> Vec<Span<'static>> {
    let cursor = marks.iter().position(|(_, on)| *on).unwrap_or_default();
    let width = |text: &str| text.chars().count() + 2;
    let mut first = 0;
    while first < cursor
        && marks[first..=cursor]
            .iter()
            .map(|(text, _)| width(text))
            .sum::<usize>()
            > room
    {
        first += 1;
    }
    let mut spans = Vec::new();
    let mut used = 0;
    if first > 0 {
        spans.push(Span::styled("… ", theme::quiet()));
        used += 2;
    }
    for (text, under) in marks.into_iter().skip(first) {
        if used + width(&text) > room {
            spans.push(Span::styled("…", theme::quiet()));
            break;
        }
        used += width(&text);
        let style = if under && focused {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            Style::default()
        };
        spans.push(Span::styled(text, style));
        spans.push(Span::raw("  "));
    }
    spans
}

fn key_span(key: &str) -> Span<'static> {
    Span::styled(
        format!(" {key}  "),
        theme::glow().add_modifier(Modifier::BOLD),
    )
}

/// What is wrong with the value of the field `field`, in the owner's language.
fn problem_text(field: &str, problem: &Problem) -> String {
    let number = |n: f64| glut::lang::decimal(n, if n.fract() == 0.0 { 0 } else { 2 });
    match problem {
        Problem::Required => tr!(form_required, field = field),
        Problem::TooShort(n) => tr!(form_too_short, field = field, n = n),
        Problem::TooLong(n) => tr!(form_too_long, field = field, n = n),
        Problem::NotEmail => tr!(form_not_email, field = field),
        Problem::NotUri => tr!(form_not_uri, field = field),
        Problem::NotDate => tr!(form_not_date, field = field),
        Problem::NotDateTime => tr!(form_not_date_time, field = field),
        Problem::NotNumber => tr!(form_not_number, field = field),
        Problem::NotInteger => tr!(form_not_integer, field = field),
        Problem::Below(n) => tr!(form_below, field = field, n = number(*n)),
        Problem::Above(n) => tr!(form_above, field = field, n = number(*n)),
        Problem::TooFew(n) => tr!(form_too_few, field = field, n = n),
        Problem::TooMany(n) => tr!(form_too_many, field = field, n = n),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }

    fn open(params: serde_json::Value) -> (OpenForm, oneshot::Receiver<Reply>) {
        let (reply, answer) = oneshot::channel();
        let asked = Asked {
            server: "mail".into(),
            from: None,
            what: glut::mcp::elicit::parse(&params).unwrap(),
        };
        (OpenForm::new(asked, reply), answer)
    }

    fn typed(form: &mut OpenForm, text: &str) {
        for c in text.chars() {
            form.key(press(KeyCode::Char(c)));
        }
    }

    #[test]
    fn a_form_is_filled_in_with_the_keys_and_sent_once_it_fits() {
        let (mut form, _answer) = open(json!({"message": "Who?", "requestedSchema": {
            "type": "object",
            "properties": {
                "name": {"type": "string"},
                "ok": {"type": "boolean"},
                "size": {"type": "string", "enum": ["s", "m", "l"]},
                "tags": {"type": "array", "items": {"enum": ["a", "b"]}},
            },
            "required": ["name", "size"],
        }}));
        // Typed before the owner began: nothing lands in a field, and the first ⏎ only begins.
        typed(&mut form, "for the input");
        assert_eq!(form.key(press(KeyCode::Enter)), Step::Nothing);
        assert_eq!(form.inputs[0], Input::Text(String::new()));
        // Not every field was on the screen yet: ⏎ goes to the first one unseen.
        form.shown([0, 1]);
        assert_eq!(form.key(press(KeyCode::Enter)), Step::Nothing);
        assert!(form.unseen && form.focus == 2);
        form.shown([2, 3]);
        form.focus = 0;
        // Nothing typed yet: ⏎ names the first field that is missing and sends nothing.
        assert_eq!(form.key(press(KeyCode::Enter)), Step::Nothing);
        assert_eq!(form.problem, Some((0, Problem::Required)));
        typed(&mut form, "Ada L");
        form.key(press(KeyCode::Backspace));
        form.key(press(KeyCode::Tab));
        form.key(press(KeyCode::Char(' ')));
        form.key(press(KeyCode::Down));
        form.key(press(KeyCode::Right));
        form.key(press(KeyCode::Char(' ')));
        form.key(press(KeyCode::Tab));
        form.key(press(KeyCode::Right));
        form.key(press(KeyCode::Char(' ')));
        // A held ⏎ never sends.
        let mut held = press(KeyCode::Enter);
        held.kind = KeyEventKind::Repeat;
        assert_eq!(form.key(held), Step::Nothing);
        let Step::Answered(Reply::Accept(content)) = form.key(press(KeyCode::Enter)) else {
            panic!("not sent");
        };
        assert_eq!(
            serde_json::Value::Object(content),
            json!({"name": "Ada", "ok": true, "size": "m", "tags": ["b"]})
        );
    }

    #[test]
    fn esc_cancels_ctrl_d_declines_and_other_chords_do_nothing() {
        let (mut form, _answer) = open(json!({"message": "m", "requestedSchema": {
            "type": "object", "properties": {"name": {"type": "string"}}}}));
        assert_eq!(
            form.key(KeyEvent::new(KeyCode::Char('a'), KeyModifiers::CONTROL)),
            Step::Nothing
        );
        assert_eq!(
            form.key(KeyEvent::new(KeyCode::Char('d'), KeyModifiers::CONTROL)),
            Step::Answered(Reply::Decline)
        );
        assert_eq!(form.key(press(KeyCode::Esc)), Step::Answered(Reply::Cancel));
        // Letters type into the field; `n` declines only a page.
        assert_eq!(form.key(press(KeyCode::Char('n'))), Step::Nothing);
        // ⌥ types what it makes, once the owner began.
        form.key(press(KeyCode::Tab));
        form.key(KeyEvent::new(KeyCode::Char('@'), KeyModifiers::ALT));
        form.paste("a\nb");
        assert_eq!(form.inputs[0], Input::Text("@a b".into()));
    }

    #[test]
    fn a_page_opens_only_on_o_and_shows_its_host() {
        let (mut page, _answer) = open(json!({"mode": "url", "message": "Sign in",
            "url": "https://login.example.com/x?y=1"}));
        assert_eq!(page.key(press(KeyCode::Enter)), Step::Nothing);
        assert_eq!(page.key(press(KeyCode::Char('o'))), Step::Open);
        assert_eq!(
            page.key(press(KeyCode::Char('n'))),
            Step::Answered(Reply::Decline)
        );
        let lines = page.lines(80, 16).lines;
        let text: Vec<String> = lines.iter().map(|line| line.to_string()).collect();
        assert!(
            text.iter().any(|line| line == "→ login.example.com"),
            "{text:?}"
        );
        assert!(
            text.iter()
                .any(|line| line.contains("https://login.example.com/x?y=1"))
        );
    }

    #[test]
    fn a_long_message_never_pushes_a_pages_host_away_nor_fakes_it() {
        let message = format!("Sign in\n→ github.com\n{}", "x\n".repeat(200));
        let (page, _answer) = open(json!({"mode": "url", "message": message,
            "url": format!("http://evil.example/{}", "p".repeat(1_500))}));
        let drawn = page.lines(60, 8);
        let text: Vec<String> = drawn.lines.iter().map(|line| line.to_string()).collect();
        assert_eq!(text[0], "→ evil.example  ⚠ http", "{text:?}");
        assert_eq!(drawn.keep, 0..1);
        assert!(text.len() <= 9, "{}", text.len());
        assert!(
            !text[1..].iter().any(|line| line.starts_with("→ ")),
            "the message's lines are one: {text:?}"
        );
    }

    #[test]
    fn a_servers_words_reach_the_screen_cleaned() {
        let (form, _answer) = open(json!({"message": "Hi\u{202e}there", "requestedSchema": {
            "type": "object", "properties": {"x\u{200b}y": {"type": "string", "title": "Na\u{202e}me"}}}}));
        let drawn = form.lines(60, 16);
        let (lines, focus) = (drawn.lines, drawn.keep.start);
        let text: String = lines.iter().map(|line| line.to_string()).collect();
        assert!(
            !text.contains('\u{202e}') && !text.contains('\u{200b}'),
            "{text:?}"
        );
        assert_eq!(focus, 1, "the first field stands under the message");
    }
}
