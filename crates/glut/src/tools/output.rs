//! Compact outputs of frequent commands (Glut lean, N3): when `bash` runs one of them, the loop rewrites its *output*
//! before the conversation gets it — never the command. Lines without content go (progress, "Compiling …", passing
//! tests, git's hints, colour codes); everything else stays word for word, and the raw output goes into the run's
//! store (`results.rs`), so nothing is lost: the note names the id `read_full_result` reads it back with.
//!
//! * **Only a simple command**: one program (after `cd dir &&` and `VAR=value`), no pipe, no redirection but
//!   `2>&1` — a model that pipes through `grep` or `tail` has already chosen what it wants to see.
//! * **The model's own format wins**: `git status --porcelain`, `git log --oneline`, `cargo test -q` … stay as they
//!   are.
//! * **Only where it pays**: a short output, or one that would shrink by less than a fifth, is left whole.

use std::collections::BTreeMap;
use std::sync::LazyLock;

use regex::Regex;

/// Outputs shorter than this many lines are never compacted: there is little to gain, and the model reads them as is.
const MIN_LINES: usize = 12;
/// A compacted output must be at most this share (in percent) of the original's characters, or it is left whole.
const MAX_SHARE_PERCENT: usize = 80;
/// For a diff, which is mostly changed lines that all stay (measured on this repo: ~80 % of a code diff), less pays.
const MAX_DIFF_SHARE_PERCENT: usize = 90;
/// A listing (`find`, `ls`, `tree`) longer than this many lines is grouped by folder.
const LISTING_LINES: usize = 60;
/// In a grouped listing, a folder with more entries than this shows only its first ones and a count.
const FOLDER_ENTRIES: usize = 8;

/// A compacted output: its text and what was left out, for the note.
#[derive(Debug, PartialEq)]
pub(crate) struct Compacted {
    pub text: String,
    /// The original's line count.
    pub lines_before: usize,
    /// The compacted text's line count.
    pub lines_after: usize,
    /// What was left out, in a few words ("progress and passing tests").
    pub left_out: &'static str,
    /// Which kind of output it was, for the count of savings (N9): "cargo", "git diff" …
    pub what: &'static str,
}

/// The kinds of output Glut knows how to compact.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    GitStatus,
    GitDiff,
    GitLog,
    Cargo,
    Install,
    JsTests,
    SvelteCheck,
    Pytest,
    Swift,
    Xcodebuild,
    Make,
    Listing,
    Tree,
}

impl Kind {
    /// The kind's name in the count of savings (N9).
    fn name(self) -> &'static str {
        match self {
            Kind::GitStatus => "git status",
            Kind::GitDiff => "git diff",
            Kind::GitLog => "git log",
            Kind::Cargo => "cargo",
            Kind::Install => "install",
            Kind::JsTests => "js tests",
            Kind::SvelteCheck => "svelte-check",
            Kind::Pytest => "pytest",
            Kind::Swift => "swift",
            Kind::Xcodebuild => "xcodebuild",
            Kind::Make => "make",
            Kind::Listing => "listing",
            Kind::Tree => "tree",
        }
    }
}

/// The output of `command` compacted, or `None` when the command is none Glut knows, the model chose its own format, or
/// compacting would not pay.
pub(crate) fn compact(command: &str, output: &str) -> Option<Compacted> {
    let words = simple_command(command)?;
    let kind = kind_of(&words)?;
    let lines_before = output.lines().count();
    if lines_before < MIN_LINES {
        return None;
    }
    // A failed command comes back as `error: <its output>`: the prefix is put aside so the filters see the output's
    // own first line, and put back afterwards.
    let (prefix, body) = match output.strip_prefix("error: ") {
        Some(body) => ("error: ", body),
        None => ("", output),
    };
    let clean = strip_ansi(body);
    // `git show rev:path` prints a file, a combined merge diff marks its lines differently: neither is a plain diff.
    if kind == Kind::GitDiff
        && (!clean
            .lines()
            .any(|l| l.starts_with("diff --git ") || l.starts_with("@@"))
            || clean
                .lines()
                .any(|l| l.starts_with("diff --cc ") || l.starts_with("diff --combined ")))
    {
        return None;
    }
    let (lines, left_out): (Vec<String>, &'static str) = match kind {
        Kind::GitStatus => (
            git_status(&clean),
            "git's hints and blank lines; entries in short form",
        ),
        Kind::GitDiff => (
            git_diff(&clean),
            "index and ---/+++ lines, unchanged lines beyond one around each change",
        ),
        Kind::GitLog => (git_log(&clean), "commit bodies; one line a commit"),
        Kind::Cargo => (
            cargo(&clean),
            "build progress, passing and empty test suites",
        ),
        Kind::Install => (install(&clean), "download and progress lines"),
        Kind::JsTests => (js_tests(&clean), "passing tests and files"),
        Kind::SvelteCheck => (svelte_check(&clean), "progress lines"),
        Kind::Pytest => (pytest(&clean), "session header and passing tests"),
        Kind::Swift => (
            swift(&clean),
            "build progress, started and passing tests, the compiler's command lines",
        ),
        Kind::Xcodebuild => (
            xcodebuild(&clean),
            "build steps and their command lines, passing tests",
        ),
        Kind::Make => (make(&clean), "directory changes and progress lines"),
        Kind::Listing => (
            listing(&clean),
            "entries of large folders beyond the first few",
        ),
        Kind::Tree => (
            tree(&clean),
            "entries of large folders beyond the first few",
        ),
    };
    // A diff's trailing spaces can be the change itself: its lines stay exactly as they are.
    let text = format!("{prefix}{}", squeeze_blank(&lines, kind != Kind::GitDiff));
    let share = if kind == Kind::GitDiff {
        MAX_DIFF_SHARE_PERCENT
    } else {
        MAX_SHARE_PERCENT
    };
    let paid = text.chars().count() * 100 <= output.chars().count() * share;
    paid.then(|| Compacted {
        lines_after: text.lines().count(),
        text,
        lines_before,
        left_out,
        what: kind.name(),
    })
}

/// The command's words when it is one program — after `cd <dir> &&` and `VAR=value` prefixes, with a trailing `2>&1`
/// dropped; `None` for a pipeline, a redirection, a list or a substitution.
fn simple_command(command: &str) -> Option<Vec<&str>> {
    let mut rest = command.trim();
    while let Some(after) = rest.strip_prefix("cd ") {
        let (_, tail) = after.split_once("&&")?;
        rest = tail.trim();
    }
    let rest = rest.strip_suffix("2>&1").unwrap_or(rest).trim();
    let forbidden = ['|', ';', '>', '<', '`', '&', '\n'];
    if rest.is_empty() || rest.contains(forbidden) || rest.contains("$(") {
        return None;
    }
    let words: Vec<&str> = rest
        .split_whitespace()
        .skip_while(|word| word.contains('=') && !word.starts_with('-'))
        .collect();
    (!words.is_empty()).then_some(words)
}

/// A chain of commands worth compacting part by part (N7): two parts or more, each a simple command or a `cd`, and
/// one of them a command Glut knows.
#[derive(Debug, PartialEq)]
pub(crate) struct Chain<'a> {
    /// The parts as the model wrote them, trimmed.
    pub parts: Vec<&'a str>,
    /// The operator after each part but the last: `&&`, `||` or `;`.
    pub ops: Vec<&'a str>,
}

/// `command` as a [`Chain`], split at its `&&`, `||` and `;` outside quotes; `None` when it is a single command (after
/// `cd dir &&`), a part is no simple command (a pipe, a redirection, a substitution), or no part is one Glut knows.
///
/// The marks put each part on lines of its own inside braces, so the chain may hold nothing that would read otherwise
/// there: the gate's own reader must vouch for every bit of it (no comment, grouping, keyword, substitution), and no
/// part may use `$` (a mark's `printf` would be the "last command" for `$?` and `$_`) or a backslash (a trailing one
/// would join the closing brace onto the part).
pub(crate) fn chain(command: &str) -> Option<Chain<'_>> {
    // `cd dir && cargo test` is one command to [`compact`] already: no marks needed.
    if simple_command(command).is_some()
        || command.contains(['$', '\\', '#'])
        || crate::permissions::command::analyze(command).is_opaque()
    {
        return None;
    }
    let (parts, ops) = split_chain(command)?;
    if parts.len() < 2 {
        return None;
    }
    let mut known = false;
    for part in &parts {
        match simple_command(part) {
            Some(words) => known |= kind_of(&words).is_some(),
            None if is_cd(part) => {}
            None => return None,
        }
    }
    known.then_some(Chain { parts, ops })
}

/// The command's parts and the operators between them, split outside quotes and escapes; `None` for an empty part or
/// a `;;`, which no chain of simple commands has.
fn split_chain(command: &str) -> Option<(Vec<&str>, Vec<&str>)> {
    let bytes = command.as_bytes();
    let (mut parts, mut ops) = (Vec::new(), Vec::new());
    let (mut start, mut i) = (0, 0);
    let (mut single, mut double) = (false, false);
    while i < bytes.len() {
        let byte = bytes[i];
        let op_len = match byte {
            _ if single => {
                single = byte != b'\'';
                0
            }
            b'\\' => {
                // The escaped byte is skipped: it opens no quote and ends no part.
                i += 2;
                continue;
            }
            b'"' => {
                double = !double;
                0
            }
            _ if double => 0,
            b'\'' => {
                single = true;
                0
            }
            b'&' | b'|' if bytes.get(i + 1) == Some(&byte) => 2,
            b';' => 1,
            _ => 0,
        };
        if op_len > 0 {
            let part = command[start..i].trim();
            if part.is_empty() || bytes.get(i + op_len) == Some(&b';') {
                return None;
            }
            parts.push(part);
            ops.push(&command[i..i + op_len]);
            i += op_len;
            start = i;
        } else {
            i += 1;
        }
    }
    let last = command[start..].trim();
    if last.is_empty() || single || double {
        return None;
    }
    parts.push(last);
    Some((parts, ops))
}

/// Whether `part` only changes the folder: `cd <dir>`, no more.
fn is_cd(part: &str) -> bool {
    let forbidden = ['|', ';', '>', '<', '`', '&', '\n', '$'];
    let mut words = part.split_whitespace();
    words.next() == Some("cd") && words.count() <= 1 && !part.contains(forbidden)
}

/// The command a pipe feeds when it is one Glut knows (N8: counted, not compacted): `cargo test 2>&1 | tail -30`
/// names "cargo".
pub(crate) fn piped(command: &str) -> Option<&'static str> {
    let (first, rest) = command.split_once('|')?;
    if rest.starts_with('|') {
        return None;
    }
    simple_command(first)
        .and_then(|words| kind_of(&words))
        .map(Kind::name)
}

/// The chain as it runs (N7): each part in braces behind a line with `mark` and its number, the operators as they
/// were — a brace group runs in the shell itself, so a `cd` still moves the folder and `&&`/`||` see the part's own
/// exit status. A part sits on lines of its own, so a trailing comment cannot swallow the closing brace. The mark and
/// the number are printed as two words joined by `printf`, so the command text itself — which a part can print, as
/// `env` does with `AXH_COMMAND` — never holds a mark [`unmark`] would take out.
pub(crate) fn marked(chain: &Chain<'_>, mark: &str, builtin: &str) -> String {
    let mut out = String::new();
    for (k, part) in chain.parts.iter().enumerate() {
        if let Some(op) = k.checked_sub(1).and_then(|before| chain.ops.get(before)) {
            out.push_str(&format!(" {op} "));
        }
        out.push_str(&format!(
            "{{ {builtin}printf '%s-%s\\n' '{mark}' '{k}'\n{part}\n}}"
        ));
    }
    out
}

/// The output of a [`marked`] chain without its marks, and where each part that ran printed: `(part, range)`, part
/// `k` from its mark to the next one found (the first from the start). A part `||` or `&&` passed over has no mark and
/// no range. `None` for the places when the marks are out of order or the output lost its middle (its marks may have
/// gone with it) — then the output stays whole.
pub(crate) fn unmark(text: &str, mark: &str) -> (String, Option<Vec<Placed>>) {
    let prefix = format!("{mark}-");
    let mut out = String::with_capacity(text.len());
    let mut starts: Vec<(usize, usize)> = Vec::new();
    let mut in_order = true;
    let mut rest = text;
    while let Some(at) = rest.find(&prefix) {
        out.push_str(&rest[..at]);
        let after = &rest[at + prefix.len()..];
        let digits = after.bytes().take_while(u8::is_ascii_digit).count();
        match after[..digits].parse::<usize>() {
            Ok(k) if starts.last().is_none_or(|(last, _)| k > *last) => starts.push((k, out.len())),
            _ => in_order = false,
        }
        let after = &after[digits..];
        rest = after.strip_prefix('\n').unwrap_or(after);
    }
    out.push_str(rest);
    if !in_order || starts.is_empty() || out.contains(DROPPED_MIDDLE) {
        return (out, None);
    }
    let ranges = (0..starts.len())
        .map(|i| {
            let start = if i == 0 { 0 } else { starts[i].1 };
            let end = starts.get(i + 1).map_or(out.len(), |next| next.1);
            (starts[i].0, start..end)
        })
        .collect();
    (out, Some(ranges))
}

/// A part's number in its chain and where its output lies, in bytes.
pub(crate) type Placed = (usize, std::ops::Range<usize>);

/// What `bash`'s note says when a long output lost its middle (`shell.rs`, `Capture::text`): a chain's marks in that
/// middle are gone, so its parts cannot be told apart.
pub(crate) const DROPPED_MIDDLE: &str = "bytes of output in the middle were not kept";

/// Which output `words` produce, if one Glut knows.
fn kind_of(words: &[&str]) -> Option<Kind> {
    let has = |flags: &[&str]| {
        words.iter().any(|w| {
            flags
                .iter()
                .any(|f| w == f || w.starts_with(&format!("{f}=")))
        })
    };
    // A flag that carries its value glued on (`-U10`, `--unified=5`).
    let starts = |flags: &[&str]| words.iter().any(|w| flags.iter().any(|f| w.starts_with(f)));
    let program = words[0].rsplit('/').next().unwrap_or(words[0]);
    let args = &words[1..];
    match program {
        "git" => {
            // `git -C dir <sub>`: the subcommand is the first word that is no option or an option's value.
            let mut sub = None;
            let mut skip = false;
            for word in args {
                if skip {
                    skip = false;
                } else if *word == "-C" || *word == "-c" {
                    skip = true;
                } else if !word.starts_with('-') {
                    sub = Some(*word);
                    break;
                }
            }
            match sub? {
                "status"
                    if !has(&["--porcelain", "-s", "--short", "-z", "--ignored"])
                        && !starts(&["-s"]) =>
                {
                    Some(Kind::GitStatus)
                }
                "diff" | "show"
                    if !has(&[
                        "--stat",
                        "--numstat",
                        "--shortstat",
                        "--name-only",
                        "--name-status",
                        "--raw",
                        "--summary",
                        "--word-diff",
                        "--color-words",
                        "-W",
                        "--function-context",
                        "--cc",
                        "-c",
                    ]) && !starts(&["-U", "--unified", "--inter-hunk-context"])
                        // `git show rev:path` prints a file.
                        && !args.iter().any(|w| !w.starts_with('-') && w.contains(':')) =>
                {
                    Some(Kind::GitDiff)
                }
                "log"
                    if !has(&[
                        "--oneline",
                        "--format",
                        "--pretty",
                        "-p",
                        "--patch",
                        "--stat",
                        "--graph",
                        "--name-only",
                        "--name-status",
                        "--numstat",
                        "--shortstat",
                        "--raw",
                        "--summary",
                        "--decorate",
                        "--source",
                    ]) && !starts(&["-L", "--format", "--pretty"]) =>
                {
                    Some(Kind::GitLog)
                }
                _ => None,
            }
        }
        "cargo" => {
            let sub = args
                .iter()
                .find(|w| !w.starts_with('-') && !w.starts_with('+'))?;
            let known = matches!(
                *sub,
                "build" | "b" | "check" | "c" | "clippy" | "test" | "t" | "nextest" | "doc"
            );
            (known && !has(&["-q", "--quiet", "--message-format"])).then_some(Kind::Cargo)
        }
        "npm" | "pnpm" | "yarn" | "bun" => match args.first().copied()? {
            "install" | "i" | "ci" | "add" => Some(Kind::Install),
            "test" | "t" => Some(Kind::JsTests),
            "run" | "exec" | "dlx" | "x" => runner_kind(&args[1..]),
            other => runner_kind(&[other]),
        },
        "npx" | "bunx" => runner_kind(args),
        "vitest" | "jest" => Some(Kind::JsTests),
        "svelte-check" => Some(Kind::SvelteCheck),
        "pytest" | "py.test" => Some(Kind::Pytest),
        "python" | "python3" if args.len() >= 2 && args[0] == "-m" => match args[1] {
            "pytest" => Some(Kind::Pytest),
            "pip" if args.get(2) == Some(&"install") => Some(Kind::Install),
            _ => None,
        },
        "pip" | "pip3" if args.first() == Some(&"install") => Some(Kind::Install),
        "uv" | "poetry" | "pdm" | "hatch" => match args.first().copied()? {
            "sync" | "add" | "install" | "lock" => Some(Kind::Install),
            "pip" if args.get(1) == Some(&"install") => Some(Kind::Install),
            // `uv run [--with x …] pytest …`: the program run is the first word that is no option or its value.
            "run" => {
                let mut rest = args[1..].iter().copied();
                while let Some(word) = rest.next() {
                    if word.starts_with('-') {
                        if !word.contains('=')
                            && matches!(word, "--with" | "--python" | "-p" | "--env-file")
                        {
                            rest.next();
                        }
                        continue;
                    }
                    let remaining: Vec<&str> = std::iter::once(word).chain(rest).collect();
                    return kind_of(&remaining);
                }
                None
            }
            _ => None,
        },
        "swift" => matches!(args.first().copied()?, "build" | "test").then_some(Kind::Swift),
        "xcodebuild" => Some(Kind::Xcodebuild),
        "make" | "gmake" | "ninja" => Some(Kind::Make),
        "cmake" if args.first() == Some(&"--build") => Some(Kind::Make),
        // An action that prints something else than paths (`-exec cat {} +`, `fd -x`) is no listing.
        "find" | "fd"
            if !has(&[
                "-exec",
                "-execdir",
                "-ok",
                "-okdir",
                "-printf",
                "-fprintf",
                "-print0",
                "-ls",
                "-fls",
                "-x",
                "--exec",
                "-X",
                "--exec-batch",
                "-0",
                "--print0",
                "-l",
                "--list-details",
            ]) =>
        {
            Some(Kind::Listing)
        }
        "ls" if has(&["-R"]) || args.iter().any(|w| w.starts_with('-') && w.contains('R')) => {
            Some(Kind::Listing)
        }
        "ls" => Some(Kind::Listing),
        "tree" => Some(Kind::Tree),
        _ => None,
    }
}

/// `npx vitest`, `pnpm exec tsc`, `npm run test` …: the runner named.
fn runner_kind(args: &[&str]) -> Option<Kind> {
    let first = args.iter().find(|w| !w.starts_with('-'))?;
    match *first {
        "vitest" | "jest" | "test" => Some(Kind::JsTests),
        "svelte-check" | "check" => Some(Kind::SvelteCheck),
        _ => None,
    }
}

static ANSI: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07]*\x07").expect("valid regex")
});

/// The text without terminal colour and cursor codes: they carry nothing for the model.
fn strip_ansi(text: &str) -> String {
    ANSI.replace_all(text, "").into_owned()
}

/// The lines joined, runs of blank lines down to one, none at the start or the end; with `trim`, without the spaces
/// at the lines' ends.
fn squeeze_blank(lines: &[String], trim: bool) -> String {
    let mut out: Vec<&str> = Vec::with_capacity(lines.len());
    for line in lines {
        let blank = line.trim().is_empty();
        if blank && out.last().is_none_or(|last| last.trim().is_empty()) {
            continue;
        }
        out.push(if trim { line.trim_end() } else { line });
    }
    while out.last().is_some_and(|last| last.trim().is_empty()) {
        out.pop();
    }
    out.join("\n")
}

/// The lines of `text` that `drop` does not match.
fn keep_lines(text: &str, drop: impl Fn(&str) -> bool) -> Vec<String> {
    text.lines()
        .filter(|l| !drop(l))
        .map(str::to_owned)
        .collect()
}

/// A note Glut itself wrote into the output (`[exit code 1]`, a time-out, bytes not kept): never dropped or regrouped.
fn glut_note(line: &str) -> bool {
    line.starts_with('[') && line.ends_with(']') && !line.starts_with("[ ")
}

/// `git status` in its long form, as the short form says it: `## branch`, then `XY path` per entry.
fn git_status(text: &str) -> Vec<String> {
    #[derive(Clone, Copy)]
    enum Section {
        None,
        Staged,
        Unstaged,
        Untracked,
        Unmerged,
    }
    let mut section = Section::None;
    let mut out = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || (trimmed.starts_with("(use ") && trimmed.ends_with(')')) {
            continue;
        }
        if let Some(branch) = line.strip_prefix("On branch ") {
            out.push(format!("## {branch}"));
            continue;
        }
        let next = match trimmed {
            "Changes to be committed:" => Some(Section::Staged),
            "Changes not staged for commit:" => Some(Section::Unstaged),
            "Untracked files:" => Some(Section::Untracked),
            "Unmerged paths:" => Some(Section::Unmerged),
            _ => None,
        };
        if let Some(next) = next {
            section = next;
            continue;
        }
        if trimmed.starts_with("no changes added to commit") {
            continue;
        }
        let entry = line.starts_with('\t');
        let code = |state: &str| match state {
            "modified" => "M",
            "new file" => "A",
            "deleted" => "D",
            "renamed" => "R",
            "copied" => "C",
            "typechange" => "T",
            "both modified" => "UU",
            "both added" => "AA",
            "both deleted" => "DD",
            "added by us" => "AU",
            "added by them" => "UA",
            "deleted by us" => "DU",
            "deleted by them" => "UD",
            _ => "?",
        };
        match (section, entry) {
            (Section::Untracked, true) => out.push(format!("?? {trimmed}")),
            (Section::Staged | Section::Unstaged | Section::Unmerged, true) => {
                let (state, path) = trimmed
                    .split_once(':')
                    .map_or(("", trimmed), |(s, p)| (s, p.trim()));
                let code = code(state.trim());
                out.push(match section {
                    Section::Staged => format!("{code:<2} {path}"),
                    Section::Unmerged => format!("{code} {path}"),
                    _ => format!(" {code} {path}"),
                });
            }
            _ => out.push(line.to_owned()),
        }
    }
    out
}

/// How many unchanged lines a diff keeps before and after each change (git shows three).
const DIFF_CONTEXT: usize = 1;

/// A unified diff without its `index` and `---`/`+++` lines (the `diff --git` line names the files) and with one
/// unchanged line around each change instead of three; every changed line stays. A summary of the files with their
/// added and removed lines comes on top when there are several.
fn git_diff(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut summary: Vec<(String, usize, usize)> = Vec::new();
    let mut in_header = false;
    // Unchanged lines since the last change: the ones kept after it, and those held back in case a change follows.
    let mut kept_after = DIFF_CONTEXT;
    let mut held: Vec<&str> = Vec::new();
    for line in text.lines() {
        // `git show`'s commit header and message, before the first file, and Glut's own notes stay as they are.
        if (summary.is_empty() && !line.starts_with("diff --git ")) || glut_note(line) {
            out.push(line.to_owned());
            continue;
        }
        if let Some(files) = line.strip_prefix("diff --git ") {
            let path = files.rsplit(" b/").next().unwrap_or(files).to_owned();
            summary.push((path, 0, 0));
            in_header = true;
            held.clear();
            out.push(line.to_owned());
            continue;
        }
        if in_header {
            if line.starts_with("index ") || line.starts_with("--- ") || line.starts_with("+++ ") {
                continue;
            }
            if !line.starts_with("@@") {
                out.push(line.to_owned());
                continue;
            }
            in_header = false;
        }
        if line.starts_with("@@") {
            held.clear();
            kept_after = DIFF_CONTEXT;
            out.push(line.to_owned());
            continue;
        }
        let change = line.starts_with('+') || line.starts_with('-') || line.starts_with('\\');
        if change {
            let start = held.len().saturating_sub(DIFF_CONTEXT);
            out.extend(held.drain(..).skip(start).map(str::to_owned));
            if let Some(last) = summary.last_mut() {
                if line.starts_with('+') {
                    last.1 += 1;
                } else if line.starts_with('-') {
                    last.2 += 1;
                }
            }
            out.push(line.to_owned());
            kept_after = 0;
        } else if kept_after < DIFF_CONTEXT {
            out.push(line.to_owned());
            kept_after += 1;
        } else {
            held.push(line);
        }
    }
    if summary.len() > 1 {
        let mut head: Vec<String> = vec![format!("{} files changed:", summary.len())];
        head.extend(
            summary
                .iter()
                .map(|(path, plus, minus)| format!("  {path} +{plus} -{minus}")),
        );
        head.push(String::new());
        head.extend(out);
        return head;
    }
    out
}

/// How many characters of a commit's hash a compacted log shows: as many as `git log --oneline` on a large repo.
const SHORT_HASH: usize = 9;

/// `git log`'s default form as one line a commit: short hash, date, author, subject.
fn git_log(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut commit: Option<(String, String, String, Option<String>)> = None;
    let flush = |commit: &mut Option<(String, String, String, Option<String>)>,
                 out: &mut Vec<String>| {
        if let Some((hash, author, date, subject)) = commit.take() {
            // The hash is the line's first word; ref names after it (`(HEAD -> main)`) stay.
            let (full, refs) = hash.split_once(' ').unwrap_or((hash.as_str(), ""));
            let short: String = full.chars().take(SHORT_HASH).collect();
            let refs = if refs.is_empty() {
                String::new()
            } else {
                format!(" {refs}")
            };
            out.push(format!(
                "{short}{refs} {date} {author}: {}",
                subject.unwrap_or_default()
            ));
        }
    };
    for line in text.lines() {
        if glut_note(line) {
            flush(&mut commit, &mut out);
            out.push(line.to_owned());
            continue;
        }
        if let Some(rest) = line.strip_prefix("commit ") {
            flush(&mut commit, &mut out);
            commit = Some((rest.to_owned(), String::new(), String::new(), None));
        } else if let Some((_, author, date, subject)) = commit.as_mut() {
            if let Some(name) = line.strip_prefix("Author:") {
                *author = name.split('<').next().unwrap_or(name).trim().to_owned();
            } else if let Some(when) = line.strip_prefix("Date:") {
                *date = when.trim().to_owned();
            } else if subject.is_none() && line.starts_with("    ") && !line.trim().is_empty() {
                *subject = Some(line.trim().to_owned());
            }
        } else {
            out.push(line.to_owned());
        }
    }
    flush(&mut commit, &mut out);
    out
}

static CARGO_PROGRESS: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^\s+(Compiling|Checking|Downloaded|Downloading|Updating|Locking|Adding|Fresh|Blocking|Documenting|",
        r"Packaging|Unpacking|Removing)\s"
    ))
    .expect("valid regex")
});
static CARGO_PASSED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^test .+ \.\.\. (ok|ignored)$").expect("valid regex"));

/// Cargo without its progress, the passing tests' lines and the suites that ran no test.
fn cargo(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text
        .lines()
        .filter(|l| !CARGO_PROGRESS.is_match(l) && !CARGO_PASSED.is_match(l))
        .collect();
    // An empty suite is its `Running …`/`Doc-tests …` head, `running 0 tests`, a blank line and its zero result.
    let mut out = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        let head = line.trim_start().starts_with("Running ")
            || line.trim_start().starts_with("Doc-tests ");
        if head {
            let block: Vec<&str> = lines[index + 1..]
                .iter()
                .copied()
                .take_while(|l| {
                    !l.trim_start().starts_with("Running ")
                        && !l.trim_start().starts_with("Doc-tests ")
                })
                .collect();
            let empty = block.iter().any(|l| l.trim() == "running 0 tests")
                && block
                    .iter()
                    .any(|l| l.starts_with("test result: ok. 0 passed"));
            let only_that = block.iter().all(|l| {
                l.trim().is_empty()
                    || l.trim() == "running 0 tests"
                    || l.starts_with("test result: ok. 0 passed")
            });
            if empty && only_that {
                index += 1 + block.len();
                continue;
            }
        }
        out.push(line.to_owned());
        index += 1;
    }
    out
}

/// A package manager without download and progress lines; warnings, errors and the summary stay.
fn install(text: &str) -> Vec<String> {
    keep_lines(text, |line| {
        let t = line.trim_start();
        t.starts_with("Collecting ")
            || t.starts_with("Downloading ")
            || t.starts_with("Downloaded ")
            || t.starts_with("Using cached ")
            || t.starts_with("Requirement already satisfied: ")
            || t.starts_with("Obtaining ")
            || t.starts_with("Preparing metadata")
            || t.starts_with("Building wheel")
            || t.starts_with("Created wheel")
            || t.starts_with("Stored in directory")
            || t.starts_with("Progress: resolved")
            || t.starts_with("npm http ")
            || t.starts_with("npm timing ")
            || t.starts_with("npm sill ")
            || t.starts_with("npm verb ")
            || t.contains('━')
            || t.contains("|█")
            || (!t.is_empty() && t.chars().all(|c| c == '+' || c == '-'))
    })
}

static JS_PASSED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^\s*(✓|√|PASS)\s").expect("valid regex"));

/// A JavaScript test run without its passing tests and files.
fn js_tests(text: &str) -> Vec<String> {
    keep_lines(text, |line| JS_PASSED.is_match(line))
}

/// `svelte-check` without its progress lines.
fn svelte_check(text: &str) -> Vec<String> {
    keep_lines(text, |line| {
        let t = line.trim();
        t.starts_with("Loading svelte-check")
            || t.starts_with("Getting Svelte diagnostics")
            || (!t.is_empty() && t.chars().all(|c| c == '='))
    })
}

static PYTEST_PASSED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"::\S+ PASSED\b").expect("valid regex"));

/// pytest without its session header and the passing tests' lines (verbose form).
fn pytest(text: &str) -> Vec<String> {
    keep_lines(text, |line| {
        let t = line.trim_start();
        PYTEST_PASSED.is_match(line)
            || t.starts_with("platform ")
            || t.starts_with("cachedir:")
            || t.starts_with("rootdir:")
            || t.starts_with("configfile:")
            || t.starts_with("plugins:")
            || t.starts_with("Installed ") && t.contains(" packages in ")
    })
}

static SWIFT_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(\[\d+\s*/\s*\d+\]|\[Planning deferred tasks\]|Building for (debugging|production)|Compiling |Write |",
        r"Emitting module |Test Case '.*' (started\.|passed \()|Test Suite '.*' started at |◇ Test .* started\.|",
        r"✔ Test .* passed after |↳ Testing Library Version|↳ Target Platform|◇ Test run started)"
    ))
    .expect("valid regex")
});

/// A line after "Failed frontend command:" longer than this is the compiler's whole invocation, not a message.
const INVOCATION_CHARS: usize = 300;

/// Swift's build and test output without progress, started and passing tests and the compiler's own command lines.
fn swift(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut lines = text.lines().peekable();
    while let Some(line) = lines.next() {
        if line.starts_with("Failed frontend command:") {
            // The next line is the compiler's whole invocation, often tens of thousands of characters.
            if lines
                .peek()
                .is_some_and(|next| next.len() > INVOCATION_CHARS)
            {
                lines.next();
            }
            continue;
        }
        if SWIFT_NOISE.is_match(line.trim_start()) {
            continue;
        }
        out.push(line.to_owned());
    }
    out
}

static XCODE_STEP: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(CompileC|CompileSwift|CompileSwiftSources|SwiftCompile|SwiftDriver|SwiftEmitModule|",
        r"SwiftMergeGeneratedHeaders|",
        r"SwiftExplicitDependency\w*|ScanDependencies|Ld|ProcessInfoPlistFile|CodeSign|CpResource|CopySwiftLibs|Copy|",
        r"Touch|RegisterExecutionPolicyException|WriteAuxiliaryFile|MkDir|ProcessProductPackaging\w*|Validate\w*|",
        r"GenerateAssetSymbols|CompileAssetCatalog\w*|LinkStoryboards|ExtractAppIntentsMetadata|PhaseScriptExecution|",
        r"CreateBuildDirectory|ComputeTargetDependencyGraph|ClangStatCache|GatherProvisioningInputs|",
        r"CreateUniversalBinary|ProcessSDKImports|Builtin-\w+|SymLink|Strip|GenerateDSYMFile|AppIntentsSSUTraining)\s"
    ))
    .expect("valid regex")
});

/// `xcodebuild` without its build steps and their indented command lines, and without passing tests.
fn xcodebuild(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut in_step = false;
    for line in text.lines() {
        if XCODE_STEP.is_match(line) {
            in_step = true;
            continue;
        }
        if in_step && (line.starts_with("    ") || line.trim().is_empty()) {
            continue;
        }
        in_step = false;
        if SWIFT_NOISE.is_match(line.trim_start()) {
            continue;
        }
        out.push(line.to_owned());
    }
    out
}

static MAKE_NOISE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(g?make(\[\d+\])?: (Entering|Leaving) directory|\[\s*\d+%\]\s+(Building|Linking|Built target|Scanning|",
        r"Generating|Automatic)|\[\d+/\d+\]\s)"
    ))
    .expect("valid regex")
});

/// `make`, `cmake --build` and `ninja` without directory changes and progress lines; compiler diagnostics stay.
fn make(text: &str) -> Vec<String> {
    keep_lines(text, |line| MAKE_NOISE.is_match(line))
}

/// A listing (`find`, `fd`, `ls`): short ones whole; a long one by folder, a big folder with its first entries and a
/// count. `ls -l` loses its permission, owner and date columns there.
fn listing(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= LISTING_LINES {
        return lines.iter().map(|l| (*l).to_owned()).collect();
    }
    static LONG_LS: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(
            r"^([dlcbps-])[rwxsStT@+.-]{9,}\s+\d+\s+\S+\s+\S+\s+(\d+)\s+\w+\s+\d+\s+[\d:]+\s+(.+)$",
        )
        .expect("valid regex")
    });
    let mut folders: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    let mut current = String::from(".");
    let mut out_of_band = Vec::new();
    for line in lines {
        if glut_note(line) || line.starts_with("total ") {
            if glut_note(line) {
                out_of_band.push(line.to_owned());
            }
            continue;
        }
        // `ls -R` names each folder on a line of its own, ending in a colon.
        if let Some(folder) = line
            .strip_suffix(':')
            .filter(|f| !f.contains(' ') || f.starts_with('.'))
        {
            current = folder.to_owned();
            continue;
        }
        if line.trim().is_empty() {
            continue;
        }
        let (folder, entry) = if let Some(caps) = LONG_LS.captures(line) {
            let kind = if &caps[1] == "d" { "/" } else { "" };
            (
                current.clone(),
                format!("{}{kind} ({} B)", &caps[3], &caps[2]),
            )
        } else if line.contains('/') {
            let (folder, name) = line
                .trim_end_matches('/')
                .rsplit_once('/')
                .unwrap_or((".", line));
            (folder.to_owned(), name.to_owned())
        } else {
            (current.clone(), line.trim().to_owned())
        };
        if !folders.contains_key(&folder) {
            order.push(folder.clone());
        }
        folders.entry(folder).or_default().push(entry);
    }
    let mut out = Vec::new();
    for folder in order {
        let entries = &folders[&folder];
        if entries.len() > FOLDER_ENTRIES {
            let shown = entries[..FOLDER_ENTRIES].join(", ");
            out.push(format!(
                "{folder}/ ({} entries): {shown}, … {} more",
                entries.len(),
                entries.len() - FOLDER_ENTRIES
            ));
        } else {
            out.push(format!("{folder}/: {}", entries.join(", ")));
        }
    }
    out.extend(out_of_band);
    out
}

/// `tree`: a long one with each big folder's first entries and a count of the rest.
fn tree(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= LISTING_LINES {
        return lines.iter().map(|l| (*l).to_owned()).collect();
    }
    // An entry's depth is where its name starts; siblings share it. Past the first entries of one folder at one depth,
    // the rest (and everything under them) is counted instead of shown.
    let depth = |line: &str| {
        line.find(|c: char| c.is_alphanumeric() || c == '.' || c == '_')
            .unwrap_or(0)
    };
    let mut out = Vec::new();
    let mut shown_at: BTreeMap<usize, usize> = BTreeMap::new();
    let mut hidden_at: BTreeMap<usize, usize> = BTreeMap::new();
    for line in lines {
        let here = depth(line);
        // Leaving deeper levels: their counters end, and what was hidden there is said.
        let deeper: Vec<usize> = shown_at.range(here + 1..).map(|(d, _)| *d).collect();
        for d in deeper.into_iter().rev() {
            shown_at.remove(&d);
            if let Some(hidden) = hidden_at.remove(&d).filter(|h| *h > 0) {
                out.push(format!("{}… {hidden} more", " ".repeat(d)));
            }
        }
        let hiding_above = hidden_at.range(..here).any(|(_, hidden)| *hidden > 0);
        if hiding_above {
            continue;
        }
        let count = shown_at.entry(here).or_default();
        *count += 1;
        if *count > FOLDER_ENTRIES && !glut_note(line) {
            *hidden_at.entry(here).or_default() += 1;
            continue;
        }
        out.push(line.to_owned());
    }
    for (d, hidden) in hidden_at.into_iter().rev() {
        if hidden > 0 {
            out.push(format!("{}… {hidden} more", " ".repeat(d)));
        }
    }
    out
}

#[cfg(test)]
mod tests;
