//! A session written out to read or share (T54): Markdown, one HTML file (its CSS inside, no
//! script — a Content-Security-Policy forbids one, and the model's raw HTML is written as text), or the messages as
//! JSON lines. What the model saw by default — results bounded, no system prompt — and with `full` the whole results
//! from the session's store and the system prompt. Cleaned: control, bidi and zero-width characters out, the owner's
//! home as `~`. Nothing is masked yet (T58): what a tool read is there as it read it.

use std::fmt::Write as _;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::{DateTime, Local};
use glut::Message;
use glut::model::Role;
use pulldown_cmark::{Event, Options, Parser, Tag};

/// The folder exports go to, below `~/.glut`: never the project's, where a commit would take one along.
const DIR: &str = "exports";
/// The most characters of a call's arguments a tool line in HTML names.
const SUMMARY_CHARS: usize = 80;

/// How a session is written out.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    Markdown,
    Html,
    Jsonl,
}

impl Format {
    pub fn parse(word: &str) -> Option<Format> {
        match word {
            "md" | "markdown" => Some(Format::Markdown),
            "html" => Some(Format::Html),
            "jsonl" | "json" => Some(Format::Jsonl),
            _ => None,
        }
    }

    pub fn extension(self) -> &'static str {
        match self {
            Format::Markdown => "md",
            Format::Html => "html",
            Format::Jsonl => "jsonl",
        }
    }
}

/// What is written out, from a kept session or a run's journal.
pub struct Export<'a> {
    pub title: &'a str,
    pub cwd: &'a Path,
    pub model: &'a str,
    pub when: Option<DateTime<Local>>,
    pub conversation: &'a [Message],
    /// The whole results and the system prompt too.
    pub full: bool,
    pub home: &'a Path,
    /// The store a bounded result's whole text is read from with `full`: its folder and its tag (the session's id).
    pub results: Option<(PathBuf, String)>,
}

/// `/export`'s words: `[md|html|jsonl|copy] [full] [path]`, in any order but the path last.
#[derive(Debug, PartialEq, Eq)]
pub struct Asked {
    pub format: Format,
    /// To the clipboard, as Markdown, instead of a file.
    pub copy: bool,
    pub full: bool,
    pub path: Option<String>,
}

pub fn parse(rest: &str) -> Asked {
    let mut asked = Asked {
        format: Format::Markdown,
        copy: false,
        full: false,
        path: None,
    };
    let mut rest = rest.trim();
    while let Some(word) = rest.split_whitespace().next() {
        match (word, Format::parse(word)) {
            (_, Some(format)) => asked.format = format,
            ("copy", _) => asked.copy = true,
            ("full", _) => asked.full = true,
            _ => break,
        }
        rest = rest[word.len()..].trim_start();
    }
    if !rest.is_empty() {
        asked.path = Some(rest.to_owned());
    }
    asked
}

/// The session in `format`.
pub fn render(export: &Export, format: Format) -> String {
    match format {
        Format::Markdown => markdown(export),
        Format::Html => html(export),
        Format::Jsonl => jsonl(export),
    }
}

/// The folder exports go to by default (T54), below the owner's `home`.
pub fn dir(home: &Path) -> PathBuf {
    home.join(glut::providers::DIR).join(DIR)
}

/// Where an export goes unless a path is named: `~/.glut/exports/<project>-<date>-<time>.<ext>`.
pub fn default_path(home: &Path, cwd: &Path, format: Format, now: DateTime<Local>) -> PathBuf {
    let project: String = cwd
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_else(|| "glut".into())
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    dir(home).join(format!(
        "{project}-{}.{}",
        now.format("%Y%m%d-%H%M%S"),
        format.extension()
    ))
}

/// A named path as the owner typed it: `~/` is the home, a relative one is below `cwd`.
pub fn named_path(typed: &str, home: &Path, cwd: &Path) -> PathBuf {
    match typed.strip_prefix("~/") {
        Some(rest) => home.join(rest),
        None => cwd.join(typed),
    }
}

/// Writes `text` to `path` as a new file only the owner can read (an export may hold secrets); never over one that is
/// there. The folder is made where it is Glut's own.
///
/// # Errors
///
/// The text the owner reads: the file is there, or cannot be written.
pub fn write(path: &Path, text: &str, home: &Path) -> Result<(), String> {
    use std::os::unix::fs::OpenOptionsExt as _;
    let own = dir(home);
    if path.parent() == Some(own.as_path()) {
        glut::providers::make_private_dir(&own)
            .map_err(|err| tr!(cannot_use, path = own.display(), reason = err))?;
    }
    let shown = path.display();
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|err| match err.kind() {
            std::io::ErrorKind::AlreadyExists => tr!(export_exists, path = shown),
            _ => tr!(cannot_write, path = shown, reason = err),
        })?;
    file.write_all(text.as_bytes())
        .map_err(|err| tr!(cannot_write, path = shown, reason = err))
}

/// What a kept session writes out: its whole results from its own store.
pub fn of_session<'a>(
    session: &'a crate::store::Session,
    full: bool,
    home: &'a Path,
) -> Export<'a> {
    Export {
        title: &session.title,
        cwd: &session.cwd,
        model: &session.model,
        when: Some(session.updated.with_timezone(&Local)),
        conversation: &session.conversation,
        full,
        home,
        results: Some((
            crate::store::results_dir(home, &session.id),
            session.id.clone(),
        )),
    }
}

/// `glut export`: a kept session (by its id or its file), or a run's journal (by its id or its file), written to `out`
/// or to stdout.
pub fn headless(
    what: &str,
    format: Format,
    full: bool,
    out: Option<&Path>,
) -> std::process::ExitCode {
    let fail = |message: String| {
        eprintln!("glut: {message}");
        std::process::ExitCode::from(1)
    };
    let home = match glut::permissions::settings::home_dir() {
        Ok(home) => home,
        Err(err) => return fail(err.to_string()),
    };
    let text = match source(what, &home) {
        Ok(Source::Session(session)) => render(&of_session(&session, full, &home), format),
        Ok(Source::Run(recorded)) => {
            let title = recorded
                .messages
                .iter()
                .find(|message| message.role == Role::User)
                .and_then(|message| message.content.as_deref())
                .map(crate::store::title_of)
                .unwrap_or_default();
            let export = Export {
                title: &title,
                cwd: &recorded.cwd,
                model: &recorded.model,
                when: None,
                conversation: &recorded.messages,
                full,
                home: &home,
                results: None,
            };
            render(&export, format)
        }
        Err(reason) => return fail(reason),
    };
    let written = match out {
        Some(path) => write(path, &text, &home),
        // A reader that stopped early (`| head`) took what it wanted: no error.
        None => match std::io::stdout().write_all(text.as_bytes()) {
            Err(err) if err.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
            done => done.map_err(|err| err.to_string()),
        },
    };
    match written {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(reason) => fail(reason),
    }
}

/// What `glut export` names.
enum Source {
    Session(crate::store::Session),
    Run(glut::journal::Recorded),
}

/// A file (a session's `.json`, a journal's `.jsonl`), else a session's id, else an unfinished run's.
fn source(what: &str, home: &Path) -> Result<Source, String> {
    let path = Path::new(what);
    if path.is_file() {
        return match path.extension().and_then(|ext| ext.to_str()) {
            Some("jsonl") => {
                let dir = path.parent().unwrap_or(Path::new("."));
                let run = path
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default();
                glut::journal::read_back(dir, run).map(Source::Run)
            }
            _ => crate::store::read(path).map(Source::Session),
        };
    }
    if let Ok(session) = crate::store::Session::load(home, what) {
        return Ok(Source::Session(session));
    }
    let runs = home.join(glut::providers::DIR).join("runs");
    glut::journal::read_back(&runs, what)
        .map(Source::Run)
        .map_err(|_| tr!(export_unknown, what = crate::transcript::clean(what)))
}

/// `text` cleaned for an export: control characters but line ends and tabs as spaces, bidi and zero-width marks out,
/// the owner's home as `~`.
fn cleaned(text: &str, home: &Path) -> String {
    let text: String = text
        .chars()
        .filter(|c| !crate::transcript::hides(*c))
        .map(|c| {
            if c.is_control() && c != '\n' && c != '\t' {
                ' '
            } else {
                c
            }
        })
        .collect();
    homeless(&text, home)
}

/// `text` with the owner's home written `~`, where it stands as a path's start.
fn homeless(text: &str, home: &Path) -> String {
    let home = home.to_string_lossy();
    if home.len() < 2 {
        return text.to_owned();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    let name_char = |c: char| c.is_alphanumeric() || c == '_' || c == '-' || c == '.';
    while let Some(at) = rest.find(home.as_ref()) {
        let after = &rest[at + home.len()..];
        // The home itself, not a folder of that name deeper down (`/Volumes/B/Users/me`) nor one with more to its name.
        let before = if at == 0 {
            out.chars().last()
        } else {
            rest[..at].chars().last()
        };
        let starts = before.is_none_or(|c| !(c == '/' || name_char(c)));
        let ends = after
            .chars()
            .next()
            .is_none_or(|c| c == '/' || !name_char(c));
        out.push_str(&rest[..at]);
        out.push_str(if starts && ends { "~" } else { home.as_ref() });
        rest = after;
    }
    out.push_str(rest);
    out
}

/// The model's thinking a message carries, as plain text.
fn thinking_of(message: &Message) -> Option<String> {
    let plain = message
        .reasoning
        .as_deref()
        .or(message.reasoning_content.as_deref())
        .filter(|text| !text.trim().is_empty());
    if let Some(text) = plain {
        return Some(text.to_owned());
    }
    // OpenRouter's and Anthropic's blocks: the readable ones only, never an encrypted or signed payload.
    let parts: Vec<&str> = message
        .reasoning_details
        .iter()
        .filter_map(|block| {
            block
                .get("text")
                .or_else(|| block.get("summary"))
                .or_else(|| block.get("thinking"))
                .and_then(serde_json::Value::as_str)
        })
        .filter(|text| !text.trim().is_empty())
        .collect();
    (!parts.is_empty()).then(|| parts.join("\n\n"))
}

/// A tool result's text: bounded as the model saw it, or — with `full` — whole from the session's store.
fn result_of(export: &Export, text: &str) -> String {
    if export.full
        && let Some((dir, tag)) = &export.results
        && let Some(whole) = glut::results::whole_of(dir, tag, text)
    {
        return whole;
    }
    text.to_owned()
}

/// A call's arguments as the export shows them: pretty JSON where they are JSON.
fn arguments(raw: &str) -> String {
    serde_json::from_str::<serde_json::Value>(raw)
        .ok()
        .and_then(|value| serde_json::to_string_pretty(&value).ok())
        .unwrap_or_else(|| raw.to_owned())
}

/// The header's line: where, on what, when.
fn meta_line(export: &Export) -> String {
    let when = export
        .when
        .map(|when| when.format(glut::lang::date_time()).to_string())
        .unwrap_or_default();
    let folder = homeless(&export.cwd.display().to_string(), export.home);
    [folder, export.model.to_owned(), when]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" · ")
}

/// A Markdown fence that `text` cannot close: one backtick more than its longest run, at least three.
fn fence(text: &str) -> String {
    let mut longest = 0;
    let mut run = 0;
    for c in text.chars() {
        run = if c == '`' { run + 1 } else { 0 };
        longest = longest.max(run);
    }
    "`".repeat((longest + 1).max(3))
}

fn markdown(export: &Export) -> String {
    let home = export.home;
    let mut out = format!(
        "# {}\n\n{}\n",
        cleaned(export.title, home).trim(),
        meta_line(export)
    );
    let names = call_names(export.conversation);
    for message in export.conversation {
        match message.role {
            Role::System if export.full => {
                let text = cleaned(message.content.as_deref().unwrap_or_default(), home);
                let fence = fence(&text);
                let _ = write!(
                    out,
                    "\n## {}\n\n{fence}\n{text}\n{fence}\n",
                    tr!(export_system)
                );
            }
            Role::System => {}
            Role::User => {
                let _ = write!(out, "\n## {}\n\n", tr!(export_owner));
                let text = cleaned(message.content.as_deref().unwrap_or_default(), home);
                out.push_str(text.trim_end());
                out.push('\n');
                for image in &message.images {
                    let _ = writeln!(out, "\n{}", image.label());
                }
            }
            Role::Assistant => {
                let _ = write!(out, "\n## {}\n", tr!(export_glut));
                if let Some(thinking) = thinking_of(message) {
                    let _ = write!(out, "\n> **{}**\n>\n", tr!(export_thinking));
                    for line in cleaned(&thinking, home).lines() {
                        let _ = writeln!(out, "> {line}");
                    }
                }
                if let Some(text) = message
                    .content
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                {
                    let _ = write!(out, "\n{}\n", cleaned(text, home).trim_end());
                }
                for call in &message.tool_calls {
                    let args = cleaned(&arguments(&call.function.arguments), home);
                    let fence = fence(&args);
                    let name = md_word(&cleaned(&call.function.name, home));
                    let _ = write!(
                        out,
                        "\n**{}**\n\n{fence}json\n{args}\n{fence}\n",
                        tr!(export_tool, name = name)
                    );
                }
            }
            Role::Tool => {
                let text = cleaned(
                    &result_of(export, message.content.as_deref().unwrap_or_default()),
                    home,
                );
                let fence = fence(&text);
                let name = message
                    .tool_call_id
                    .as_ref()
                    .and_then(|id| names.get(id.as_str()))
                    .map(|name| md_word(&cleaned(name, home)))
                    .unwrap_or_default();
                let _ = write!(
                    out,
                    "\n*{}*\n\n{fence}\n{}\n{fence}\n",
                    tr!(export_result, name = name),
                    text.trim_end()
                );
                for image in &message.images {
                    let _ = writeln!(out, "\n{}", image.label());
                }
            }
        }
    }
    out
}

/// A tool's name as one Markdown word: on one line, its marks escaped — a name is the model's, and must not make a
/// heading or close the emphasis around it.
fn md_word(name: &str) -> String {
    let mut out = String::with_capacity(name.len());
    for c in name.chars() {
        match c {
            '\n' | '\r' => out.push(' '),
            '\\' | '`' | '*' | '_' | '#' | '[' | ']' | '<' | '>' | '!' => {
                out.push('\\');
                out.push(c);
            }
            c => out.push(c),
        }
    }
    out
}

/// Each call's tool by the call's id, for the result that answers it.
fn call_names(conversation: &[Message]) -> std::collections::HashMap<&str, &str> {
    conversation
        .iter()
        .flat_map(|message| &message.tool_calls)
        .map(|call| (call.id.as_str(), call.function.name.as_str()))
        .collect()
}

/// `text` safe inside HTML, as text or in an attribute.
fn escaped(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            c => out.push(c),
        }
    }
    out
}

/// Markdown as HTML, with nothing in it that runs or reaches out: raw HTML is written as text, and a link or an image
/// that is not on the web stays its text.
fn markdown_html(text: &str) -> String {
    let options =
        Options::ENABLE_TABLES | Options::ENABLE_STRIKETHROUGH | Options::ENABLE_TASKLISTS;
    let safe = |dest: &str| dest.starts_with("https://") || dest.starts_with("http://");
    // Whether each open link was dropped, so its end goes with it — a link inside an image's text (CommonMark allows
    // one) must not lose its end and leave an `<a>` over the rest of the page.
    let mut links: Vec<bool> = Vec::new();
    let mut images = 0usize;
    let events = Parser::new_ext(text, options).filter_map(|event| match event {
        Event::Html(raw) | Event::InlineHtml(raw) => Some(Event::Text(raw)),
        // An image is never loaded (the policy forbids it anyway): its text stands for it.
        Event::Start(Tag::Image { .. }) => {
            images += 1;
            None
        }
        Event::End(pulldown_cmark::TagEnd::Image) => {
            images = images.saturating_sub(1);
            None
        }
        Event::Start(Tag::Link { ref dest_url, .. }) => {
            let dropped = images > 0 || !safe(dest_url);
            links.push(dropped);
            (!dropped).then_some(event)
        }
        Event::End(pulldown_cmark::TagEnd::Link) => (!links.pop().unwrap_or(true)).then_some(event),
        event => Some(event),
    });
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, events);
    out
}

const STYLE: &str = "body{font:15px/1.55 -apple-system,system-ui,sans-serif;max-width:52rem;margin:2rem auto;\
padding:0 1rem;color:#1d1d1f;background:#fff}h1{font-size:1.5rem;margin-bottom:.2rem}.meta{color:#6e6e73;\
margin-bottom:2rem}section{margin:1.4rem 0}h2.who{font-size:.85rem;text-transform:uppercase;letter-spacing:.06em;\
color:#6e6e73;margin:0 0 .4rem}.owner{border-left:3px solid #ff7a1a;padding-left:.9rem}pre{background:#f5f5f7;\
padding:.7rem;border-radius:6px;overflow-x:auto;white-space:pre-wrap;word-break:break-word;font:13px/1.45 \
ui-monospace,Menlo,monospace}details{margin:.5rem 0;border:1px solid #e5e5ea;border-radius:6px;padding:.3rem .7rem}\
summary{cursor:pointer;color:#3a3a3c;font:13px ui-monospace,Menlo,monospace}img{max-width:100%;border-radius:6px}\
@media (prefers-color-scheme:dark){body{color:#f5f5f7;background:#1c1c1e}pre{background:#2c2c2e}\
details{border-color:#3a3a3c}summary,h2.who,.meta{color:#aeaeb2}}";

fn html(export: &Export) -> String {
    let home = export.home;
    let title = escaped(cleaned(export.title, home).trim());
    let mut out = format!(
        "<!doctype html>\n<html lang=\"{}\"><head><meta charset=\"utf-8\">\
         <meta name=\"viewport\" content=\"width=device-width,initial-scale=1\">\
         <meta http-equiv=\"Content-Security-Policy\" content=\"default-src 'none'; img-src data:; \
         style-src 'unsafe-inline'; base-uri 'none'; form-action 'none'\">\
         <title>{title}</title><style>{STYLE}</style></head><body>\n\
         <h1>{title}</h1><div class=\"meta\">{}</div>\n",
        glut::lang::current().code(),
        escaped(&meta_line(export))
    );
    let names = call_names(export.conversation);
    // Each call's answer by the call's id, looked up once: a long session has thousands.
    let answers: std::collections::HashMap<&str, &Message> = export
        .conversation
        .iter()
        .filter_map(|message| Some((message.tool_call_id.as_deref()?, message)))
        .collect();
    for message in export.conversation {
        match message.role {
            Role::System if export.full => {
                let text = cleaned(message.content.as_deref().unwrap_or_default(), home);
                let _ = writeln!(
                    out,
                    "<details><summary>{}</summary><pre>{}</pre></details>",
                    escaped(tr!(export_system)),
                    escaped(&text)
                );
            }
            Role::System => {}
            Role::User => {
                let text = cleaned(message.content.as_deref().unwrap_or_default(), home);
                let _ = write!(
                    out,
                    "<section class=\"owner\"><h2 class=\"who\">{}</h2>{}",
                    escaped(tr!(export_owner)),
                    markdown_html(&text)
                );
                for image in &message.images {
                    out.push_str(&image_html(image));
                }
                out.push_str("</section>\n");
            }
            Role::Assistant => {
                let _ = write!(
                    out,
                    "<section><h2 class=\"who\">{}</h2>",
                    escaped(tr!(export_glut))
                );
                if let Some(thinking) = thinking_of(message) {
                    let _ = write!(
                        out,
                        "<details><summary>{}</summary><pre>{}</pre></details>",
                        escaped(tr!(export_thinking)),
                        escaped(&cleaned(&thinking, home))
                    );
                }
                if let Some(text) = message
                    .content
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                {
                    out.push_str(&markdown_html(&cleaned(text, home)));
                }
                for call in &message.tool_calls {
                    let args = cleaned(&arguments(&call.function.arguments), home);
                    let result = answers.get(call.id.as_str());
                    let summary: String = cleaned(&call.function.arguments, home)
                        .replace('\n', " ")
                        .chars()
                        .take(SUMMARY_CHARS)
                        .collect();
                    let _ = write!(
                        out,
                        "<details><summary>{} · {}</summary><pre>{}</pre>",
                        escaped(&cleaned(&call.function.name, home)),
                        escaped(&summary),
                        escaped(&args)
                    );
                    if let Some(result) = result {
                        let text = cleaned(
                            &result_of(export, result.content.as_deref().unwrap_or_default()),
                            home,
                        );
                        let _ = write!(out, "<pre>{}</pre>", escaped(text.trim_end()));
                        for image in &result.images {
                            out.push_str(&image_html(image));
                        }
                    }
                    out.push_str("</details>");
                }
                out.push_str("</section>\n");
            }
            // Written with the call it answers; one whose call is not in the conversation (a compaction kept it
            // apart) stands alone.
            Role::Tool => {
                let answered = message
                    .tool_call_id
                    .as_ref()
                    .is_some_and(|id| names.contains_key(id.as_str()));
                if !answered {
                    let text = cleaned(
                        &result_of(export, message.content.as_deref().unwrap_or_default()),
                        home,
                    );
                    let _ = writeln!(out, "<pre>{}</pre>", escaped(text.trim_end()));
                }
            }
        }
    }
    out.push_str("</body></html>\n");
    out
}

/// An image inside the page, or its line where its file is gone.
fn image_html(image: &glut::images::Image) -> String {
    match glut::images::base64_of(image) {
        Ok((media, data)) => format!(
            "<p><img alt=\"{}\" src=\"data:{media};base64,{data}\"></p>",
            escaped(&image.label())
        ),
        Err(_) => format!("<p>{}</p>", escaped(&image.label())),
    }
}

fn jsonl(export: &Export) -> String {
    let mut out = String::new();
    for message in export.conversation {
        if message.role == Role::System && !export.full {
            continue;
        }
        let mut message = message.clone();
        if message.role == Role::Tool
            && let Some(text) = &message.content
        {
            message.content = Some(result_of(export, text));
        }
        if let Ok(line) = serde_json::to_string(&message) {
            out.push_str(&homeless(&line, export.home));
            out.push('\n');
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glut::model::{FunctionCall, ToolCall};

    fn answer(text: &str) -> Message {
        Message {
            role: Role::Assistant,
            ..Message::user(text)
        }
    }

    fn conversation() -> Vec<Message> {
        let mut asked = answer("I look.");
        asked.reasoning = Some("the tests first".into());
        asked.tool_calls = vec![ToolCall {
            id: "c1".into(),
            kind: "function".into(),
            function: FunctionCall {
                name: "bash".into(),
                arguments: r#"{"command":"ls /home/owner/proj"}"#.into(),
            },
        }];
        let result = Message::tool("c1", "a.rs\n```\nb.rs\u{202e}");
        vec![
            Message::system("the system prompt"),
            Message::user("what is in /home/owner/proj? <script>alert(1)</script>"),
            asked,
            result,
            answer("Two files, see [here](javascript:alert(1)) and [docs](https://example.com)."),
        ]
    }

    fn export<'a>(conversation: &'a [Message], full: bool) -> Export<'a> {
        Export {
            title: "what is in it",
            cwd: Path::new("/home/owner/proj"),
            model: "deepseek",
            when: None,
            conversation,
            full,
            home: Path::new("/home/owner"),
            results: None,
        }
    }

    #[test]
    fn markdown_writes_what_the_model_saw_cleaned_with_the_home_as_tilde() {
        glut::lang::set_for_thread(Some(glut::lang::Lang::En));
        let messages = conversation();
        let md = render(&export(&messages, false), Format::Markdown);
        assert!(
            md.starts_with("# what is in it\n\n~/proj · deepseek\n"),
            "{md}"
        );
        assert!(
            !md.contains("the system prompt"),
            "no system prompt by default"
        );
        assert!(md.contains("> the tests first"), "thinking quoted: {md}");
        assert!(md.contains("ls ~/proj"), "{md}");
        assert!(!md.contains('\u{202e}'), "bidi marks out");
        // The result holds three backticks: its fence takes four.
        assert!(md.contains("````\na.rs\n```\nb.rs"), "{md}");
        assert!(render(&export(&messages, true), Format::Markdown).contains("the system prompt"));
        glut::lang::set_for_thread(None);
    }

    #[test]
    fn html_runs_nothing_and_reaches_nowhere() {
        glut::lang::set_for_thread(Some(glut::lang::Lang::En));
        let messages = conversation();
        let page = render(&export(&messages, false), Format::Html);
        assert!(
            page.contains("default-src 'none'"),
            "a policy that forbids scripts"
        );
        assert!(!page.contains("<script"), "the owner's raw HTML as text");
        assert!(page.contains("&lt;script&gt;"));
        assert!(
            !page.contains("javascript:"),
            "a link off the web stays its text: {page}"
        );
        assert!(page.contains("<a href=\"https://example.com\">docs</a>"));
        assert!(page.contains("<details><summary>bash · "), "{page}");
        // A link inside an image's text loses its start and its end alike: no `<a>` left open.
        let nested = markdown_html("![a [b](https://x.example) c](y.png) and after");
        assert_eq!(
            nested.matches("<a ").count(),
            nested.matches("</a>").count(),
            "{nested}"
        );
        assert!(!nested.contains("<a "), "{nested}");
        glut::lang::set_for_thread(None);
    }

    #[test]
    fn jsonl_keeps_the_messages_as_stored_and_full_adds_the_whole_results() {
        let dir = std::env::temp_dir().join(format!("glut-export-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let store = glut::results::ResultStore::persistent(glut::results::StoreSpec {
            dir: dir.clone(),
            tag: "s1".into(),
        });
        let whole = "y".repeat(40);
        let id = store.put(whole.clone());
        let mut messages = conversation();
        messages[3].content = Some(format!(
            "yy\n\n[… 36 of 40 characters left out of the middle; `read_full_result` with id \"{id}\" and an offset \
             reads the whole text …]\n\nyy"
        ));
        let lines = render(&export(&messages, false), Format::Jsonl);
        assert_eq!(lines.lines().count(), 4, "no system prompt");
        assert!(!lines.contains(&whole));
        let full = Export {
            results: Some((dir.clone(), "s1".into())),
            ..export(&messages, true)
        };
        let lines = render(&full, Format::Jsonl);
        assert_eq!(lines.lines().count(), 5);
        assert!(lines.contains(&whole), "the whole result from the store");
        for line in lines.lines() {
            serde_json::from_str::<Message>(line).unwrap();
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_words_say_format_copy_full_and_a_path_last() {
        assert_eq!(
            parse("html full ~/out file.html"),
            Asked {
                format: Format::Html,
                copy: false,
                full: true,
                path: Some("~/out file.html".into()),
            }
        );
        assert_eq!(parse("").format, Format::Markdown);
        assert!(parse("copy").copy);
        assert_eq!(parse("notes.md").path.as_deref(), Some("notes.md"));
        assert_eq!(
            homeless("/home/owner/a /home/ownerx", Path::new("/home/owner")),
            "~/a /home/ownerx"
        );
    }

    #[test]
    fn an_export_is_a_new_file_never_one_written_over() {
        let dir = std::env::temp_dir().join(format!("glut-export-file-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.md");
        let _ = std::fs::remove_file(&path);
        write(&path, "one", &dir).unwrap();
        assert!(write(&path, "two", &dir).is_err());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one");
        use std::os::unix::fs::PermissionsExt as _;
        assert_eq!(
            std::fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let _ = std::fs::remove_dir_all(&dir);
    }
}
