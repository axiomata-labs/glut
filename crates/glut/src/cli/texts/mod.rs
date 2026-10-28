//! What the headless commands say to the owner on the terminal, in each of Glut's languages (`docs/plans/glut-tui.md`,
//! T48): `glut providers`, `glut models`, `glut resume`, `glut undo`, `glut gain` and the notes of `glut run` on
//! stderr. `glut run`'s JSON lines, its errors (an `Event::Error`) and whatever a model reads stay English.

mod de;
mod en;
mod es;
mod fr;
mod it;
mod ja;
mod pt;
mod zh;

use crate::lang::{Lang, Plural};

crate::catalog! {
    /// Every text of the headless commands.
    pub struct Texts {
        /// `$HOME` missing or relative.
        home_unset: &'static str,
        /// A folder that cannot be used: `{path}`, `{err}`.
        cannot_use: &'static str,
        /// `glut gain` without a home.
        gain_no_home: &'static str,
        /// `glut gain` cannot read its file: `{err}`.
        gain_unreadable: &'static str,
        /// What a run starts with: `{provider}`, `{model}`.
        run_provider: &'static str,
        /// The model is not in its provider's list: `{model}`, `{provider}`.
        model_unlisted: &'static str,
        /// The provider's list could not be had: `{provider}`, `{reason}`.
        no_model_list: &'static str,
        /// A run's name, to resume it by: `{run}`.
        run_started: &'static str,
        /// A skill that cannot be used: `{note}` (the skill's own words, English).
        skill_left_out: &'static str,
        /// `--max-usd` without prices: `{model}`.
        max_usd_no_prices: &'static str,
        /// Beside a run that still runs in `glut resume --list`.
        resume_running: &'static str,
        /// `glut resume` without a run.
        resume_name_run: &'static str,
        /// `glut resume` with a run that is not unfinished: `{run}`.
        resume_no_run: &'static str,
        /// `glut resume --discard` done: `{run}`.
        resume_given_up: &'static str,
        /// A journal not written by `glut run`: `{run}`, `{err}`.
        resume_not_glut_run: &'static str,
        /// What a resume goes on with: `{run}`, `{flags}` the flags as written.
        resume_with: &'static str,
        /// In those flags, for a run on the default provider.
        resume_default_provider: &'static str,
        /// In those flags, for a run on its provider's model.
        resume_providers_model: &'static str,
        /// In those flags, for a value the settings give.
        resume_from_settings: &'static str,
        /// One run of `glut undo --list`: `{run}`, `{steps}`.
        undo_steps: &'static str,
        /// `glut undo` with nothing to put back: `{path}`.
        undo_no_snapshots: &'static str,
        /// `glut undo` done, before a step: `{path}`, `{step}`, `{run}`, `{commit}`.
        undo_back_step: &'static str,
        /// `glut undo` done, before a whole run: `{path}`, `{run}`, `{commit}`.
        undo_back: &'static str,
        /// `glut undo`: a folder beside the working one (T60c) not put back, `{what}` which and why.
        undo_beside_missed: &'static str,
        /// `glut models` without a provider.
        models_no_provider: &'static str,
        /// A provider's list, its head: `{id}`, `{shown}` of `{all}`, `{when}` it was fetched.
        models_fetched: &'static str,
        /// How many models a list holds: `{n}`.
        models_count: Plural,
        /// `glut models` when no list could be read.
        models_none_read: &'static str,
        /// The last column of a model that reasons.
        models_reasons: &'static str,
        /// `glut providers list` without a provider.
        providers_none: &'static str,
        /// Where a run takes the key: `{name}` the variable.
        key_from_env: &'static str,
        /// Where a run takes the key: `{place}` the store.
        key_in_store: &'static str,
        /// A provider that needs no key.
        key_not_needed: &'static str,
        /// A provider signed in to with an account (PV4).
        key_signed_in: &'static str,
        /// A catalog provider the owner signs in to.
        catalog_sign_in: &'static str,
        /// Above the sign-in's address: `{host}`.
        plan_sign_in_at: &'static str,
        /// Above OpenRouter's sign-in address: `{host}`.
        key_sign_in_at: &'static str,
        /// A sign-in done: `{id}`, `{who}` the account.
        plan_signed_in: &'static str,
        /// `login`/`logout` on a provider with a key: `{id}`.
        not_a_sign_in: &'static str,
        /// `logout` with nothing signed in: `{id}`.
        plan_out_nothing: &'static str,
        /// A sign-out done and revoked: `{id}`.
        plan_out_revoked: &'static str,
        /// A sign-out done here, its revocation unconfirmed: `{id}`, `{reason}`.
        plan_out_unconfirmed: &'static str,
        /// A provider without a key: `{reason}`.
        key_missing: &'static str,
        /// A provider without a model of its own.
        no_default_model: &'static str,
        /// In the catalog, a provider that needs no key.
        catalog_no_key: &'static str,
        /// In the catalog, a provider that needs a key with no variable of its own.
        catalog_key: &'static str,
        /// In the catalog, what an OpenAI-compatible provider needs.
        catalog_custom_sign_in: &'static str,
        /// In the catalog, what an OpenAI-compatible provider is.
        catalog_custom_note: &'static str,
        /// `--keyless` on a catalog provider.
        keyless_only_custom: &'static str,
        /// A key stored: `{id}`, `{place}`.
        key_kept: &'static str,
        /// A key's sign-in replaced a key kept before: `{id}`.
        key_replaced: &'static str,
        /// No key typed where the account keeps one: `{id}`.
        key_shared: &'static str,
        /// After the name of the provider added, when it is the default.
        the_default: &'static str,
        /// A provider added: `{id}`, `{default}` the text above or nothing.
        provider_added: &'static str,
        /// Asks for a key the account may already keep: `{id}`.
        key_prompt_shared: &'static str,
        /// Asks for a key: `{id}`.
        key_prompt: &'static str,
        /// The key could not be read: `{err}`.
        key_unreadable: &'static str,
        /// A key too long: `{max}` bytes.
        key_too_long: &'static str,
        /// No key typed.
        no_key_given: &'static str,
        /// A key with spaces or control characters.
        key_odd: &'static str,
        /// Asks for the key that changes another one (T70): `{id}`.
        key_prompt_new: &'static str,
        /// A command that keeps a key or a sign-in, run from inside a command Glut runs.
        owners_only: &'static str,
        /// `glut providers key` with no terminal to type into.
        key_needs_terminal: &'static str,
        /// `key` on a provider the owner signs in to instead (T70): `{id}`.
        key_sign_in: &'static str,
        /// `key` on a provider that needs no key (T70): `{id}`.
        key_no_key_needed: &'static str,
        /// `key` while the key comes from the environment right now (T70): `{id}`, `{name}` the variable.
        key_env_now: &'static str,
        /// Before the small call that proves the new key (T70): `{id}`.
        key_checking: &'static str,
        /// The new key does not prove itself (T70): `{reason}`.
        key_check_failed: &'static str,
        /// Asked when the new key does not prove itself (T70): nothing typed keeps the old key.
        key_keep_anyway: &'static str,
        /// The new key kept, checked before it replaced the old one (T70): `{id}`, `{place}` the store.
        key_changed: &'static str,
        /// The old key kept, the new one not proven (T70): `{id}`.
        key_kept_old: &'static str,
        /// A provider that is not there: `{id}`.
        no_such_provider: &'static str,
        /// A provider removed: `{id}`.
        provider_removed: &'static str,
        /// One provider is left, and no default.
        one_left: &'static str,
        /// Several are left, and no default.
        no_default_now: &'static str,
        /// The default set: `{id}`.
        default_set: &'static str,
        /// `glut providers test`: the answer had no text.
        test_no_text: &'static str,
        /// `glut providers test` done: `{name}`, `{secs}`, `{text}`, `{input}` and `{output}` tokens.
        test_answered: &'static str,
        /// `glut mcp list` without a server.
        mcp_none: &'static str,
        /// A server the folder does not have: `{name}`.
        mcp_no_server: &'static str,
        /// Where a server comes from: the owner's file.
        mcp_origin_owner: &'static str,
        /// Where a server comes from: the project's file.
        mcp_origin_project: &'static str,
        /// A server Glut starts itself.
        mcp_kept_stdio: &'static str,
        /// A server signed in by a header of its entry.
        mcp_kept_header: &'static str,
        /// A server with a kept OAuth sign-in.
        mcp_kept_oauth: &'static str,
        /// A server with a kept token.
        mcp_kept_token: &'static str,
        /// A hosted server with nothing kept.
        mcp_kept_nothing: &'static str,
        /// `login` on a server Glut starts: `{name}`.
        mcp_stdio_no_login: &'static str,
        /// `login` on a server with its own Authorization header: `{name}`.
        mcp_header_login: &'static str,
        /// `login` on a project's server not approved: `{name}`.
        mcp_not_approved: &'static str,
        /// Asks for the secret of the owner's client: `{id}`.
        mcp_secret_prompt: &'static str,
        /// A server that answers without a sign-in: `{name}`.
        mcp_open: &'static str,
        /// A server with OAuth for registered clients only: `{name}`.
        mcp_registered_only: &'static str,
        /// Asks for a token: `{name}`, `{host}`.
        mcp_token_prompt: &'static str,
        /// No token typed.
        mcp_no_token: &'static str,
        /// Before the address of a sign-in: `{host}`, `{name}`.
        mcp_sign_in_at: &'static str,
        /// The browser was opened.
        mcp_browser_opened: &'static str,
        /// The browser was not opened: the owner opens the address, over SSH pastes it back.
        mcp_open_yourself: &'static str,
        /// Signed in and reached: `{name}`, `{count}` tools.
        mcp_signed_in: &'static str,
        /// Signed in, but the server does not start: `{name}`, `{reason}`.
        mcp_signed_in_failed: &'static str,
        /// `logout` with nothing kept: `{name}`.
        mcp_nothing_kept: &'static str,
        /// A typed token forgotten: `{name}`.
        mcp_out_token: &'static str,
        /// A grant revoked and forgotten: `{name}`.
        mcp_out_revoked: &'static str,
        /// A grant forgotten, the server offers no revocation: `{name}`.
        mcp_out_here: &'static str,
        /// A grant forgotten, the revocation failed: `{name}`, `{reason}`.
        mcp_out_failed: &'static str,
        /// `add` without a URL or command.
        mcp_add_what: &'static str,
        /// A server added: `{name}`.
        mcp_added: &'static str,
        /// `remove` of a server not in the owner's file: `{name}`.
        mcp_not_owners: &'static str,
        /// A server removed: `{name}`.
        mcp_removed: &'static str,
        /// A sign-in whose tokens come from another host: `{name}`, `{host}`.
        mcp_token_elsewhere: &'static str,
        /// Before the browser opens: the owner's Enter opens it, or a pasted address finishes.
        mcp_enter_opens: &'static str,
        /// `glut keychain list` with the Keychain shut; the programs follow, one a line.
        keychain_shut: &'static str,
        /// `glut keychain list` with the Keychain open to every command; the programs follow.
        keychain_open: &'static str,
        /// A program put on the list: `{program}`.
        keychain_added: &'static str,
        /// `add` of a program on the list already: `{program}`.
        keychain_listed_already: &'static str,
        /// A program taken off the list: `{program}`.
        keychain_removed: &'static str,
        /// `remove` of a program not on the list: `{program}`.
        keychain_not_listed: &'static str,
        /// `glut keychain open`.
        keychain_opened: &'static str,
        /// `glut keychain close`.
        keychain_closed: &'static str,
    }
}

/// The texts of one language.
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
    of(crate::lang::current())
}

/// A text in the language read now — with `name = value` pairs, its placeholders filled. A debug build checks that
/// the names given are the text's own.
macro_rules! say {
    ($field:ident) => {{
        let text: &'static str = $crate::cli::texts::t().$field;
        debug_assert!($crate::lang::placeholders(text).is_empty(), "{}: placeholders not filled", stringify!($field));
        text
    }};
    ($field:ident, $($name:ident = $value:expr),+ $(,)?) => {{
        let text: &'static str = $crate::cli::texts::t().$field;
        debug_assert_eq!(
            $crate::lang::placeholders(text),
            [$(stringify!($name)),+].into_iter().collect::<std::collections::BTreeSet<_>>(),
            "{}",
            stringify!($field)
        );
        $crate::lang::fill(text, &[$((stringify!($name), &$value as &dyn std::fmt::Display)),+])
    }};
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The words a translation must keep as written: flags, commands, `<names>`, paths and variables.
    fn kept(text: &str) -> Vec<String> {
        let words: Vec<&str> = text
            .split(|c: char| c.is_whitespace() || "():;,.?".contains(c))
            .filter(|word| !word.is_empty())
            .collect();
        let mut kept = Vec::new();
        for (at, word) in words.iter().enumerate() {
            if word.starts_with("--")
                || word.starts_with('<')
                || word.starts_with("~/")
                || word.contains("HOME")
            {
                kept.push((*word).to_owned());
            }
            if *word == "glut"
                && words
                    .get(at + 1)
                    .is_some_and(|next| next.chars().all(|c| c.is_ascii_lowercase()))
            {
                kept.push(format!("glut {}", words[at + 1]));
            }
        }
        kept
    }

    #[test]
    fn every_language_has_the_placeholders_and_keeps_the_syntax() {
        let english = of(Lang::En).entries();
        for lang in Lang::ALL.into_iter().filter(|lang| *lang != Lang::En) {
            let other = of(lang).entries();
            let found = crate::lang::mismatches(&english, &other);
            assert!(found.is_empty(), "{}: {found:?}", lang.code());
            for ((name, source), (_, translated)) in english.iter().zip(&other) {
                for (source, text) in source.texts().into_iter().zip(translated.texts()) {
                    for word in kept(source) {
                        assert!(
                            text.contains(&word),
                            "{} {name}: {word} lost in {text}",
                            lang.code()
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn a_text_is_said_in_the_language_read_now() {
        crate::lang::set_for_thread(Some(Lang::De));
        assert_eq!(
            say!(provider_removed, id = "x"),
            "Provider x entfernt, samt gespeichertem Schlüssel."
        );
        crate::lang::set_for_thread(None);
        assert_eq!(
            say!(provider_removed, id = "x"),
            "Provider x removed, with its stored key."
        );
    }
}
