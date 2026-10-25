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
        /// A background agent holds a message for the owner: `{name}`.
        holds_message: &'static str,
        /// A background agent asks the owner: `{name}`.
        agent_asks: &'static str,
        /// Another pane asks the owner: `{n}` its number.
        pane_asks: &'static str,
        /// A pane by its number: `{n}`.
        pane_named: &'static str,
        /// The frame of `/peek`: `{name}` the agent.
        peek_title: &'static str,
        /// What a review covers: the whole session.
        review_session: &'static str,
        /// What a review covers: the last turn.
        review_turn: &'static str,
        /// The review's frame: `{scope}` one of the two above, `{since}` the message it starts before, `{done}` of
        /// `{all}` files.
        review_title: &'static str,
        /// The plan's frame while Glut waits for the owner's word on it.
        plan_asking_title: &'static str,
        /// The plan's frame, only to read.
        plan_title: &'static str,
        /// An image shown whole: `{label}` its line.
        picture_title: &'static str,
        /// An image that cannot be drawn: `{n}` its number.
        picture_gone: &'static str,
        /// When a steering message is taken in.
        steer_before: &'static str,
        /// When a queued message goes, while Glut works.
        queued_after: &'static str,
        /// A queued message while Glut rests.
        queued_send: &'static str,
        /// Waiting messages beyond those shown: `{n}`.
        more_waiting: &'static str,
        /// The first start's hint to the keys and commands.
        welcome_keys: &'static str,
        /// The first start's hint to the modes.
        welcome_modes: &'static str,
        /// The first start's hint to `/init` where the folder has no guide.
        welcome_init: &'static str,
        /// A pinned project with no session yet.
        pinned: &'static str,
        /// Under the welcome's projects.
        projects_pick: &'static str,
        /// Rows below while the stream is scrolled up: `{n}`.
        scroll_more: &'static str,
        /// An ask's frame from a subagent: `{name}` it, `{tool}` the tool.
        agent_asks_tool: &'static str,
        /// An ask's frame: `{tool}` the tool.
        glut_asks_tool: &'static str,
        /// An ask's first choice.
        allow_once: &'static str,
        /// An ask's choice of a scope for the session: `{scope}`, drawn in bold where it stands.
        allow_scope: &'static str,
        /// An ask's last choice.
        refuse: &'static str,
        /// Rows of a long target left out: `{n}`.
        more_rows: &'static str,
        /// The search found nothing.
        no_match: &'static str,
        /// The search's match shown: `{at}` of `{count}`.
        match_of: &'static str,
        /// The search's matches: `{count}`.
        matches: &'static str,
        /// Before the search's query.
        search_label: &'static str,
        /// The footer's count while the screen holds still: `{n}` seconds.
        working_secs: &'static str,
        /// The footer while the input is a `!!` command.
        hint_shell_alone: &'static str,
        /// The footer while the input is a `!` command.
        hint_shell: &'static str,
        /// The footer while the search is open.
        hint_search: &'static str,
        /// The footer while the plan waits for the owner's word.
        hint_plan: &'static str,
        /// The footer while an ask with scopes is open: `{keys}` their keys.
        hint_ask_scopes: &'static str,
        /// The footer while an ask without scopes is open.
        hint_ask: &'static str,
        /// The footer in the reading mode.
        hint_reading: &'static str,
        /// The footer over a view only to read.
        hint_scroll: &'static str,
        /// The footer over a subagent's own stream.
        hint_agent: &'static str,
        /// The footer while a list is open.
        hint_picker: &'static str,
        /// The footer while Glut works.
        hint_working: &'static str,
        /// The footer while Glut rests.
        hint_usual: &'static str,
        /// A pane without the keys whose session asks.
        hint_other_asks: &'static str,
        /// The Agents tab's heading of the agents in the background.
        in_background: &'static str,
        /// Under the agents in the background.
        background_keys: &'static str,
        /// The Agents tab with no subagent yet.
        no_subagent: &'static str,
        /// The project's to-dos cannot be read: `{name}` the file, `{fault}` why.
        todos_unreadable: &'static str,
        /// The project's to-dos file holds none: `{name}` the file.
        todos_nothing: &'static str,
        /// The project has no to-dos file yet: `{name}` the file.
        todos_none: &'static str,
        /// The Plan tab with no plan and no task list.
        no_plan: &'static str,
        /// The task list's count: `{done}` of `{all}`.
        done_of: &'static str,
        /// The panel's keys with its focus: the tabs.
        keys_tabs: &'static str,
        /// The panel's keys in the Agents list.
        keys_agents: &'static str,
        /// The panel's keys in the To-dos list.
        keys_todos: &'static str,
        /// The panel's keys with its focus elsewhere.
        keys_focus: &'static str,
        /// The shown tab's key: `{key}`.
        key_hides: &'static str,
        /// The frame of the command suggestions.
        commands_title: &'static str,
        /// The frame of the Keys sheet over the stream.
        keys_title: &'static str,
        /// A model list filtered to the free ones.
        free_only: &'static str,
        /// A model list's bottom line: `{sort}` how it is sorted, `{free}` the filter or nothing.
        sorted_by: &'static str,
        /// A list entry that cannot be used here: `{detail}` what it says.
        not_usable: &'static str,
        /// A list where nothing fits what was typed.
        nothing_matches: &'static str,
        /// The side panel's tabs: Plan, Gate, Meter, Agents, To-dos, Keys — in this order.
        tab_names: &'static [&'static str],
        /// The Meter tab's heading of the context window.
        meter_context: &'static str,
        /// How full the window is: `{used}` of `{size}`, `{share}` percent.
        meter_of: &'static str,
        /// Where the session compacts itself: `{at}` tokens.
        meter_compacts_at: &'static str,
        /// The session compacts only when told.
        meter_compacts_manual: &'static str,
        /// Where the window's size came from: `{source}`.
        meter_window: &'static str,
        /// A window size the owner set in Glut.
        window_set_here: &'static str,
        /// A window size from the provider's model list.
        window_from_list: &'static str,
        /// Tokens used where the window is unknown: `{used}`.
        meter_used: &'static str,
        /// The window's size is unknown.
        meter_window_unknown: &'static str,
        /// The Meter tab's heading of the session's totals.
        meter_session: &'static str,
        /// The session's tokens: `{input}` in, `{cached}` cached, `{output}` out.
        meter_in_out: &'static str,
        /// The session's cost: `{cost}`.
        meter_price: &'static str,
        /// No price for the model.
        meter_no_price: &'static str,
        /// The effort the model runs at: `{effort}`.
        meter_effort: &'static str,
        /// The run's budget: `{budget}`.
        meter_budget: &'static str,
        /// Glut lean spared nothing yet.
        meter_lean_none: &'static str,
        /// What Glut lean spared this session: `{chars}` characters, `{calls}` calls, `{share}` percent.
        meter_lean_session: &'static str,
        /// What Glut lean spared over every run: `{chars}`, `{calls}`.
        meter_lean_all: &'static str,
        /// The Meter tab's heading of the turns.
        meter_turns: &'static str,
        /// No turn yet.
        meter_none_yet: &'static str,
        /// One turn's tokens: `{input}`, `{output}`, `{cost}` its cost or nothing.
        meter_turn_in_out: &'static str,
        /// One turn's cache, calls and tools: `{cached}`, `{calls}`, `{tools}`.
        meter_turn_detail: &'static str,
        /// How long the first token took: `{secs}` seconds.
        meter_first_token: &'static str,
        /// The Meter tab's heading of the tools.
        meter_tools: &'static str,
        /// No tool called yet.
        meter_none_called: &'static str,
        /// Calls of a tool that failed: `{n}`.
        meter_failed: &'static str,
        /// Entries of a list beyond those shown: `{n}`.
        more_listed: &'static str,
        /// The Gate tab: commands run in the sandbox.
        gate_sandbox_on: &'static str,
        /// The Gate tab: the system has no sandbox.
        gate_sandbox_none: &'static str,
        /// The Gate tab: the sandbox is off.
        gate_sandbox_off: &'static str,
        /// The Gate tab: the mode, `{mode}` its name.
        gate_mode: &'static str,
        /// The Gate tab: how many rules of each kind, `{allow}`, `{ask}`, `{deny}` — the kinds' own words stay.
        gate_rules: &'static str,
        /// The Gate tab: this turn's refusals, `{refused}` of `{total}`, the same call `{same}` of `{repeat}`.
        gate_refused: &'static str,
        /// The Gate tab's heading of the "always" answers.
        gate_from_now: &'static str,
        /// The Gate tab's heading of the hosts.
        gate_network: &'static str,
        /// The Gate tab with no host reached yet.
        gate_no_hosts: &'static str,
        /// A host reached `{reached}` times and refused `{refused}` times.
        gate_host_refused: &'static str,
        /// The Gate tab's heading of the judgements.
        gate_judgements: &'static str,
        /// The Gate tab with no call judged yet.
        gate_no_judgements: &'static str,
        /// The project's to-dos changed under Glut: `{name}` the file.
        todos_changed: &'static str,
        /// The project's to-dos cannot be written: `{name}`, `{fault}`.
        todos_unwritable: &'static str,
        /// A list scrolled to its top: `{below}` rows below.
        rows_below: &'static str,
        /// A list scrolled to its end: `{above}` rows above.
        rows_above: &'static str,
        /// A list scrolled between: `{above}` rows above, `{below}` below.
        rows_around: &'static str,
        /// What Space does to the done items shown.
        todos_fold: &'static str,
        /// What Space does to the done items folded.
        todos_show: &'static str,
        /// The done items' row: `{count}` of them, `{action}` what Space does.
        todos_done: &'static str,
        /// A path that is no plain file.
        no_plain_file: &'static str,
        /// A file over the limit: `{kb}` kilobytes.
        larger_than_kb: &'static str,
        /// A subagent's state: at work.
        agent_running: &'static str,
        /// A subagent's state: finished.
        agent_done: &'static str,
        /// A subagent's state: it failed or was stopped.
        agent_stopped: &'static str,
        /// A subagent's state: the gate refused it.
        agent_refused: &'static str,
        /// A subagent's view, at the first level.
        subagent: &'static str,
        /// A subagent's view below the first: `{depth}` its level.
        subagent_level: &'static str,
        /// A subagent's view's frame: `{level}`, `{task}`, `{state}`, `{calls}`, `{input}` and `{output}` thousands of
        /// tokens.
        subagent_title: &'static str,
        /// A subagent's view before it has done anything.
        subagent_starting: &'static str,
        /// A subagent's view of a session opened again.
        subagent_not_kept: &'static str,
        /// A subagent's view's keys.
        subagent_keys: &'static str,
        /// A subagent's view's keys, after its own subagents by number.
        subagent_keys_nested: &'static str,
        /// What a subagent spent: `{calls}`, `{tokens}`, `{model}` its model or nothing.
        agent_spent: &'static str,
        /// The plan waits for the owner.
        plan_waiting: &'static str,
        /// The plan approved: `{mode}` the mode it runs in.
        plan_approved: &'static str,
        /// The plan sent back: `{note}` the owner's note.
        plan_sent_back_note: &'static str,
        /// The plan sent back without a note.
        plan_sent_back: &'static str,
        /// The plan left undecided.
        plan_dropped: &'static str,
        /// The plan handed to another pane: `{pane}`.
        plan_handed: &'static str,
        /// The heading of the plan's task list.
        task_list: &'static str,
        /// Above the note that sends a plan back.
        plan_why: &'static str,
        /// The plan dialog's choice to approve it.
        plan_approve: &'static str,
        /// A mode offered while no sandbox guards it: `{mode}`.
        plan_no_sandbox: &'static str,
        /// The plan dialog's choice to build in another pane: `{pane}`.
        plan_build_in: &'static str,
        /// The plan dialog's choice to send it back with a note.
        plan_send_back_note: &'static str,
        /// The plan dialog's choice to send it back.
        plan_send_back: &'static str,
        /// Under the plan dialog's choices: `{mode}` the mode it runs in once approved.
        plan_carries_out: &'static str,
        /// The keys of a view only to read, after its state.
        hint_scroll_short: &'static str,
        /// The plan's rows beyond the Plan tab's: `{n}`.
        plan_more_lines: &'static str,
        /// A file in the review whose name git quotes.
        review_quoted_name: &'static str,
        /// A binary file in the review.
        review_binary: &'static str,
        /// A change the diff cut short.
        review_cut: &'static str,
        /// A hunk header the review cannot read.
        review_bad_header: &'static str,
        /// A file whose mode alone changed.
        review_mode_only: &'static str,
        /// A hunk that cannot be put back: it holds bytes that are no text.
        review_not_text: &'static str,
        /// A hunk that cannot be put back: the file changed since.
        review_changed_since: &'static str,
        /// A review with no change in its scope.
        review_nothing: &'static str,
        /// A file the owner marked reviewed.
        review_reviewed: &'static str,
        /// A file the change created.
        review_new: &'static str,
        /// A file the change deleted.
        review_gone: &'static str,
        /// A hunk the owner accepted.
        review_accepted: &'static str,
        /// A hunk the owner commented on.
        review_commented: &'static str,
        /// A hunk's head: lines `{from}` to `{to}`.
        review_lines: &'static str,
        /// Before the reason a hunk is rejected.
        review_why: &'static str,
        /// Before a comment on a hunk.
        review_comment: &'static str,
        /// The review's keys.
        review_keys: &'static str,
        /// The side panel's tabs as the panel's key hints name them (`⌃B plan`), in `tab_names`' order.
        tab_words: &'static [&'static str],
        /// How a model list is sorted: by name, newest, cheapest, largest window — in this order.
        sort_names: &'static [&'static str],
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
    fn every_language_names_each_tab_and_sort() {
        for lang in Lang::ALL {
            let texts = of(lang);
            assert_eq!(texts.tab_names.len(), 6, "{}", lang.code());
            assert_eq!(texts.tab_words.len(), 6, "{}", lang.code());
            assert_eq!(texts.sort_names.len(), 4, "{}", lang.code());
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
