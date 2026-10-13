//! What a shell command does, as far as rules can tell (`docs/plans/harness.md`, P2).
//!
//! A small lexer in the shell's own terms — quotes, escapes, operators, redirections — that is deliberately unsure:
//! whatever it cannot vouch for makes the command *opaque*. That is a comment, a here-document, a substitution or any
//! `$` expansion, grouping, a keyword, a command word that is not literal (`=rm`, a glob, `~`), a builtin that runs or
//! redefines other commands (`eval`, `source`, `alias`, `trap`), a shell reading a script, or git's own configuration.
//! An opaque command is never allowed by a rule and never by the `auto` mode; it is asked about. Deny rules are still
//! checked against everything the lexer did read, against every word sequence after a wrapper (`env`, `sudo`,
//! `xargs`, `noglob`), and inside `sh -c` strings. Best effort until the sandbox of HS3b: a command can always do more
//! than its words say (two security reviews, 2026-10-13, shaped this list).

use globset::GlobBuilder;

/// Path components a command must not touch without the owner's say: version-control stores, the configuration a later
/// session would obey, and files that run code when the owner next builds or enters the folder. Compared without case:
/// macOS volumes do not tell `.git` from `.GIT`.
pub const PROTECTED_NAMES: [&str; 14] = [
    ".git",
    ".hg",
    ".jj",
    ".axh",
    ".axiomata",
    ".claude",
    ".opencode",
    ".vscode",
    ".mcp.json",
    "opencode.json",
    "trusted.json",
    ".cargo",
    ".envrc",
    ".husky",
];

/// Words a command starts with that only run the command after them: stripped before deny rules are matched — and,
/// because their options are many and differ by system, every word sequence after one is a candidate as well.
const WRAPPERS: [&str; 21] = [
    "command",
    "builtin",
    "env",
    "sudo",
    "doas",
    "nohup",
    "time",
    "nice",
    "exec",
    "timeout",
    "xargs",
    "noglob",
    "nocorrect",
    "-",
    "repeat",
    "setsid",
    "stdbuf",
    "caffeinate",
    "arch",
    "watch",
    "unbuffer",
];

/// Builtins that run, define or redefine commands the words do not show: the command is opaque.
const RUNS_OTHERS: [&str; 21] = [
    "eval",
    "source",
    ".",
    "trap",
    "alias",
    "unalias",
    "hash",
    "setopt",
    "unsetopt",
    "emulate",
    "enable",
    "disable",
    "fc",
    "autoload",
    "functions",
    "export",
    "declare",
    "typeset",
    "readonly",
    "local",
    "set",
];

/// Shells: with `-c` their argument is a command of its own, read again; without, they run a script or their input.
const SHELLS: [&str; 6] = ["sh", "bash", "zsh", "dash", "ksh", "fish"];

/// Reserved words: what follows them is a compound command the lexer does not follow.
const KEYWORDS: [&str; 20] = [
    "if", "then", "else", "elif", "fi", "for", "foreach", "while", "until", "do", "done", "case",
    "esac", "function", "select", "coproc", "always", "[[", "]]", "!",
];

/// Git settings that make git run a program: written or set by a command, the command is opaque.
const GIT_RUNS: [&str; 6] = [
    "hookspath",
    "fsmonitor",
    "sshcommand",
    "pager",
    "editor",
    "alias.",
];

/// Git's options that come before the sub-command and take a value.
const GIT_GLOBAL_WITH_VALUE: [&str; 5] = ["-C", "-c", "--git-dir", "--work-tree", "--namespace"];

/// How deep `sh -c` strings are followed.
const MAX_NESTING: usize = 3;

/// Command names compare without case where the file system does: on macOS `RM` finds `/bin/rm`.
pub const CASE_INSENSITIVE_COMMANDS: bool = cfg!(target_os = "macos");

/// Whether two command names are the same command.
pub fn same_command(a: &str, b: &str) -> bool {
    if CASE_INSENSITIVE_COMMANDS {
        a.eq_ignore_ascii_case(b)
    } else {
        a == b
    }
}

/// One simple command: its leading `NAME=value` assignments and its words, quotes and escapes resolved.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Part {
    pub assignments: Vec<String>,
    pub words: Vec<String>,
}

/// What the lexer found.
#[derive(Debug, Clone, Default)]
pub struct Analysis {
    pub parts: Vec<Part>,
    /// Why the command is opaque; empty when every bit of it was read.
    pub opaque: Vec<&'static str>,
    /// Targets of output redirections (`>`, `>>`, `&>`, `>|`, `<>`), `/dev/null` left out.
    pub writes: Vec<String>,
    /// Files read by an input redirection (`< file`).
    pub reads: Vec<String>,
    /// A word or redirection names a protected path component, literally or through a glob.
    pub protected: bool,
    /// Commands given to a shell with `-c`, read in turn.
    pub nested: Vec<Analysis>,
}

impl Analysis {
    pub fn is_opaque(&self) -> bool {
        !self.opaque.is_empty()
    }

    /// Every word list a deny or ask rule is held against: each part as written (assignments dropped), with its
    /// wrappers stripped, every sequence after a wrapper, the command word without its folder, git without its global
    /// options, and the same for every nested command. Over-matching here only turns a call into an ask or a refusal.
    pub fn deny_candidates(&self) -> Vec<Vec<String>> {
        let mut out: Vec<Vec<String>> = Vec::new();
        let mut add = |words: Vec<String>| {
            if !words.is_empty() && !out.contains(&words) {
                out.push(words);
            }
        };
        for part in &self.parts {
            if part.words.is_empty() {
                continue;
            }
            add(part.words.clone());
            add(with_base(part.words.clone()));
            if is_wrapper(&part.words[0]) {
                for start in 1..part.words.len() {
                    add(with_base(part.words[start..].to_vec()));
                }
            }
            let core = strip_wrappers(&part.words);
            if let Some(git) = without_git_globals(&core) {
                add(git);
            }
            add(core);
        }
        for nested in &self.nested {
            for words in nested.deny_candidates() {
                add(words);
            }
        }
        out
    }
}

/// Reads `command`.
pub fn analyze(command: &str) -> Analysis {
    analyze_at(command, 0)
}

fn analyze_at(command: &str, depth: usize) -> Analysis {
    let mut lexer = Lexer::new(command);
    lexer.run();
    let globbed = std::mem::take(&mut lexer.globbed);
    let mut analysis = lexer.analysis;
    for part in &mut analysis.parts {
        let split = part
            .words
            .iter()
            .take_while(|word| is_assignment(word))
            .count();
        part.assignments = part.words.drain(..split).collect();
    }
    let mut reasons: Vec<&'static str> = Vec::new();
    let mut nested_texts = Vec::new();
    for part in &analysis.parts {
        let Some(first) = part.words.first() else {
            if !part.assignments.is_empty() {
                // `X=rm` on its own sets a variable a later part may run as a command.
                reasons.push("a variable set for a later part");
            }
            continue;
        };
        if KEYWORDS.iter().any(|keyword| same_command(keyword, first)) {
            reasons.push("a compound command (if, for, while, …)");
        }
        let core = strip_wrappers(&part.words);
        let Some(command) = core.first() else {
            continue;
        };
        if RUNS_OTHERS.iter().any(|name| same_command(name, command)) {
            reasons.push(
                "a builtin that runs or defines other commands (eval, source, alias, trap, …)",
            );
        }
        if SHELLS.iter().any(|name| same_command(name, command)) {
            match shell_argument(&core) {
                Some(text) => {
                    reasons.push("a command handed to a shell with -c");
                    nested_texts.push(text);
                }
                None => reasons.push("a shell running a script or its input"),
            }
        }
        if same_command(command, "git") && touches_git_config(&core) {
            reasons.push("git's own configuration, which can make git run programs");
        }
    }
    analysis.opaque.extend(reasons);
    analysis.protected |= analysis
        .parts
        .iter()
        .flat_map(|part| part.words.iter().chain(part.assignments.iter()))
        .chain(analysis.writes.iter())
        .chain(analysis.reads.iter())
        .any(|word| names_protected(word))
        || globbed.iter().any(|word| glob_may_name_protected(word));
    if depth < MAX_NESTING {
        for text in nested_texts {
            let nested = analyze_at(&text, depth + 1);
            analysis.protected |= nested.protected;
            analysis.nested.push(nested);
        }
    }
    analysis.opaque.sort_unstable();
    analysis.opaque.dedup();
    analysis
}

/// Whether `path` has a component that is one of [`PROTECTED_NAMES`], whatever its case. Any character a shell or a
/// tool could take as a separator (`/`, `=`, `:`, `,`, …) separates components here.
pub fn names_protected(path: &str) -> bool {
    path.split(|c: char| !(c.is_alphanumeric() || matches!(c, '.' | '_' | '-')))
        .any(|component| {
            PROTECTED_NAMES
                .iter()
                .any(|name| component.eq_ignore_ascii_case(name))
        })
}

/// Whether a word with unquoted glob characters could expand to a protected name (`.ax?`, `.[g]it`, `.gi*`).
fn glob_may_name_protected(word: &str) -> bool {
    word.split('/')
        .filter(|component| component.contains(['*', '?', '[']))
        .any(|component| {
            GlobBuilder::new(component)
                .case_insensitive(true)
                .build()
                .map(|glob| glob.compile_matcher())
                // A component that is no valid glob is not expanded by the shell either.
                .is_ok_and(|matcher| PROTECTED_NAMES.iter().any(|name| matcher.is_match(name)))
        })
}

/// `NAME=value`, `NAME+=value`, `NAME[index]=value`: an assignment in front of a command.
fn is_assignment(word: &str) -> bool {
    let Some((name, _)) = word.split_once('=') else {
        return false;
    };
    let name = name.strip_suffix('+').unwrap_or(name);
    let name = match name.split_once('[') {
        Some((base, index)) if index.ends_with(']') => base,
        Some(_) => return false,
        None => name,
    };
    !name.is_empty()
        && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        && !name.starts_with(|c: char| c.is_ascii_digit())
}

fn is_wrapper(word: &str) -> bool {
    WRAPPERS
        .iter()
        .any(|wrapper| same_command(wrapper, base(word)))
}

/// The command word without its folder (`/bin/rm` → `rm`).
fn base(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

fn with_base(mut words: Vec<String>) -> Vec<String> {
    if let Some(first) = words.first_mut() {
        *first = base(first).to_owned();
    }
    words
}

/// The options of a wrapper that take the next word as their value.
fn options_with_values(wrapper: &str) -> &'static [&'static str] {
    match wrapper.to_ascii_lowercase().as_str() {
        "sudo" | "doas" => &["-u", "-g", "-h", "-p", "-C", "-U", "-r", "-t", "-D"],
        "env" => &["-u", "-C", "-S", "-P"],
        "nice" => &["-n"],
        "timeout" => &["-s", "-k", "--signal", "--kill-after"],
        "xargs" => &[
            "-I", "-J", "-L", "-R", "-S", "-n", "-P", "-s", "-E", "-d", "-a",
        ],
        "exec" => &["-a"],
        "arch" => &["-arch"],
        "stdbuf" => &["-i", "-o", "-e"],
        _ => &[],
    }
}

/// `words` without the wrappers in front and with the command word's folder dropped (`sudo -u x /bin/rm -rf a` →
/// `rm -rf a`).
fn strip_wrappers(words: &[String]) -> Vec<String> {
    let mut rest: &[String] = words;
    while let Some(first) = rest.first() {
        if !is_wrapper(first) {
            break;
        }
        let name = base(first);
        rest = &rest[1..];
        // The wrapper's own options and settings: `env -i A=1`, `nice -n 5`, `timeout 10`, `repeat 3`, and the
        // options that take the next word as their value (`sudo -u root`, `xargs -I {}`).
        let takes_value = options_with_values(name);
        while let Some(word) = rest.first() {
            let numeric = !word.is_empty() && word.chars().all(|c| c.is_ascii_digit() || c == '.');
            if takes_value.contains(&word.as_str()) {
                rest = rest.get(2..).unwrap_or(&[]);
            } else if word.starts_with('-') || is_assignment(word) || numeric {
                rest = &rest[1..];
            } else {
                break;
            }
        }
    }
    with_base(rest.to_vec())
}

/// `git` with its global options removed (`git -C . --no-pager push` → `git push`); `None` for anything else.
fn without_git_globals(words: &[String]) -> Option<Vec<String>> {
    let (first, rest) = words.split_first()?;
    if !same_command(first, "git") {
        return None;
    }
    let mut at = 0;
    while let Some(word) = rest.get(at) {
        if GIT_GLOBAL_WITH_VALUE.contains(&word.as_str()) {
            at += 2;
        } else if word.starts_with('-') {
            at += 1;
        } else {
            break;
        }
    }
    let mut out = vec!["git".to_owned()];
    out.extend(rest.iter().skip(at).cloned());
    Some(out)
}

/// Whether a git call reads or writes a setting that runs a program: `git config`, or a `-c` naming one.
fn touches_git_config(words: &[String]) -> bool {
    let core = without_git_globals(words).unwrap_or_default();
    core.get(1)
        .is_some_and(|sub| sub.eq_ignore_ascii_case("config"))
        || words.iter().any(|word| {
            let lower = word.to_ascii_lowercase();
            GIT_RUNS.iter().any(|setting| lower.contains(setting))
                && (lower.contains("core.") || lower.contains("alias."))
        })
}

/// The string a shell is told to run with `-c`, if `words` (wrappers stripped) are such a call. Only a short option
/// cluster counts (`-c`, `-lc`), never a long option that happens to contain a `c` (`--norc`).
fn shell_argument(core: &[String]) -> Option<String> {
    let at = core.iter().skip(1).position(|word| {
        word.len() > 1
            && word.starts_with('-')
            && !word.starts_with("--")
            && word[1..].chars().all(|c| c.is_ascii_alphabetic())
            && word.contains('c')
    })?;
    core.get(at + 2).cloned()
}

struct Lexer<'a> {
    chars: std::iter::Peekable<std::str::Chars<'a>>,
    analysis: Analysis,
    part: Vec<String>,
    word: String,
    /// The current word has begun (an empty `''` is a word too).
    started: bool,
    /// The current word holds an unquoted glob character.
    glob: bool,
    /// Words with unquoted glob characters, for the protected-name check.
    globbed: Vec<String>,
    /// The next word is the target of an output redirection.
    redirect: bool,
    /// The next word is the file of an input redirection.
    reading: bool,
    /// The current part has a redirection, so a part without words is a command of its own (zsh's `NULLCMD`).
    part_redirected: bool,
}

impl<'a> Lexer<'a> {
    fn new(command: &'a str) -> Self {
        Lexer {
            chars: command.chars().peekable(),
            analysis: Analysis::default(),
            part: Vec::new(),
            word: String::new(),
            started: false,
            glob: false,
            globbed: Vec::new(),
            redirect: false,
            reading: false,
            part_redirected: false,
        }
    }

    fn opaque(&mut self, why: &'static str) {
        self.analysis.opaque.push(why);
    }

    fn end_word(&mut self) {
        if !self.started {
            return;
        }
        let word = std::mem::take(&mut self.word);
        self.started = false;
        if std::mem::take(&mut self.glob) {
            // A command word that is a glob runs whatever it expands to.
            if self.part.is_empty() && !self.redirect && !self.reading {
                self.opaque("a command word that is a glob");
            }
            self.globbed.push(word.clone());
        }
        if self.redirect {
            self.redirect = false;
            if word != "/dev/null" {
                self.analysis.writes.push(word);
            }
        } else if self.reading {
            self.reading = false;
            self.analysis.reads.push(word);
        } else {
            self.part.push(word);
        }
    }

    fn end_part(&mut self) {
        self.end_word();
        if self.redirect || self.reading {
            self.redirect = false;
            self.reading = false;
            self.opaque("a redirection without a target");
        }
        let words = std::mem::take(&mut self.part);
        if words.is_empty() && std::mem::take(&mut self.part_redirected) {
            self.opaque("a redirection without a command");
        }
        self.part_redirected = false;
        if !words.is_empty() {
            self.analysis.parts.push(Part {
                assignments: Vec::new(),
                words,
            });
        }
    }

    fn run(&mut self) {
        while let Some(c) = self.chars.next() {
            match c {
                ' ' | '\t' => self.end_word(),
                // A carriage return is an ordinary character to the shell, a separator to a reader's eye.
                '\r' => self.opaque("a carriage return"),
                '\n' | ';' => self.end_part(),
                '\\' => match self.chars.next() {
                    // A line continuation joins the lines.
                    Some('\n') => {}
                    Some(next) => self.push(next),
                    None => self.opaque("a trailing backslash"),
                },
                '\'' => self.single_quoted(),
                '"' => self.double_quoted(),
                '`' => self.opaque("a command substitution"),
                '$' => self.dollar(),
                '#' if !self.started => {
                    self.opaque("a comment");
                    return;
                }
                '(' | ')' | '{' | '}' => self.opaque("grouping, a subshell or brace expansion"),
                '!' if !self.started => self.opaque("a negated or history-expanded command"),
                // zsh's `=cmd` expands to a command's path; `=(…)` is a process substitution.
                '=' if !self.started => self.opaque("zsh's =command expansion"),
                '~' if !self.started && self.part.is_empty() && !self.redirect && !self.reading => {
                    self.opaque("a command word starting with ~");
                    self.push('~');
                }
                '*' | '?' | '[' => {
                    self.glob = true;
                    self.push(c);
                }
                '|' => {
                    // `||` and `|&` are one operator.
                    if matches!(self.chars.peek(), Some('|') | Some('&')) {
                        self.chars.next();
                    }
                    self.end_part();
                }
                '&' => match self.chars.peek() {
                    Some('&') => {
                        self.chars.next();
                        self.end_part();
                    }
                    // `&>` and `&>>`: both streams into a file.
                    Some('>') => {
                        self.chars.next();
                        if self.chars.peek() == Some(&'>') {
                            self.chars.next();
                        }
                        self.start_redirect();
                    }
                    _ => self.end_part(),
                },
                '>' | '<' => self.redirection(c),
                other => self.push(other),
            }
        }
        self.end_part();
    }

    fn push(&mut self, c: char) {
        self.word.push(c);
        self.started = true;
    }

    fn single_quoted(&mut self) {
        self.started = true;
        for c in self.chars.by_ref() {
            if c == '\'' {
                return;
            }
            self.word.push(c);
        }
        self.opaque("an unclosed quote");
    }

    fn double_quoted(&mut self) {
        self.started = true;
        while let Some(c) = self.chars.next() {
            match c {
                '"' => return,
                '\\' => match self.chars.next() {
                    Some(next) => self.word.push(next),
                    None => break,
                },
                '`' => self.opaque("a command substitution"),
                '$' if self.chars.peek().is_some_and(|&next| expands(next)) => {
                    self.opaque("a variable or other expansion");
                    self.word.push(c);
                }
                other => self.word.push(other),
            }
        }
        self.opaque("an unclosed quote");
    }

    /// `$` followed by a name, a digit, a special parameter or a bracket expands; on its own it is a plain `$`.
    fn dollar(&mut self) {
        match self.chars.peek() {
            Some('\'') => self.opaque("an ANSI-C quoted string"),
            Some('"') => self.opaque("a locale-translated string"),
            Some(&next) if expands(next) => self.opaque("a variable or other expansion"),
            _ => {}
        }
        self.push('$');
    }

    fn start_redirect(&mut self) {
        self.end_word();
        self.redirect = true;
        self.part_redirected = true;
    }

    /// `>`, `>>`, `>|`, `<`, `<>`, `<<`, a descriptor in front (`2>`), and duplications (`2>&1`, `>&2`, `<&-`), which
    /// write no file.
    fn redirection(&mut self, c: char) {
        // A word of digits right before the operator is the descriptor it applies to, not an argument.
        if self.started && self.word.chars().all(|d| d.is_ascii_digit()) {
            self.word.clear();
            self.started = false;
        } else {
            self.end_word();
        }
        self.part_redirected = true;
        if self.chars.peek() == Some(&'(') {
            self.opaque("a process substitution");
            return;
        }
        if c == '<' {
            match self.chars.peek() {
                Some('<') => {
                    self.opaque("a here-document or here-string");
                    self.chars.next();
                }
                Some('>') => {
                    self.chars.next();
                    self.start_redirect();
                }
                Some('&') => {
                    self.chars.next();
                    self.duplication();
                }
                // Input from a file: read, not written.
                _ => {
                    self.end_word();
                    self.reading = true;
                }
            }
            return;
        }
        match self.chars.peek() {
            Some('>') | Some('|') => {
                self.chars.next();
                self.start_redirect();
            }
            Some('&') => {
                self.chars.next();
                self.duplication();
            }
            _ => self.start_redirect(),
        }
    }

    /// After `>&` or `<&`: a descriptor or `-` duplicates or closes; anything else is a file (`>&file`, `>&2foo`).
    fn duplication(&mut self) {
        let mut target = String::new();
        while let Some(&c) = self.chars.peek() {
            if c.is_ascii_digit() || c == '-' {
                target.push(c);
                self.chars.next();
            } else {
                break;
            }
        }
        let ends = self
            .chars
            .peek()
            .is_none_or(|&c| c.is_whitespace() || matches!(c, ';' | '|' | '&' | '<' | '>'));
        if target.is_empty() || !ends {
            self.start_redirect();
            for c in target.chars() {
                self.push(c);
            }
        }
    }
}

/// Whether `$` followed by `next` expands something.
fn expands(next: char) -> bool {
    next.is_ascii_alphanumeric()
        || matches!(
            next,
            '_' | '{' | '(' | '[' | '?' | '!' | '#' | '*' | '@' | '-' | '$'
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn words(command: &str) -> Vec<Vec<String>> {
        analyze(command)
            .parts
            .into_iter()
            .map(|part| part.words)
            .collect()
    }

    fn denied_as(command: &str, wanted: &str) -> bool {
        analyze(command)
            .deny_candidates()
            .iter()
            .any(|words| words.first().is_some_and(|word| same_command(word, wanted)))
    }

    #[test]
    fn quotes_escapes_and_operators_are_read_as_the_shell_reads_them() {
        assert_eq!(
            words("cargo test -- 'a;b && c'"),
            [["cargo", "test", "--", "a;b && c"]]
        );
        assert_eq!(words("r\"\"m -rf x"), [["rm", "-rf", "x"]]);
        assert_eq!(words("\\rm x"), [["rm", "x"]]);
        assert_eq!(
            words("a && b || c; d | e |& f & g\nh"),
            [["a"], ["b"], ["c"], ["d"], ["e"], ["f"], ["g"], ["h"]]
        );
        assert_eq!(words("git\t push   origin"), [["git", "push", "origin"]]);
        assert_eq!(words("rm \\\n-rf x"), [["rm", "-rf", "x"]]);
    }

    #[test]
    fn descriptor_duplication_is_no_file_and_no_separator() {
        let analysis = analyze("cargo test 2>&1 | tee out.log");
        assert_eq!(words("cargo test 2>&1"), [["cargo", "test"]]);
        assert!(!analysis.is_opaque());
        assert!(analysis.writes.is_empty());
        assert!(analyze("echo x >&2").writes.is_empty());
        assert!(analyze("cmd 2>/dev/null").writes.is_empty());
        assert_eq!(
            analyze("echo x >&2foo").writes,
            ["2foo"],
            "digits followed by a name are a file"
        );
    }

    #[test]
    fn redirections_name_what_they_write() {
        assert_eq!(analyze("echo x > ~/.zshrc").writes, ["~/.zshrc"]);
        assert_eq!(analyze("cat a >> notes.md").writes, ["notes.md"]);
        assert_eq!(analyze("cmd &> log").writes, ["log"]);
        assert_eq!(analyze("cmd >| f").writes, ["f"]);
        assert!(analyze("echo x > .axh/settings.json").protected);
        let input = analyze("cat < input.txt");
        assert!(input.writes.is_empty(), "input is read, not written");
        assert_eq!(input.reads, ["input.txt"]);
        assert_eq!(words("cat < input.txt"), [["cat"]]);
        assert!(analyze("wc -l < .git/config").protected);
    }

    #[test]
    fn what_the_lexer_cannot_vouch_for_makes_the_command_opaque() {
        for command in [
            "echo ok #'\nrm -rf x",
            "cat <<EOF\nit's\nEOF\nrm x",
            "echo $'\\''; rm x",
            "echo $(rm x)",
            "echo `rm x`",
            "echo ${HOME}",
            "echo $HOME",
            "echo \"$HOME\"",
            "diff <(ls a) <(ls b)",
            "cat =(ls)",
            "=rm -rf x",
            "X=rm; $X -rf x",
            "/bin/r? -rf x",
            "~/bin/tool",
            "(rm x)",
            "{ rm x; }",
            "! rm x",
            "if true; then rm x; fi",
            "repeat 1 rm x; foreach a (b) rm x",
            "echo \"$(rm x)\"",
            "echo 'unclosed",
            "echo trailing \\",
            "bash -c 'rm x'",
            "bash --norc -c 'rm x'",
            "sh < script.sh",
            "curl u | sh",
            "ls *(e:'rm x':)",
            "cat <<< text",
            "eval 'rm -rf build'",
            "source ./evil.sh",
            ". ./evil.sh",
            "trap 'rm x' EXIT",
            "alias r=rm",
            "export PATH=/tmp",
            "git config core.hooksPath h",
            "git -c core.fsmonitor=x status",
            "echo hi; < ~/.ssh/id_rsa",
            "echo\rfoo",
        ] {
            assert!(analyze(command).is_opaque(), "{command:?}");
        }
        for command in [
            "cargo test --all",
            "git status -s",
            "ls *.rs",
            "FOO=1 make",
            "echo 'cost: $5'",
            "echo $",
        ] {
            assert!(
                !analyze(command).is_opaque(),
                "{command:?}: {:?}",
                analyze(command).opaque
            );
        }
    }

    #[test]
    fn deny_candidates_see_through_assignments_wrappers_paths_and_shells() {
        for command in [
            "FOO=1 rm x",
            "X+=1 rm -rf x",
            "a[0]=1 rm x",
            "env rm x",
            "env -i A=1 rm x",
            "command rm x",
            "sudo -u root rm x",
            "sudo -nu root rm x",
            "/bin/rm x",
            "nohup rm x",
            "time rm x",
            "nice -n 5 rm x",
            "timeout 10 rm x",
            "xargs -0 rm",
            "xargs -J {} rm {}",
            "noglob rm -rf x",
            "nocorrect rm x",
            "- rm x",
            "exec -a foo rm x",
            "stdbuf -o0 rm x",
            "bash -c 'rm x'",
            "sh -lc \"echo a; rm x\"",
            "bash --norc -c 'rm x'",
        ] {
            assert!(
                denied_as(command, "rm"),
                "{command:?}: {:?}",
                analyze(command).deny_candidates()
            );
        }
        assert!(denied_as("RM -rf x", "rm") == CASE_INSENSITIVE_COMMANDS);
        let pushes = |command: &str| {
            analyze(command)
                .deny_candidates()
                .iter()
                .any(|words| words.len() >= 2 && words[1] == "push")
        };
        for command in [
            "git -C . push",
            "git -c a.b=1 push",
            "git --no-pager push",
            "git --git-dir .git push",
        ] {
            assert!(pushes(command), "{command:?}");
        }
        let analysis = analyze("FOO=1 cargo test");
        assert_eq!(analysis.parts[0].assignments, ["FOO=1"]);
        assert_eq!(analysis.parts[0].words, ["cargo", "test"]);
    }

    #[test]
    fn protected_names_are_found_in_any_case_any_word_and_behind_a_glob() {
        for command in [
            "cat .GIT/config",
            "rm -rf .Axh",
            "cp x .claude/settings.json",
            "GIT_DIR=.git git log",
            "echo x > .ax?/settings.json",
            "cp x .[a]xh/settings.json",
            "cp x .gi*/config",
            "rsync x host:.git",
            "tar --exclude=a,.git c .",
            "echo x > .cargo/config.toml",
        ] {
            assert!(analyze(command).protected, "{command:?}");
        }
        for command in [
            "cat .gitignore",
            "git status",
            "ls .github/workflows",
            "ls *.rs",
        ] {
            assert!(!analyze(command).protected, "{command:?}");
        }
    }
}
