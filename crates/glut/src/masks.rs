//! Secrets masked before they leave (T58 of `docs/plans/glut-tui.md`): every tool result is searched for keys before
//! the model, the stream or a file of the session sees it, and a hit becomes a numbered placeholder —
//! `‹secret 1: anthropic key, 108 chars›`. Three ways find one: the values Glut knows itself (its own keys, found
//! exactly — a pattern once let the owner's OpenRouter key through), known formats, and assignments whose name says
//! secret (`API_KEY=…`). A value once hidden is searched exactly from then on, and the same value keeps its number for
//! the whole session, so the cache holds and the model can say that two places hold the same key.
//!
//! Writing one back is the trap: a model that read a masked `.env` and writes it anew would put the placeholder where
//! the key was. [`Masks::unmask`] puts the value back only into a file that already holds it — nothing new leaves —
//! and refuses every other; a command, an MCP server or the web gets the placeholder as written.
//!
//! What the owner adds (formats of their own, values let through always, masking off) is `~/.glut/secrets.json`,
//! a file Glut writes from `/secrets` — never the owner's `settings.json`, never a project's `.glut/`.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, MutexGuard, PoisonError};

use regex::Regex;
use serde::{Deserialize, Serialize};
use sha2::{Digest as _, Sha256};

use crate::error::HarnessError;

/// The file below the home folder's `.glut` holding what the owner set.
const FILE: &str = "secrets.json";
/// A secrets file larger than this is refused rather than read.
const MAX_FILE_BYTES: u64 = 256 * 1024;
/// A known value shorter than this is no secret worth searching: it would hide ordinary words.
const MIN_KNOWN_CHARS: usize = 8;
/// What opens a placeholder; no key format holds it, so a placeholder is never masked again.
pub const OPEN: char = '‹';
/// What closes a placeholder.
pub const CLOSE: char = '›';

/// The formats Glut knows: a name for the placeholder and the pattern. Earlier entries win where two match at the
/// same place with the same length (`sk-ant-…` is Anthropic's before it is anyone's `sk-…`).
const FORMATS: [(&str, &str); 33] = [
    // To its end, or to the end of the text when a read stopped inside it.
    (
        "private key",
        r"-----BEGIN[A-Z ]*PRIVATE KEY(?: BLOCK)?-----[\s\S]*?(?:-----END[A-Z ]*PRIVATE KEY(?: BLOCK)?-----|\z)",
    ),
    // A key's body read without its first line (`sed -n 2,20p`): three lines or more of base64 at a PEM's width.
    (
        "key material",
        r"(?m)(?:^[A-Za-z0-9+/]{60,80}={0,2}\r?\n){3,}",
    ),
    ("anthropic key", r"sk-ant-[A-Za-z0-9_\-]{20,}"),
    ("openrouter key", r"sk-or-v1-[A-Za-z0-9]{32,}"),
    (
        "openai key",
        r"\bsk-(?:proj-|svcacct-|admin-)?[A-Za-z0-9_\-]{20,}",
    ),
    ("groq key", r"\bgsk_[A-Za-z0-9]{40,}"),
    ("xai key", r"\bxai-[A-Za-z0-9]{40,}"),
    ("perplexity key", r"\bpplx-[A-Za-z0-9]{40,}"),
    ("replicate token", r"\br8_[A-Za-z0-9]{30,}"),
    ("github token", r"\bgh[pousr]_[A-Za-z0-9]{30,}"),
    ("github token", r"\bgithub_pat_[A-Za-z0-9_]{40,}"),
    ("gitlab token", r"\bglpat-[A-Za-z0-9_\-]{20,}"),
    ("aws access key", r"\b(?:AKIA|ASIA)[A-Z0-9]{16}\b"),
    ("slack token", r"\bxox[abposr]-[A-Za-z0-9\-]{10,}"),
    (
        "slack webhook",
        r"https://hooks\.slack\.com/services/[A-Za-z0-9/]{20,}",
    ),
    ("stripe key", r"\b[rs]k_(?:live|test)_[A-Za-z0-9]{16,}"),
    ("google api key", r"\bAIza[0-9A-Za-z_\-]{35}"),
    ("hugging face token", r"\bhf_[A-Za-z0-9]{30,}"),
    ("npm token", r"\bnpm_[A-Za-z0-9]{36}"),
    ("pypi token", r"\bpypi-[A-Za-z0-9_\-]{50,}"),
    ("docker token", r"\bdckr_pat_[A-Za-z0-9_\-]{20,}"),
    ("vault token", r"\bhvs\.[A-Za-z0-9_\-]{20,}"),
    (
        "sendgrid key",
        r"\bSG\.[A-Za-z0-9_\-]{16,}\.[A-Za-z0-9_\-]{16,}",
    ),
    ("shopify token", r"\bshp(?:at|ca|pa|ss)_[a-fA-F0-9]{32}"),
    ("databricks token", r"\bdapi[a-f0-9]{32}"),
    (
        "telegram bot token",
        r"\b[0-9]{8,10}:AA[A-Za-z0-9_\-]{33}\b",
    ),
    ("tavily key", r"\btvly-[A-Za-z0-9\-]{20,}"),
    (
        "jwt",
        r"\beyJ[A-Za-z0-9_\-]{10,}\.eyJ[A-Za-z0-9_\-]{10,}\.[A-Za-z0-9_\-]{10,}",
    ),
    // The groups are what is hidden: a URL's password (`postgres://user:pw@host`), a header's credentials.
    (
        "password in a url",
        r"\b[a-z][a-z0-9+.\-]*://[^/\s:@]+:([^/\s@]{3,})@",
    ),
    (
        "bearer token",
        r"(?i)\bauthorization:\s*bearer\s+([A-Za-z0-9_\-.~+/]{16,}=*)",
    ),
    (
        "basic credentials",
        r"(?i)\bauthorization:\s*basic\s+([A-Za-z0-9+/]{8,}={0,2})",
    ),
    (
        "azure account key",
        r"(?i)\bAccountKey=([A-Za-z0-9+/]{40,}={0,2})",
    ),
    (
        "kubernetes key",
        r"\bclient-key-data:\s*([A-Za-z0-9+/]{40,}={0,2})",
    ),
];

/// An assignment whose name says secret: `API_KEY=…`, `password: …`, `"token": "…"`. Group 1 is the name, 2 or 3 a
/// value in quotes, 4 one without; [`plausible`] decides whether the value is one.
const ASSIGNMENT: &str = concat!(
    r#"(?i)["']?\b([a-z0-9_.\-]*(?:api[_\-]?key|apikey|secret|token|passw(?:or)?d|pwd|credentials?|private[_\-]?key|"#,
    r#"access[_\-]?key)[a-z0-9_\-]*)["']?\s*(?:=|:|=>)\s*"#,
    r#"(?:"([^"\n]{8,})"|'([^'\n]{8,})'|([A-Za-z0-9_\-+/=.~!@%^*$&?]{8,}))"#,
);

/// The arguments of `write_file` and `edit_file` that are a file's text: the only ones a hidden value goes back into.
pub const WRITTEN_FIELDS: [&str; 3] = ["content", "old_string", "new_string"];

/// A value hidden once is searched exactly from then on when it is at least this long ([`MIN_KNOWN_CHARS`] too): a
/// short one (a URL's three-letter password) would hide every word it is part of.
const MIN_LEARNED_CHARS: usize = MIN_KNOWN_CHARS;
/// The most values one session remembers: a file of thousands of fake keys must not make every later result slow.
const MOST_LEARNED: usize = 512;
/// The longest value remembered for the exact search.
const LONGEST_LEARNED: usize = 8 * 1024;

/// A placeholder in a text: group 1 is its number.
const PLACEHOLDER: &str = r"‹secret (\d+): [^›\n]{1,80}›";

/// What the owner set (`~/.glut/secrets.json`): written by `/secrets`, readable by hand.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Owner {
    /// `false`: nothing is masked. Absent counts as on.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mask: Option<bool>,
    /// Formats of the owner's own beside Glut's.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub patterns: Vec<OwnPattern>,
    /// Values always let through, as the hex of their SHA-256 — never the value itself.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub allowed: Vec<String>,
}

/// One format of the owner's own: its name in the placeholder and its pattern.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OwnPattern {
    pub name: String,
    pub regex: String,
}

impl Owner {
    /// The file under `home` (`~`).
    pub fn path(home: &Path) -> PathBuf {
        home.join(".glut").join(FILE)
    }

    /// What the owner set; none when the file is missing.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the file cannot be read, is too large or does not parse, or a pattern in it
    /// does not compile: masking must not quietly fall back to less than the owner asked for.
    pub fn load(home: &Path) -> Result<Self, HarnessError> {
        use std::io::Read as _;
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        let file = match std::fs::File::open(&path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(err) => return Err(fail(err.to_string())),
        };
        let mut text = String::new();
        file.take(MAX_FILE_BYTES + 1)
            .read_to_string(&mut text)
            .map_err(|err| fail(err.to_string()))?;
        if text.len() as u64 > MAX_FILE_BYTES {
            return Err(fail(format!("larger than {MAX_FILE_BYTES} bytes")));
        }
        let owner: Self = serde_json::from_str(&text).map_err(|err| fail(err.to_string()))?;
        for pattern in &owner.patterns {
            compile(&pattern.regex)
                .map_err(|reason| fail(format!("{}: {reason}", pattern.name)))?;
        }
        Ok(owner)
    }

    /// Writes the file under `home`, only the owner able to read it.
    ///
    /// # Errors
    ///
    /// [`HarnessError::Settings`] when the folder or the file cannot be written.
    pub fn save(&self, home: &Path) -> Result<(), HarnessError> {
        let path = Self::path(home);
        let fail = |reason: String| HarnessError::Settings {
            path: path.display().to_string(),
            reason,
        };
        let dir = path.parent().ok_or_else(|| fail("no folder".into()))?;
        crate::providers::make_private_dir(dir).map_err(|err| fail(err.to_string()))?;
        let text = serde_json::to_string_pretty(self).map_err(|err| fail(err.to_string()))?;
        crate::providers::write_replacing(dir, &path, text.as_bytes(), 0o600)
            .map_err(|err| fail(err.to_string()))
    }

    /// Whether masking is on.
    pub fn masks(&self) -> bool {
        self.mask != Some(false)
    }
}

/// Whether `regex` is a pattern masking can use: it compiles and matches no empty text (which would hide nothing
/// and everywhere).
///
/// # Errors
///
/// Why not, in words.
pub fn compile(regex: &str) -> Result<Regex, String> {
    let compiled = Regex::new(regex).map_err(|err| err.to_string())?;
    if compiled.is_match("") {
        return Err("it matches an empty text".into());
    }
    Ok(compiled)
}

/// The hex of a value's SHA-256: how a value let through always is kept.
pub fn fingerprint(value: &str) -> String {
    Sha256::digest(value.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// A value hidden in this session, as `/secrets` lists it — never with the value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hidden {
    pub number: u32,
    pub kind: String,
    pub chars: usize,
    /// Where it was seen, first first: a tool and its target, "the instructions", "your message".
    pub seen: Vec<String>,
}

/// A text with its secrets replaced, and what was hidden in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Masked {
    pub text: String,
    /// The placeholders put in, one per place.
    pub hidden: Vec<String>,
}

/// The session's masking: shared by the run, its subagents, its scripts and the TUI that started it, so every one
/// numbers a value alike. Cloning shares it; [`Masks::session`] makes one for another session that shares only the
/// rules — on or off, the formats, the values let through always — as a screen's panes do.
/// [`RunConfig::masks`](crate::RunConfig) `None` masks nothing.
#[derive(Debug, Clone)]
pub struct Masks {
    rules: Arc<Mutex<Rules>>,
    seen: Arc<Mutex<Seen>>,
}

/// What every session of a screen masks by.
#[derive(Debug)]
struct Rules {
    /// `false`: nothing is masked (`/secrets off`), though what was hidden can still be written back.
    on: bool,
    formats: Vec<(String, Regex)>,
    /// How many of [`Rules::formats`] are Glut's own; the owner's follow.
    own_from: usize,
    /// Values Glut knows itself, with a name: found exactly before any pattern.
    known: Vec<(String, String)>,
    /// Let through always, by [`fingerprint`].
    allowed_always: HashSet<String>,
}

/// What one session has hidden.
#[derive(Debug)]
struct Seen {
    hidden: Vec<Value>,
    next: u32,
    /// Let through for this session.
    allowed: HashSet<String>,
}

impl Default for Seen {
    fn default() -> Self {
        Seen {
            hidden: Vec::new(),
            next: 1,
            allowed: HashSet::new(),
        }
    }
}

#[derive(Debug)]
struct Value {
    number: u32,
    kind: String,
    value: String,
    seen: Vec<String>,
}

/// One hit in a text, in bytes.
struct Hit {
    start: usize,
    end: usize,
    kind: String,
    /// Before the others at the same place: Glut's own values, then the formats in their order, the assignments last.
    rank: usize,
}

/// [`ASSIGNMENT`], compiled once — a constant of this file, tested below.
static ASSIGNMENT_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(ASSIGNMENT).expect("the assignment pattern compiles"));
/// [`PLACEHOLDER`], compiled once.
static PLACEHOLDER_RE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(PLACEHOLDER).expect("the placeholder pattern compiles"));

impl Default for Masks {
    fn default() -> Self {
        Self::new(&Owner::default())
    }
}

impl Masks {
    /// Glut's formats and the owner's, with the owner's values let through always. A pattern of the owner's that does
    /// not compile is left out — [`Owner::load`] has refused the file before.
    pub fn new(owner: &Owner) -> Self {
        let mut formats: Vec<(String, Regex)> = FORMATS
            .iter()
            .filter_map(|(name, regex)| Some(((*name).to_owned(), Regex::new(regex).ok()?)))
            .collect();
        let own_from = formats.len();
        formats.extend(
            owner
                .patterns
                .iter()
                .filter_map(|own| Some((own.name.clone(), compile(&own.regex).ok()?))),
        );
        let rules = Rules {
            on: owner.masks(),
            formats,
            own_from,
            known: Vec::new(),
            allowed_always: owner.allowed.iter().cloned().collect(),
        };
        Masks {
            rules: Arc::new(Mutex::new(rules)),
            seen: Arc::default(),
        }
    }

    /// Masking for another session under the same rules: its own numbers, its own values let through for it.
    pub fn session(&self) -> Self {
        Masks {
            rules: Arc::clone(&self.rules),
            seen: Arc::default(),
        }
    }

    /// A value Glut knows to be secret — a provider's key, an MCP server's token — found exactly wherever it shows.
    pub fn know(&self, kind: &str, value: &str) {
        let value = value.trim();
        if value.chars().count() < MIN_KNOWN_CHARS {
            return;
        }
        let mut rules = self.rules();
        if !rules.known.iter().any(|(_, known)| known == value) {
            rules.known.push((kind.to_owned(), value.to_owned()));
        }
    }

    /// Whether masking is on.
    pub fn is_on(&self) -> bool {
        self.rules().on
    }

    /// Turns masking on or off from the next text on; what was hidden keeps its number.
    pub fn set_on(&self, on: bool) {
        self.rules().on = on;
    }

    /// A format of the owner's own, from the next text on (`/secrets add`).
    pub fn add_format(&self, name: &str, regex: Regex) {
        self.rules().formats.push((name.to_owned(), regex));
    }

    /// Drops the owner's format `name`; whether there was one. Glut's own stay.
    pub fn remove_format(&self, name: &str) -> bool {
        let mut rules = self.rules();
        let from = rules.own_from;
        let before = rules.formats.len();
        let mut at = 0;
        rules.formats.retain(|(own, _)| {
            at += 1;
            at <= from || own != name
        });
        rules.formats.len() < before
    }

    /// Lets a value through always, by its fingerprint, for every session under these rules.
    pub fn allow_always(&self, print: &str) {
        self.rules().allowed_always.insert(print.to_owned());
    }

    /// `text` with every secret replaced by its placeholder; `at` says where it was seen, for `/secrets`.
    pub fn mask(&self, text: &str, at: &str) -> Masked {
        let rules = self.rules();
        let mut seen = self.seen();
        let hits = if rules.on {
            hits(&rules, &seen, text)
        } else {
            Vec::new()
        };
        drop(rules);
        if hits.is_empty() {
            return Masked {
                text: text.to_owned(),
                hidden: Vec::new(),
            };
        }
        let mut out = String::with_capacity(text.len());
        let mut hidden = Vec::new();
        let mut from = 0;
        for hit in hits {
            out.push_str(&text[from..hit.start]);
            let placeholder = seen.number(&text[hit.start..hit.end], &hit.kind, at);
            out.push_str(&placeholder);
            hidden.push(placeholder);
            from = hit.end;
        }
        out.push_str(&text[from..]);
        Masked { text: out, hidden }
    }

    /// The kinds of secret in `text`, without numbering anything: what the TUI warns of before the owner's message
    /// goes.
    pub fn kinds_in(&self, text: &str) -> Vec<String> {
        let rules = self.rules();
        let seen = self.seen();
        let mut kinds: Vec<String> = Vec::new();
        if !rules.on {
            return kinds;
        }
        for hit in hits(&rules, &seen, text) {
            if !kinds.contains(&hit.kind) {
                kinds.push(hit.kind);
            }
        }
        kinds
    }

    /// The arguments of a write with each placeholder's value put back — only in the text a file is given
    /// ([`WRITTEN_FIELDS`]) and only where `holds` says the file written already holds that value, so nothing leaves
    /// that was not there. A placeholder the file holds as it is (a doc or a test that shows one) stays as written.
    /// Arguments without placeholders, or not an object, come back as they were.
    ///
    /// # Errors
    ///
    /// The model's words for a placeholder outside the written text (a path named after a secret would carry it out),
    /// one whose value this session does not know (an earlier window's), or one whose file does not hold it.
    pub fn unmask(&self, arguments: &str, holds: impl Fn(&str) -> bool) -> Result<String, String> {
        if !arguments.contains(OPEN) {
            return Ok(arguments.to_owned());
        }
        let Ok(serde_json::Value::Object(mut fields)) = serde_json::from_str(arguments) else {
            return Ok(arguments.to_owned());
        };
        let mut changed = false;
        for (name, value) in &mut fields {
            let Some(text) = value.as_str().filter(|text| PLACEHOLDER_RE.is_match(text)) else {
                continue;
            };
            if !WRITTEN_FIELDS.contains(&name.as_str()) {
                return Err(format!(
                    "error: `{name}` holds a placeholder of a hidden value — Glut puts one back only into the text \
                     a file is given, never into its path or another argument"
                ));
            }
            let back = self.put_back(text, &holds)?;
            if back != text {
                *value = serde_json::Value::String(back);
                changed = true;
            }
        }
        Ok(if changed {
            serde_json::Value::Object(fields).to_string()
        } else {
            arguments.to_owned()
        })
    }

    /// [`Masks::unmask`] for one text.
    fn put_back(&self, text: &str, holds: &impl Fn(&str) -> bool) -> Result<String, String> {
        let seen = self.seen();
        let mut out = String::with_capacity(text.len());
        let mut from = 0;
        for found in PLACEHOLDER_RE.captures_iter(text) {
            let whole = found.get(0).expect("a match has its whole");
            let number: u32 = found[1].parse().unwrap_or(0);
            let value = seen.hidden.iter().find(|value| value.number == number);
            out.push_str(&text[from..whole.start()]);
            from = whole.end();
            match value {
                Some(value) if holds(&value.value) => out.push_str(&value.value),
                // The file shows the placeholder itself: it is the file's own text, kept.
                _ if holds(whole.as_str()) => out.push_str(whole.as_str()),
                Some(value) => {
                    return Err(format!(
                        "error: secret {number} ({}) is not in this file — Glut puts a hidden value back only into \
                         a file that already holds it; it was seen in {}",
                        value.kind,
                        value.seen.join(", ")
                    ));
                }
                None => {
                    return Err(format!(
                        "error: secret {number} is not known any more — read the file again, then write around the \
                         placeholder"
                    ));
                }
            }
        }
        out.push_str(&text[from..]);
        Ok(out)
    }

    /// Numbers go on above every placeholder in `texts` (a session read back): a new value never takes the number
    /// of one the conversation already speaks of.
    pub fn continue_after<'a>(&self, texts: impl IntoIterator<Item = &'a str>) {
        let highest = texts
            .into_iter()
            .filter(|text| text.contains(OPEN))
            .flat_map(|text| {
                PLACEHOLDER_RE
                    .captures_iter(text)
                    .filter_map(|found| found[1].parse::<u32>().ok())
                    .collect::<Vec<_>>()
            })
            .max()
            .unwrap_or(0);
        let mut seen = self.seen();
        seen.next = seen.next.max(highest.saturating_add(1));
    }

    /// The values hidden so far, by number.
    pub fn hidden(&self) -> Vec<Hidden> {
        self.seen()
            .hidden
            .iter()
            .map(|value| Hidden {
                number: value.number,
                kind: value.kind.clone(),
                chars: value.value.chars().count(),
                seen: value.seen.clone(),
            })
            .collect()
    }

    /// Lets the value `number` through from now on, for this session; returns its fingerprint for keeping it always
    /// ([`Owner::allowed`], [`Masks::allow_always`]), `None` when no value has that number.
    pub fn allow(&self, number: u32) -> Option<String> {
        let mut seen = self.seen();
        let value = seen
            .hidden
            .iter()
            .find(|value| value.number == number)?
            .value
            .clone();
        let print = fingerprint(&value);
        seen.allowed.insert(value);
        Some(print)
    }

    /// Forgets every value seen: a new session in the same window starts its numbers anew.
    pub fn forget(&self) {
        *self.seen() = Seen::default();
    }

    /// The rules, always taken before [`Masks::seen`] where both are. A poisoned lock still holds them: a thread that
    /// panicked loses nothing of the others'.
    fn rules(&self) -> MutexGuard<'_, Rules> {
        self.rules.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn seen(&self) -> MutexGuard<'_, Seen> {
        self.seen.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Every secret in `text`, in order, none overlapping another, none let through.
fn hits(rules: &Rules, seen: &Seen, text: &str) -> Vec<Hit> {
    let mut hits = Vec::new();
    let exact = rules
        .known
        .iter()
        .map(|(kind, value)| (kind.as_str(), value.as_str()))
        .chain(
            seen.hidden
                .iter()
                .filter(|value| value.value.chars().count() >= MIN_LEARNED_CHARS)
                .map(|value| (value.kind.as_str(), value.value.as_str())),
        );
    for (kind, value) in exact {
        for (start, _) in text.match_indices(value) {
            hits.push(Hit {
                start,
                end: start + value.len(),
                kind: kind.to_owned(),
                rank: 0,
            });
        }
    }
    for (rank, (kind, regex)) in rules.formats.iter().enumerate() {
        for found in regex.captures_iter(text) {
            // One of Glut's formats with a group hides the group; the owner's hide the whole match, whatever groups
            // they use to say it.
            let group = if rank < rules.own_from {
                found.get(1)
            } else {
                None
            };
            let Some(part) = group.or_else(|| found.get(0)) else {
                continue;
            };
            hits.push(Hit {
                start: part.start(),
                end: part.end(),
                kind: kind.clone(),
                rank: rank + 1,
            });
        }
    }
    for found in ASSIGNMENT_RE.captures_iter(text) {
        let Some(name) = found.get(1) else {
            continue;
        };
        let (value, quoted) = match (found.get(2), found.get(3), found.get(4)) {
            (Some(value), _, _) | (_, Some(value), _) => (value, true),
            (_, _, Some(value)) => (value, false),
            _ => continue,
        };
        if plausible(name.as_str(), value.as_str(), &text[value.end()..], quoted) {
            hits.push(Hit {
                start: value.start(),
                end: value.end(),
                kind: name.as_str().to_owned(),
                rank: rules.formats.len() + 1,
            });
        }
    }
    hits.sort_by(|a, b| {
        a.start
            .cmp(&b.start)
            .then((b.end - b.start).cmp(&(a.end - a.start)))
            .then(a.rank.cmp(&b.rank))
    });
    // Overlapping hits become one, so no tail of a longer one stays bare; a value let through keeps what lies in it.
    let mut kept: Vec<(Hit, bool)> = Vec::new();
    for hit in hits {
        if let Some((last, _)) = kept.last_mut()
            && hit.start < last.end
        {
            last.end = last.end.max(hit.end);
            continue;
        }
        kept.push((hit, false));
    }
    for (hit, through) in &mut kept {
        let value = &text[hit.start..hit.end];
        *through =
            seen.allowed.contains(value) || rules.allowed_always.contains(&fingerprint(value));
    }
    kept.into_iter()
        .filter(|(_, through)| !through)
        .map(|(hit, _)| hit)
        .collect()
}

impl Seen {
    /// The placeholder of `value`: its number if it was hidden before, else the next. Past [`MOST_LEARNED`] values, or
    /// one longer than [`LONGEST_LEARNED`], a new one is numbered but not remembered — hidden, never written back.
    fn number(&mut self, value: &str, kind: &str, at: &str) -> String {
        let chars = value.chars().count();
        let index = match self.hidden.iter().position(|known| known.value == value) {
            Some(index) => index,
            None if self.hidden.len() >= MOST_LEARNED || chars > LONGEST_LEARNED => {
                let number = self.next;
                self.next += 1;
                return placeholder(number, kind, chars);
            }
            None => {
                let number = self.next;
                self.next += 1;
                self.hidden.push(Value {
                    number,
                    kind: kind.to_owned(),
                    value: value.to_owned(),
                    seen: Vec::new(),
                });
                self.hidden.len() - 1
            }
        };
        let entry = &mut self.hidden[index];
        if !at.is_empty() && !entry.seen.iter().any(|seen| seen == at) {
            entry.seen.push(at.to_owned());
        }
        placeholder(entry.number, &entry.kind, chars)
    }
}

/// How a hidden value reads in a text.
pub fn placeholder(number: u32, kind: &str, chars: usize) -> String {
    let kind: String = kind
        .chars()
        .filter(|c| *c != CLOSE && *c != '\n')
        .take(60)
        .collect();
    format!("{OPEN}secret {number}: {kind}, {chars} chars{CLOSE}")
}

/// Names that say password: their value is one even without a digit (`PASSWORD=changeme`).
const PASSWORD_NAMES: [&str; 4] = ["passw", "pwd", "secret", "private"];

/// Whether an assignment's value looks like a secret rather than code. A value in quotes, or one that ends its line
/// (`.env`, YAML, a shell's `export`) — never one followed by more code (`token: Sha256Digest,`,
/// `let secret = decode_v2;`). Not a reference (`${DB_PASSWORD}`, `{{ token }}`, `config.api_key2`), no spaces (a
/// sentence, a placeholder text), not one character over and over; letters and digits both, except under a name that
/// says password.
fn plausible(name: &str, value: &str, after: &str, quoted: bool) -> bool {
    if !quoted {
        let rest = after.trim_start_matches([' ', '\t']);
        if !(rest.is_empty() || rest.starts_with(['\n', '\r', '#'])) {
            return false;
        }
    }
    if value.contains("${")
        || value.contains("{{")
        || value.starts_with(['$', '<', '%'])
        || value.contains(char::is_whitespace)
    {
        return false;
    }
    let has_digit = value.chars().any(|c| c.is_ascii_digit());
    let has_letter = value.chars().any(|c| c.is_ascii_alphabetic());
    let name = name.to_ascii_lowercase();
    let says_password = PASSWORD_NAMES.iter().any(|word| name.contains(word));
    // `self.token1`, `config.api_key2`: a path through names, each starting with a letter.
    let a_path = value.contains('.')
        && value.split('.').all(|part| {
            part.chars()
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        });
    let first = value.chars().next();
    let repeated = value.chars().all(|c| Some(c) == first);
    has_letter && (has_digit || says_password) && !a_path && !repeated
}

#[cfg(test)]
mod tests {
    use super::*;

    const ANTHROPIC: &str = "sk-ant-api03-abcdefghijklmnopqrstuvwxyz0123456789";

    fn masked(text: &str) -> String {
        Masks::default().mask(text, "test").text
    }

    #[test]
    fn the_constant_patterns_compile() {
        for (name, regex) in FORMATS {
            assert!(compile(regex).is_ok(), "{name}");
        }
        assert!(compile(ASSIGNMENT).is_ok());
        assert!(Regex::new(PLACEHOLDER).is_ok());
    }

    #[test]
    fn known_formats_are_hidden_with_kind_and_length() {
        let text = masked(&format!("key: {ANTHROPIC}\n"));
        assert_eq!(text, "key: ‹secret 1: anthropic key, 49 chars›\n");
        for (secret, kind) in [
            ("ghp_abcdefghijklmnopqrstuvwxyz0123456789", "github token"),
            ("AKIAIOSFODNN7EXAMPLE", "aws access key"),
            (
                "sk-or-v1-0123456789abcdef0123456789abcdef",
                "openrouter key",
            ),
            ("glpat-abcdefghij0123456789", "gitlab token"),
        ] {
            let text = masked(&format!("x {secret} y"));
            assert!(text.contains(kind) && !text.contains(secret), "{text}");
        }
    }

    #[test]
    fn a_private_key_block_goes_whole() {
        let block = "-----BEGIN OPENSSH PRIVATE KEY-----\nb3BlbnNzaC1rZXk\nAAAA\n-----END OPENSSH PRIVATE KEY-----";
        let text = masked(&format!("cat id\n{block}\ndone"));
        assert!(
            text.starts_with("cat id\n‹secret 1: private key,") && text.ends_with("›\ndone"),
            "{text}"
        );
    }

    #[test]
    fn assignments_hide_the_value_and_name_it() {
        let text = masked("DATABASE_PASSWORD=hunter2rocks9\nPORT=8080\n");
        assert_eq!(
            text,
            "DATABASE_PASSWORD=‹secret 1: DATABASE_PASSWORD, 13 chars›\nPORT=8080\n"
        );
        let json = masked(r#"{"api_key": "a1b2c3d4e5f6"}"#);
        assert_eq!(json, r#"{"api_key": "‹secret 1: api_key, 12 chars›"}"#);
    }

    #[test]
    fn code_is_no_assignment_of_a_secret() {
        for code in [
            "let token = next_token2(input);",
            "api_key: config.api_key2,",
            "password: Option<String>,",
            "max_tokens = 4096",
            "secret = xxxxxxxxxxxx",
            "token_type: bearer_access",
            "pub token: Sha256Digest,",
            "access_token: Oauth2Token,",
            "password: Utf8String,",
            "private_key = ed25519_key,",
            "let secret = base64_v2_decode;",
            r#"password: "${DB_PASSWORD}""#,
            r#"api_key: "your-api-key-here""#,
            r#"token: "{{ vault_token }}""#,
        ] {
            assert_eq!(masked(code), code);
        }
    }

    #[test]
    fn the_same_value_keeps_its_number_and_is_found_bare_later() {
        let masks = Masks::default();
        let first = masks.mask("API_KEY=abc123def456ghi", ".env").text;
        assert_eq!(first, "API_KEY=‹secret 1: API_KEY, 15 chars›");
        // Learned: the bare value in a log is found though no name stands before it.
        let later = masks.mask("sent abc123def456ghi upstream", "log").text;
        assert_eq!(later, "sent ‹secret 1: API_KEY, 15 chars› upstream");
        let other = masks.mask(ANTHROPIC, "x").text;
        assert!(other.starts_with("‹secret 2:"), "{other}");
        assert_eq!(masks.hidden()[0].seen, [".env", "log"]);
    }

    #[test]
    fn a_known_value_is_found_exactly_whatever_its_form() {
        let masks = Masks::default();
        masks.know("openrouter key", "plain-looking-value-42");
        let text = masks.mask("echo plain-looking-value-42", "bash").text;
        assert_eq!(text, "echo ‹secret 1: openrouter key, 22 chars›");
        masks.know("short", "abc");
        assert_eq!(masks.mask("abc", "x").text, "abc");
    }

    #[test]
    fn a_url_password_and_a_bearer_token_are_hidden() {
        let text = masked("postgres://app:s3cretpass@db:5432/x");
        assert_eq!(
            text,
            "postgres://app:‹secret 1: password in a url, 10 chars›@db:5432/x"
        );
        let header = masked("Authorization: Bearer abcdefghijklmnop1234");
        assert!(
            header.starts_with("Authorization: Bearer ‹secret 1: bearer token"),
            "{header}"
        );
    }

    #[test]
    fn a_placeholder_is_never_masked_again() {
        let masks = Masks::default();
        let once = masks.mask(&format!("API_KEY={ANTHROPIC}"), "a").text;
        assert_eq!(masks.mask(&once, "b").text, once);
    }

    #[test]
    fn a_value_goes_back_only_into_a_file_that_holds_it() {
        let masks = Masks::default();
        let shown = masks.mask("TOKEN=abc123def456ghi\n", ".env").text;
        let arguments =
            serde_json::json!({ "path": ".env", "content": format!("{shown}DEBUG=1\n") })
                .to_string();
        let back = masks
            .unmask(&arguments, |value| value == "abc123def456ghi")
            .unwrap();
        assert!(back.contains("TOKEN=abc123def456ghi\\nDEBUG=1"), "{back}");
        let refused = masks.unmask(&arguments, |_| false).unwrap_err();
        assert!(
            refused.contains("secret 1") && refused.contains(".env"),
            "{refused}"
        );
        let unknown = masks
            .unmask(r#"{"content":"‹secret 7: x, 3 chars›"}"#, |_| false)
            .unwrap_err();
        assert!(unknown.contains("not known"), "{unknown}");
        // A file that shows a placeholder as its own text keeps it as written.
        let literal = r#"{"content":"‹secret 7: x, 3 chars›"}"#;
        assert_eq!(
            masks.unmask(literal, |held| held.starts_with('‹')).unwrap(),
            literal
        );
        // Never into a path: a file named after the value would carry it out.
        let named = serde_json::json!({ "path": shown.trim_start_matches("TOKEN=").trim_end(), "content": "x" });
        let refused = masks.unmask(&named.to_string(), |_| true).unwrap_err();
        assert!(refused.contains("`path`"), "{refused}");
        assert_eq!(
            masks.unmask(r#"{"a":"b"}"#, |_| false).unwrap(),
            r#"{"a":"b"}"#
        );
    }

    #[test]
    fn a_value_with_quotes_goes_back_as_json() {
        let masks = Masks::default();
        masks.know("own", "pa\"ss\\word123");
        let shown = masks.mask("pa\"ss\\word123", "x").text;
        let arguments = serde_json::json!({ "content": shown }).to_string();
        let back = masks.unmask(&arguments, |_| true).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&back).unwrap();
        assert_eq!(parsed["content"], "pa\"ss\\word123");
    }

    #[test]
    fn numbers_go_on_above_an_earlier_conversation() {
        let masks = Masks::default();
        masks.continue_after([
            "see ‹secret 4: x, 9 chars› and ‹secret 2: y, 9 chars›",
            "none",
        ]);
        let text = masks.mask(ANTHROPIC, "x").text;
        assert!(text.starts_with("‹secret 5:"), "{text}");
        masks.forget();
        assert!(masks.hidden().is_empty());
    }

    #[test]
    fn a_value_let_through_stays_and_always_by_its_fingerprint() {
        let masks = Masks::default();
        masks.mask(ANTHROPIC, "x");
        let print = masks.allow(1).unwrap();
        assert_eq!(masks.mask(ANTHROPIC, "x").text, ANTHROPIC);
        assert_eq!(masks.allow(9), None);
        let owner = Owner {
            allowed: vec![print],
            ..Owner::default()
        };
        assert_eq!(Masks::new(&owner).mask(ANTHROPIC, "x").text, ANTHROPIC);
    }

    #[test]
    fn the_owners_formats_join_and_a_bad_one_is_refused() {
        let owner = Owner {
            patterns: vec![OwnPattern {
                name: "acme token".into(),
                regex: r"acme_[0-9a-f]{12}".into(),
            }],
            ..Owner::default()
        };
        let text = Masks::new(&owner).mask("t=acme_0123456789ab", "x").text;
        assert_eq!(text, "t=‹secret 1: acme token, 17 chars›");
        assert!(compile("(").is_err());
        assert!(compile("a*").is_err());
    }

    #[test]
    fn the_owners_file_round_trips_and_a_bad_pattern_fails_it() {
        let home = std::env::temp_dir().join(format!("glut-masks-owner-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        assert_eq!(Owner::load(&home).unwrap(), Owner::default());
        let owner = Owner {
            mask: Some(false),
            patterns: vec![OwnPattern {
                name: "n".into(),
                regex: "x[0-9]+".into(),
            }],
            allowed: vec![fingerprint("v")],
        };
        owner.save(&home).unwrap();
        let loaded = Owner::load(&home).unwrap();
        assert_eq!(loaded, owner);
        assert!(!loaded.masks());
        std::fs::write(
            Owner::path(&home),
            r#"{"patterns":[{"name":"n","regex":"("}]}"#,
        )
        .unwrap();
        assert!(matches!(
            Owner::load(&home),
            Err(HarnessError::Settings { .. })
        ));
    }

    #[test]
    fn off_masks_nothing_and_still_writes_back() {
        let masks = Masks::default();
        let shown = masks.mask(ANTHROPIC, "x").text;
        masks.set_on(false);
        assert_eq!(masks.mask(ANTHROPIC, "x").text, ANTHROPIC);
        assert!(masks.kinds_in(ANTHROPIC).is_empty());
        let back = masks
            .unmask(&format!(r#"{{"content":"{shown}"}}"#), |value| {
                value == ANTHROPIC
            })
            .unwrap();
        assert!(back.contains(ANTHROPIC));
        let off = Owner {
            mask: Some(false),
            ..Owner::default()
        };
        assert!(!Masks::new(&off).is_on());
    }

    #[test]
    fn an_own_format_comes_and_goes_and_glut_s_stay() {
        let masks = Masks::default();
        masks.add_format("acme", compile(r"acme_[0-9]{6}").unwrap());
        assert!(masks.mask("acme_123456", "x").text.contains("acme"));
        assert!(masks.remove_format("acme"));
        assert!(!masks.remove_format("acme"));
        assert!(
            !masks.remove_format("anthropic key"),
            "Glut's own formats are not the owner's to drop"
        );
        assert_eq!(masks.mask("acme_654321", "x").text, "acme_654321");
    }

    #[test]
    fn another_session_shares_the_rules_but_numbers_its_own() {
        let first = Masks::default();
        let second = first.session();
        first.mask(ANTHROPIC, "x");
        assert!(second.hidden().is_empty());
        assert!(
            second
                .mask("TOKEN=abc123def456ghi", "y")
                .text
                .starts_with("TOKEN=‹secret 1:")
        );
        first.set_on(false);
        assert_eq!(second.mask(ANTHROPIC, "x").text, ANTHROPIC);
        first.set_on(true);
        second.allow_always(&fingerprint(ANTHROPIC));
        assert_eq!(first.mask(ANTHROPIC, "x").text, ANTHROPIC);
    }

    #[test]
    fn a_key_read_in_part_is_hidden_from_its_head_or_by_its_body() {
        let line = "b3BlbnNzaC1rZXktdjEAAAAABG5vbmUAAAAEbm9uZQAAAAAAAAABAAAAMwAAAAtzc2gtZWQyNTUx";
        let head = format!("-----BEGIN OPENSSH PRIVATE KEY-----\n{line}\n");
        assert!(
            masked(&head).starts_with("‹secret 1: private key"),
            "{}",
            masked(&head)
        );
        let body = format!("{line}\n{line}\n{line}\n");
        assert!(
            masked(&body).starts_with("‹secret 1: key material"),
            "{}",
            masked(&body)
        );
        assert_eq!(
            masked(&format!("{line}\n")),
            format!("{line}\n"),
            "one line of base64 is no key"
        );
    }

    #[test]
    fn a_password_without_digits_counts_under_a_password_name_only() {
        assert_eq!(
            masked("PASSWORD=changeme\n"),
            "PASSWORD=‹secret 1: PASSWORD, 8 chars›\n"
        );
        assert_eq!(masked("API_KEY=changeme\n"), "API_KEY=changeme\n");
    }

    #[test]
    fn a_short_value_is_hidden_where_found_but_not_searched_after() {
        let masks = Masks::default();
        let text = masks.mask("postgres://u:int@h/db", "x").text;
        assert_eq!(
            text,
            "postgres://u:‹secret 1: password in a url, 3 chars›@h/db"
        );
        assert_eq!(masks.mask("let n: int = 1;", "y").text, "let n: int = 1;");
    }

    #[test]
    fn an_owners_pattern_hides_its_whole_match_whatever_its_groups() {
        let masks = Masks::default();
        masks.add_format("acme", compile(r"acme_(live|test)_[0-9a-f]{12}").unwrap());
        assert_eq!(
            masks.mask("k=acme_live_0123456789ab", "x").text,
            "k=‹secret 1: acme, 22 chars›"
        );
    }

    #[test]
    fn past_the_most_values_a_new_one_is_hidden_but_not_remembered() {
        let masks = Masks::default();
        let text: String = (0..MOST_LEARNED + 3)
            .map(|n| format!("TOKEN=a1b2c3d4e{n:05}\n"))
            .collect();
        let shown = masks.mask(&text, "x").text;
        assert!(!shown.contains("a1b2c3d4e"), "every one hidden");
        assert_eq!(masks.hidden().len(), MOST_LEARNED);
    }

    #[test]
    fn kinds_in_names_without_numbering() {
        let masks = Masks::default();
        assert_eq!(
            masks.kinds_in(&format!("{ANTHROPIC} {ANTHROPIC}")),
            ["anthropic key"]
        );
        assert!(masks.hidden().is_empty());
    }
}
