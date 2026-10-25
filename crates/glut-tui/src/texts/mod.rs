//! What the terminal UI says to the owner, in each of Glut's languages (`docs/plans/glut-tui.md`, T48): one constant
//! per language of the one struct [`Texts`], so a text missing in a language does not compile. English is the source;
//! another language is one file more.
//!
//! `tr!(field)` takes a text in the language read now, `tr!(field, name = value, …)` fills its placeholders. A text
//! is taken when it is shown: a `/language` changes what is drawn next, a note already in the stream keeps its words.

mod de;
mod en;
mod es;
mod fr;
mod it;
mod ja;
mod pt;
mod zh;

use glut::lang::{Lang, Plural};

glut::catalog! {
    /// Every text of the terminal UI.
    pub struct Texts {
        /// `/language` changed it: `{name}` the language's own name.
        language_set: &'static str,
        /// `/language` kept it for this run only: `{name}`, `{reason}` why it could not be kept.
        language_unkept: &'static str,
        /// `/language` with a word that names none: `{name}`, `{known}` the codes there are.
        language_unknown: &'static str,
        /// The list `/language` opens.
        language_title: &'static str,
        /// Beside the language read now in that list.
        language_now: &'static str,
        /// How many panes: `{n}`.
        panes_count: Plural,
        /// The help's topics, in `commands::Topic::ALL`'s order.
        topics: &'static [&'static str],
        /// What each `/` command does, by its name — every command of `commands::COMMANDS`, in its order; English's
        /// are its own words there.
        commands: &'static [(&'static str, &'static str)],
        /// What each key does, by the key as written — every key of `commands::KEYS`, in its order.
        keys: &'static [(&'static str, &'static str)],
        /// `/help`'s heading of the commands.
        help_commands: &'static str,
        /// `/help`'s heading of the keys: `{legend}` what the key symbols stand for.
        help_keys: &'static str,
        /// `/help`'s line of what is neither a command nor a key.
        help_also: &'static str,
        /// `/help`'s line on the vi keys.
        help_vim: &'static str,
        /// What the key symbols stand for, by the names the system gives the keys.
        legend: &'static str,
    }
}

/// The language the owner gave this run — `glut --lang`, or a `/language` — whatever is kept or detected.
static FORCED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// `glut --lang <code>` or `/language`: the owner reads `lang` for the rest of this run; a new pane or session does
/// not take another, and a switch to another project carries it.
pub fn force(lang: Lang) {
    glut::lang::set(lang);
    FORCED.store(true, std::sync::atomic::Ordering::Relaxed);
}

/// The language the owner gave this run.
pub fn forced() -> Option<Lang> {
    FORCED
        .load(std::sync::atomic::Ordering::Relaxed)
        .then(glut::lang::current)
}

/// The language a session starts in, set as the one read now: see [`resolve`]. The system is asked once a run.
pub fn apply(kept: Option<&str>, host: Option<Lang>) {
    static DETECTED: std::sync::OnceLock<Lang> = std::sync::OnceLock::new();
    // A test reads English unless it sets another itself: never the language of the machine it runs on.
    let detect = || {
        if cfg!(test) {
            Lang::En
        } else {
            *DETECTED.get_or_init(glut::lang::detect)
        }
    };
    if let Some(lang) = resolve(forced(), kept, host, detect) {
        glut::lang::set(lang);
    }
}

/// The language to start in: none to set where the owner gave one for this run; else the one `/language` kept, else
/// the host's, else the system's.
fn resolve(
    forced: Option<Lang>,
    kept: Option<&str>,
    host: Option<Lang>,
    detect: impl FnOnce() -> Lang,
) -> Option<Lang> {
    if forced.is_some() {
        return None;
    }
    Some(kept.and_then(Lang::parse).or(host).unwrap_or_else(detect))
}

/// The texts of `lang`.
pub fn of(lang: Lang) -> &'static Texts {
    match lang {
        Lang::En => &en::TEXTS,
        Lang::De => &de::TEXTS,
        Lang::Fr => &fr::TEXTS,
        Lang::It => &it::TEXTS,
        Lang::Es => &es::TEXTS,
        Lang::Zh => &zh::TEXTS,
        Lang::Ja => &ja::TEXTS,
        Lang::Pt => &pt::TEXTS,
    }
}

/// The texts of the language read now.
pub fn t() -> &'static Texts {
    of(glut::lang::current())
}

/// A text in the language read now — with `name = value` pairs, its placeholders filled. A debug build checks that
/// the names given are the text's own: a name misspelt would show as `{name}`.
macro_rules! tr {
    ($field:ident) => {{
        let text: &'static str = $crate::texts::t().$field;
        debug_assert!(glut::lang::placeholders(text).is_empty(), "{}: placeholders not filled", stringify!($field));
        text
    }};
    ($field:ident, $($name:ident = $value:expr),+ $(,)?) => {{
        let text: &'static str = $crate::texts::t().$field;
        debug_assert_eq!(
            glut::lang::placeholders(text),
            [$(stringify!($name)),+].into_iter().collect::<std::collections::BTreeSet<_>>(),
            "{}",
            stringify!($field)
        );
        glut::lang::fill(text, &[$((stringify!($name), &$value as &dyn std::fmt::Display)),+])
    }};
}

/// What a translation must keep of its English source, for the tests of every catalog.
#[cfg(test)]
pub(crate) mod check {
    /// The field's own words, which no language translates (T48's glossary).
    const KEPT: [&str; 11] = [
        "Gate", "Pane", "Skill", "Hook", "MCP", "Token", "Worktree", "Commit", "Glut", "Wache",
        "Glimm",
    ];

    /// The marks a key or a command is written with: a word holding one is written the same in every language.
    const KEY_MARKS: [char; 13] = [
        '⌃', '⇧', '⌥', '⏎', '⇥', '⎋', '⌫', '←', '→', '↑', '↓', '⌘', '@',
    ];

    /// Whether `term` stands in `text` as a word of its own (a plural `s` let be): "pane" in "2 panes", not in "panel".
    fn holds(text: &str, term: &str) -> bool {
        let text = text.to_lowercase();
        let term = term.to_lowercase();
        text.match_indices(&term).any(|(at, _)| {
            let before = text[..at].chars().next_back();
            let mut after = text[at + term.len()..].chars();
            let next = match after.next() {
                Some('s') => after.next(),
                other => other,
            };
            !before.is_some_and(char::is_alphabetic) && !next.is_some_and(char::is_alphabetic)
        })
    }

    /// What `english` holds that `translated` lost: a glossary word, a `/command`, a `--flag`, a `<name>`, a key.
    pub fn kept_in(english: &str, translated: &str) -> Vec<String> {
        let mut lost: Vec<String> = KEPT
            .iter()
            .filter(|term| holds(english, term) && !holds(translated, term))
            .map(|term| (*term).to_owned())
            .collect();
        let trim = |word: &'_ str| {
            word.trim_matches(|c: char| "(),;:.…\"“”".contains(c))
                .to_owned()
        };
        let raw: Vec<&str> = english.split_whitespace().collect();
        let words: Vec<String> = raw.iter().map(|word| trim(word)).collect();
        let mut at = 0;
        while at < words.len() {
            let word = &words[at];
            let key = word.chars().any(|c| KEY_MARKS.contains(&c));
            // A key's second part (`⌃X 0`, `⌃X A`, `⌃X }`) belongs to it — not a word after a comma or a closing mark.
            let bare = raw[at].trim_start_matches(['(', '“', '"']) == word.as_str();
            let whole = match words.get(at + 1) {
                Some(next)
                    if key
                        && bare
                        && next.chars().count() == 1
                        && next.chars().all(|c| {
                            c.is_ascii_uppercase() || c.is_ascii_digit() || "{}[]".contains(c)
                        }) =>
                {
                    at += 1;
                    format!("{word} {next}")
                }
                _ => word.clone(),
            };
            let fixed = key
                || (word.len() > 1
                    && (word.starts_with('/') || word.starts_with("--") || word.starts_with('<')));
            if fixed && !translated.contains(&whole) {
                lost.push(whole);
            }
            at += 1;
        }
        lost
    }
}

#[cfg(test)]
mod tests {
    use super::check::kept_in;
    use super::*;

    #[test]
    fn every_language_has_each_text_with_the_english_placeholders() {
        let english = of(Lang::En).entries();
        for lang in Lang::ALL {
            let found = glut::lang::mismatches(&english, &of(lang).entries());
            assert!(found.is_empty(), "{}: {found:#?}", lang.code());
        }
    }

    #[test]
    fn a_lost_command_key_flag_or_term_is_found() {
        let english = "close this pane — /todo <text> adds one (⌃X 0; /init --migrate)";
        assert!(
            kept_in(
                english,
                "ce pane — /todo <text> en ajoute un (⌃X 0 ; /init --migrate)"
            )
            .is_empty()
        );
        assert_eq!(
            kept_in(
                english,
                "dieses Fenster — /todo <Text> fügt hinzu (⌃X ; /init)"
            ),
            ["Pane", "<text>", "⌃X 0", "--migrate"]
        );
    }

    #[test]
    fn the_fields_own_words_stay_in_every_language() {
        let english = of(Lang::En).entries();
        for lang in Lang::ALL {
            for ((name, source), (_, translated)) in english.iter().zip(of(lang).entries()) {
                for (source, text) in source.texts().into_iter().zip(translated.texts()) {
                    let lost = kept_in(source, text);
                    assert!(
                        lost.is_empty(),
                        "{}: {name} drops {lost:?}: {text}",
                        lang.code()
                    );
                }
            }
        }
    }

    #[test]
    fn a_text_follows_the_language_read_now() {
        glut::lang::set_for_thread(Some(Lang::De));
        assert_eq!(tr!(language_set, name = "Deutsch"), "Sprache: Deutsch");
        assert_eq!(t().panes_count.of(1), "1 Pane");
        glut::lang::set_for_thread(Some(Lang::Fr));
        assert_eq!(t().panes_count.of(0), "0 pane");
        glut::lang::set_for_thread(Some(Lang::En));
        assert_eq!(t().panes_count.of(2), "2 panes");
        glut::lang::set_for_thread(None);
    }

    #[test]
    fn the_language_given_for_the_run_wins_then_the_kept_then_the_hosts_then_the_systems() {
        let system = || Lang::Ja;
        assert_eq!(
            resolve(Some(Lang::Fr), Some("de"), Some(Lang::It), system),
            None
        );
        assert_eq!(
            resolve(None, Some("de"), Some(Lang::It), system),
            Some(Lang::De)
        );
        assert_eq!(
            resolve(None, Some("odd"), Some(Lang::It), system),
            Some(Lang::It)
        );
        assert_eq!(resolve(None, None, None, system), Some(Lang::Ja));
    }
}
