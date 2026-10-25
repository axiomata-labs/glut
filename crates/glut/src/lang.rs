//! Glut in eight languages (`docs/plans/glut-tui.md`, T48): the languages, which one is chosen, and the small parts
//! every catalog shares — named placeholders, plural forms, and a macro that declares a catalog so a text missing in
//! one language does not compile.
//!
//! Only what the owner reads is translated. What a model reads — the system prompt, tool descriptions, tool
//! results — stays English, and `glut run`'s JSON events never change.

use std::cell::Cell;
use std::collections::BTreeSet;
use std::fmt::Display;
use std::sync::atomic::{AtomicU8, Ordering};
use std::time::{Duration, Instant};

/// A language Glut speaks to the owner in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Lang {
    /// English, the source of every text.
    En,
    /// German.
    De,
    /// French.
    Fr,
    /// Italian.
    It,
    /// Spanish.
    Es,
    /// Chinese in its simplified script.
    Zh,
    /// Japanese.
    Ja,
    /// Brazilian Portuguese.
    Pt,
}

/// Which of a text's forms a count takes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Form {
    /// The count the language calls one: 1, in French and Portuguese 0 too.
    One,
    /// Every other count.
    Other,
}

impl Lang {
    /// Every language, English — the source — first.
    pub const ALL: [Lang; 8] = [
        Lang::En,
        Lang::De,
        Lang::Fr,
        Lang::It,
        Lang::Es,
        Lang::Zh,
        Lang::Ja,
        Lang::Pt,
    ];

    /// Its tag, as `/language` and `--lang` take it.
    pub fn code(self) -> &'static str {
        match self {
            Lang::En => "en",
            Lang::De => "de",
            Lang::Fr => "fr",
            Lang::It => "it",
            Lang::Es => "es",
            Lang::Zh => "zh",
            Lang::Ja => "ja",
            Lang::Pt => "pt",
        }
    }

    /// Its name in itself, as a list of languages shows it.
    pub fn name(self) -> &'static str {
        match self {
            Lang::En => "English",
            Lang::De => "Deutsch",
            Lang::Fr => "Français",
            Lang::It => "Italiano",
            Lang::Es => "Español",
            Lang::Zh => "简体中文",
            Lang::Ja => "日本語",
            Lang::Pt => "Português (Brasil)",
        }
    }

    /// The language a tag names: `de`, `de_DE.UTF-8`, `zh-Hans-CN`, `pt_BR`, a name like `deutsch`; `None` for one Glut
    /// does not speak and for `C` or `POSIX`, which name none. Any Chinese is the simplified script, any Portuguese
    /// Brazil's — nearer than English for their readers.
    pub fn parse(tag: &str) -> Option<Lang> {
        let tag = tag.trim().to_lowercase();
        let tag = tag.split(['.', '@']).next().unwrap_or_default();
        let base = tag.split(['_', '-']).next().unwrap_or_default();
        Lang::ALL.into_iter().find(|lang| {
            base == lang.code() || tag == lang.name().to_lowercase() || tag == lang.english_name()
        })
    }

    /// Its English name, which `parse` takes too.
    fn english_name(self) -> &'static str {
        match self {
            Lang::En => "english",
            Lang::De => "german",
            Lang::Fr => "french",
            Lang::It => "italian",
            Lang::Es => "spanish",
            Lang::Zh => "chinese",
            Lang::Ja => "japanese",
            Lang::Pt => "portuguese",
        }
    }

    /// The form a count of `n` takes, by CLDR's rules for whole numbers: French and Portuguese count 0 as one,
    /// Chinese and Japanese have one form for every count. (Spanish and Italian have a third form for whole millions,
    /// which no count Glut shows reaches.)
    pub fn form(self, n: u64) -> Form {
        let one = match self {
            Lang::Fr | Lang::Pt => n <= 1,
            Lang::Zh | Lang::Ja => false,
            _ => n == 1,
        };
        if one { Form::One } else { Form::Other }
    }

    fn index(self) -> u8 {
        Lang::ALL.iter().position(|lang| *lang == self).unwrap_or(0) as u8
    }
}

/// The language the owner reads now: English until one is set.
static CURRENT: AtomicU8 = AtomicU8::new(0);

thread_local! {
    /// A language one thread reads whatever the process's is: a test's own, so tests side by side do not mix.
    static THREAD: Cell<Option<Lang>> = const { Cell::new(None) };
}

/// The language the owner reads now — this thread's own where [`set_for_thread`] gave it one.
pub fn current() -> Lang {
    THREAD.with(Cell::get).unwrap_or_else(|| {
        Lang::ALL
            .get(usize::from(CURRENT.load(Ordering::Relaxed)))
            .copied()
            .unwrap_or(Lang::En)
    })
}

/// From now on the owner reads `lang`: every text taken after this is in it.
pub fn set(lang: Lang) {
    CURRENT.store(lang.index(), Ordering::Relaxed);
}

/// This thread reads `lang` whatever [`set`] says, `None` the process's again — for tests, each in its own language.
pub fn set_for_thread(lang: Option<Lang>) {
    THREAD.with(|thread| thread.set(lang));
}

/// The system's language: `LC_ALL`, else `LC_MESSAGES`, where the owner set one — as POSIX reads them, the first set
/// decides, and one Glut does not speak (or `C`) is English; on macOS then the first of the system's preferred
/// languages Glut speaks — a terminal sets `LANG` by itself (Ghostty to `en_US.UTF-8`), so on a Mac it says little;
/// then `LANG`; English where none fits.
pub fn detect() -> Lang {
    let env = |name: &str| std::env::var(name).ok().filter(|value| !value.is_empty());
    explicit(env("LC_ALL"), env("LC_MESSAGES"))
        .or_else(system)
        .or_else(|| env("LANG").and_then(|value| Lang::parse(&value)))
        .unwrap_or(Lang::En)
}

/// The language `LC_ALL` or else `LC_MESSAGES` sets: the first given decides, English where it names none Glut speaks.
fn explicit(all: Option<String>, messages: Option<String>) -> Option<Lang> {
    all.or(messages)
        .map(|value| Lang::parse(&value).unwrap_or(Lang::En))
}

/// How long the system is given to say its languages: a `defaults` that hangs must not hold Glut's start.
#[cfg(target_os = "macos")]
const SYSTEM_WAIT: Duration = Duration::from_millis(300);

/// The first of macOS' preferred languages Glut speaks, as `defaults read -g AppleLanguages` lists them — `None` when
/// it does not answer within [`SYSTEM_WAIT`].
#[cfg(target_os = "macos")]
fn system() -> Option<Lang> {
    use std::io::Read as _;
    let mut child = std::process::Command::new("/usr/bin/defaults")
        .args(["read", "-g", "AppleLanguages"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .ok()?;
    let since = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if since.elapsed() < SYSTEM_WAIT => {
                std::thread::sleep(Duration::from_millis(5));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let mut listed = String::new();
    child.stdout.take()?.read_to_string(&mut listed).ok()?;
    preferred(&listed)
}

#[cfg(not(target_os = "macos"))]
fn system() -> Option<Lang> {
    None
}

/// The first language Glut speaks in `defaults`' list: `(\n    "de-DE",\n    "en-US"\n)`.
fn preferred(listed: &str) -> Option<Lang> {
    listed
        .split([',', '\n', '(', ')'])
        .map(|tag| tag.trim().trim_matches('"'))
        .filter(|tag| !tag.is_empty())
        .find_map(Lang::parse)
}

/// `text` with each `{name}` in it replaced by its value; a name not given stays as it is written.
pub fn fill(text: &str, values: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let named = after.find('}').and_then(|close| {
            let name = &after[..close];
            values
                .iter()
                .find(|(given, _)| *given == name)
                .map(|(_, value)| (close, value.to_string()))
        });
        match named {
            Some((close, value)) => {
                out.push_str(&value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// The names of the placeholders in `text`: `{n}`, `{name}`.
pub fn placeholders(text: &str) -> BTreeSet<&str> {
    let mut names = BTreeSet::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        let name = &after[..close];
        if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            names.insert(name);
            rest = &after[close + 1..];
        } else {
            // A lone `{` is text: the name may begin after it, as `fill` reads it.
            rest = after;
        }
    }
    names
}

/// A text whose words follow a count: `{n}` in it stands for the count.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Plural {
    /// For the count the language calls one.
    pub one: &'static str,
    /// For every other count.
    pub other: &'static str,
}

impl Plural {
    /// The form `n` takes in the language read now, `{n}` filled.
    pub fn of(&self, n: u64) -> String {
        let text = match current().form(n) {
            Form::One => self.one,
            Form::Other => self.other,
        };
        fill(text, &[("n", &n)])
    }
}

/// One text of a catalog, as the tests that compare languages read it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Entry {
    Text(&'static str),
    Plural(Plural),
}

impl Entry {
    /// Its words, each form of a plural.
    pub fn texts(&self) -> Vec<&'static str> {
        match self {
            Entry::Text(text) => vec![text],
            Entry::Plural(plural) => vec![plural.one, plural.other],
        }
    }
}

impl From<&&'static str> for Entry {
    fn from(text: &&'static str) -> Self {
        Entry::Text(text)
    }
}

impl From<&Plural> for Entry {
    fn from(plural: &Plural) -> Self {
        Entry::Plural(*plural)
    }
}

/// Declares a catalog: a struct of texts (`&'static str` or [`Plural`]) that each language fills in a constant of its
/// own — a field left out does not compile — and `entries()`, every text by its name, for the tests.
#[macro_export]
macro_rules! catalog {
    (
        $(#[$meta:meta])*
        $vis:vis struct $name:ident {
            $( $(#[$field_meta:meta])* $field:ident : $ty:ty ),* $(,)?
        }
    ) => {
        $(#[$meta])*
        $vis struct $name {
            $( $(#[$field_meta])* pub $field: $ty, )*
        }

        impl $name {
            /// Every text by its field's name.
            pub fn entries(&self) -> Vec<(&'static str, $crate::lang::Entry)> {
                vec![$( (stringify!($field), $crate::lang::Entry::from(&self.$field)), )*]
            }
        }
    };
}

/// Where two catalogs of one struct part: a text whose placeholders differ from the English one, or that is empty —
/// `name: what`, for a test to list.
pub fn mismatches(
    english: &[(&'static str, Entry)],
    other: &[(&'static str, Entry)],
) -> Vec<String> {
    let mut found = Vec::new();
    for ((name, source), (_, translated)) in english.iter().zip(other) {
        let wanted: BTreeSet<&str> = source.texts().into_iter().flat_map(placeholders).collect();
        for text in translated.texts() {
            if text.trim().is_empty() {
                found.push(format!("{name}: empty"));
            }
            let given = placeholders(text);
            // A plural's one-form may leave `{n}` out ("one file"), never bring a name of its own.
            if !given.is_subset(&wanted) || (matches!(source, Entry::Text(_)) && given != wanted) {
                found.push(format!("{name}: {given:?} where English has {wanted:?}"));
            }
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_names_its_language_whatever_its_region_or_encoding() {
        assert_eq!(Lang::parse("de_DE.UTF-8"), Some(Lang::De));
        assert_eq!(Lang::parse("zh-Hans-CN"), Some(Lang::Zh));
        assert_eq!(Lang::parse("zh_TW"), Some(Lang::Zh));
        assert_eq!(Lang::parse("pt-PT"), Some(Lang::Pt));
        assert_eq!(Lang::parse("Deutsch"), Some(Lang::De));
        assert_eq!(Lang::parse("japanese"), Some(Lang::Ja));
        assert_eq!(Lang::parse("C"), None);
        assert_eq!(Lang::parse("POSIX"), None);
        assert_eq!(Lang::parse("nl_NL"), None);
        assert!(
            Lang::ALL
                .iter()
                .all(|lang| Lang::parse(lang.code()) == Some(*lang))
        );
    }

    #[test]
    fn the_system_list_gives_its_first_language_glut_speaks() {
        assert_eq!(
            preferred("(\n    \"nl-NL\",\n    \"de-DE\",\n    \"en-US\"\n)"),
            Some(Lang::De)
        );
        assert_eq!(preferred("(\n    \"nl-NL\"\n)"), None);
        assert_eq!(preferred(""), None);
    }

    #[test]
    fn an_explicit_locale_decides_and_one_glut_does_not_speak_is_english() {
        assert_eq!(
            explicit(Some("fr_FR.UTF-8".into()), Some("de_DE".into())),
            Some(Lang::Fr)
        );
        assert_eq!(
            explicit(Some("C".into()), Some("de_DE".into())),
            Some(Lang::En)
        );
        assert_eq!(explicit(None, Some("ja_JP".into())), Some(Lang::Ja));
        assert_eq!(explicit(None, None), None);
        assert_eq!(Lang::parse("FRANÇAIS"), Some(Lang::Fr));
    }

    #[test]
    fn a_threads_own_language_wins_over_the_processs() {
        set_for_thread(Some(Lang::Ja));
        assert_eq!(current(), Lang::Ja);
        set_for_thread(None);
        assert_ne!(current(), Lang::Ja);
    }

    #[test]
    fn counts_take_each_languages_form() {
        assert_eq!(Lang::En.form(1), Form::One);
        assert_eq!(Lang::En.form(0), Form::Other);
        assert_eq!(Lang::Fr.form(0), Form::One);
        assert_eq!(Lang::Fr.form(2), Form::Other);
        assert_eq!(Lang::Ja.form(1), Form::Other);
        assert_eq!(Lang::De.form(1), Form::One);
        assert_eq!(Lang::Pt.form(0), Form::One);
    }

    #[test]
    fn placeholders_are_filled_by_name_and_others_stay() {
        let text = "{n} files in {folder} — {unknown}";
        assert_eq!(
            fill(text, &[("n", &3), ("folder", &"src")]),
            "3 files in src — {unknown}"
        );
        assert_eq!(fill("a { b", &[]), "a { b");
        assert_eq!(
            placeholders(text).into_iter().collect::<Vec<_>>(),
            ["folder", "n", "unknown"]
        );
        assert!(placeholders("{ not one }").is_empty());
        assert_eq!(
            placeholders("x { y {n}").into_iter().collect::<Vec<_>>(),
            ["n"]
        );
    }

    #[test]
    fn translations_with_other_placeholders_are_found() {
        let english = [
            ("files", Entry::Text("{n} files")),
            (
                "panes",
                Entry::Plural(Plural {
                    one: "one pane",
                    other: "{n} panes",
                }),
            ),
        ];
        let good = [
            ("files", Entry::Text("{n} Dateien")),
            (
                "panes",
                Entry::Plural(Plural {
                    one: "ein Pane",
                    other: "{n} Panes",
                }),
            ),
        ];
        assert!(mismatches(&english, &good).is_empty());
        let bad = [
            ("files", Entry::Text("{count} Dateien")),
            (
                "panes",
                Entry::Plural(Plural {
                    one: "",
                    other: "{n} Panes",
                }),
            ),
        ];
        assert_eq!(mismatches(&english, &bad).len(), 2);
    }
}
