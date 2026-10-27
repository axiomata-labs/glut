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
        /// The help box's title.
        help_title: &'static str,
        /// The help box's last entry: the keys and the rest.
        help_more: &'static str,
        /// What the help box's last entry holds.
        help_more_what: &'static str,
        /// The help box's keys while it lists the commands.
        help_list_keys: &'static str,
        /// The help box's keys while a command's page is open.
        help_page_keys: &'static str,
        /// Above the keys on a command's page.
        help_page_keys_heading: &'static str,
        /// `/help` with a word that names no command: `{name}`.
        help_unknown: &'static str,
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
        /// The footer while ⌃X waits for its second key (T71).
        hint_chord: &'static str,
        /// A second key ⌃X does not know (T71): `{key}` it.
        chord_unknown: &'static str,
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
        /// The session could not be saved: `{reason}`.
        session_unkept: &'static str,
        /// A session without a title opened: `{when}` it was last worked on.
        session_opened: &'static str,
        /// A session opened: `{when}`, `{title}`.
        session_opened_titled: &'static str,
        /// The reader mode's line: one turn ended and the next began.
        state_next_message: &'static str,
        /// The reader mode's line: work goes on after an answer.
        state_works_on: &'static str,
        /// The reader mode's line: work begins.
        state_works: &'static str,
        /// The reader mode's line: a question, `{tool}` and `{target}`.
        state_asks_tool: &'static str,
        /// The reader mode's line: a list waits for a choice.
        state_asks_choose: &'static str,
        /// The reader mode's line: the turn answered.
        state_done: &'static str,
        /// The reader mode's line: the turn failed.
        state_failed: &'static str,
        /// The reader mode's line: the turn was stopped.
        state_stopped: &'static str,
        /// F2 gave the mouse back to Glut.
        mouse_glut: &'static str,
        /// F2 gave the mouse to the terminal.
        mouse_terminal: &'static str,
        /// A turn cut off goes on.
        turn_resumed: &'static str,
        /// Under a plan whose event was cut.
        plan_head_only: &'static str,
        /// Above a file of Glut's own, whole, before it is written.
        glut_file_whole: &'static str,
        /// A turn the owner stopped.
        stopped: &'static str,
        /// A turn that ended early: `{reason}`.
        turn_ended: &'static str,
        /// Why a model's message is held: the pane runs wider now.
        held_wider: &'static str,
        /// The conversation was compacted: `{n}` messages.
        compacted: &'static str,
        /// The conversation could not be compacted: `{reason}`.
        not_compacted: &'static str,
        /// The handoff was written: `{path}`.
        handoff_written: &'static str,
        /// The handoff could not be written: `{reason}`.
        no_handoff: &'static str,
        /// A message the host refused, back in the input: `{reason}`.
        not_sent_back: &'static str,
        /// A message the host refused: `{reason}`.
        not_sent: &'static str,
        /// The cache keeping stopped: `{reason}`.
        warm_ended: &'static str,
        /// The title of the list when Glut asks for an agent's or a skill's model.
        pick_own_model: &'static str,
        /// A provider with no model chosen: `{provider}`.
        provider_no_model: &'static str,
        /// A `!cmd`'s output joins the next message.
        shell_to_model: &'static str,
        /// A step whose change is empty: `{turn}`.
        step_changed_nothing: &'static str,
        /// The block showing a step's change: `{turn}`.
        step_changes: &'static str,
        /// A change that cannot be shown: `{reason}`.
        no_change_to_show: &'static str,
        /// A change that cannot be reviewed: `{reason}`.
        no_change_to_review: &'static str,
        /// The files went back: `{snapshot}`, and what they were is kept as run `{kept}`.
        files_restored: &'static str,
        /// The files could not go back: `{reason}`.
        files_not_restored: &'static str,
        /// A model without prices: `{model}`.
        no_prices: &'static str,
        /// The model chosen could not be kept for the next start: `{reason}`.
        choice_unkept: &'static str,
        /// The model switched to: `{provider}`, `{model}`.
        model_now: &'static str,
        /// Why a turn ended: it answered.
        ended_answered: &'static str,
        /// Why a turn ended: its steps ran out.
        ended_steps: &'static str,
        /// Why a turn ended: its time ran out.
        ended_time: &'static str,
        /// Why a turn ended: the model's API failed.
        ended_failed: &'static str,
        /// Why a turn ended: too many refusals.
        ended_refused: &'static str,
        /// Why a turn ended: the budget is spent.
        ended_budget: &'static str,
        /// A paste that lost its mark to an edit.
        paste_lost: &'static str,
        /// Enter while an image is still being read.
        image_reading: &'static str,
        /// A command that waits for the turn: `{name}`.
        command_waits: &'static str,
        /// A `!cmd` while Glut works.
        shell_waits: &'static str,
        /// A message with images to a model that sees none: `{model}`.
        model_no_images: &'static str,
        /// A message with images while Glut works.
        images_wait: &'static str,
        /// Enter while Glut works: the message steers it.
        steer_taken: &'static str,
        /// Enter while Glut compacts or switches: the message waits.
        queued_free: &'static str,
        /// Tab while Glut works: the message waits for the turn's end.
        queued_turn: &'static str,
        /// ⌥↑ while the input holds a message.
        input_full: &'static str,
        /// ⌥↑ with nothing waiting.
        nothing_waits: &'static str,
        /// A `!cmd` in a session that keeps them off.
        no_shell: &'static str,
        /// Whose a `!cmd` is: the owner's, its output for Glut.
        shell_for_glut: &'static str,
        /// Whose a `!!cmd` is: the owner's alone.
        shell_alone: &'static str,
        /// The file list's title after `@`.
        files_title: &'static str,
        /// The file list while the folder is still walked: it starts with `files_title`.
        files_looking: &'static str,
        /// The file list of a large folder: it starts with `files_title`.
        files_truncated: &'static str,
        /// ⇧⇥ in a session whose mode is the host's.
        mode_locked: &'static str,
        /// The mode switched to plan — the mode's name stays.
        mode_plan: &'static str,
        /// The mode switched to ask.
        mode_ask: &'static str,
        /// The mode switched to build.
        mode_build: &'static str,
        /// The mode switched to auto, in the sandbox.
        mode_auto: &'static str,
        /// The mode switched to auto where there is no sandbox.
        mode_auto_unsandboxed: &'static str,
        /// The mode switched to don't ask.
        mode_dont_ask: &'static str,
        /// After a mode switched while a turn runs.
        mode_next_message: &'static str,
        /// While the conversation is compacted.
        compacting: &'static str,
        /// `/handoff` alone.
        handoff_usage: &'static str,
        /// While the handoff is written.
        handoff_writing: &'static str,
        /// A handoff taken in: `{path}` where it moved.
        handoff_read: &'static str,
        /// `/handoff` with a word it does not take: `{word}`.
        handoff_unknown: &'static str,
        /// `/projects` where the host picks the folder.
        projects_hosted: &'static str,
        /// `/plan` before any plan.
        no_plan_yet: &'static str,
        /// `/vim` with a word it does not take.
        vim_unknown: &'static str,
        /// Why the footer's joins are what they are: guessed from `{terminal}`.
        footer_guessed: &'static str,
        /// Why the footer is plain: an unknown terminal.
        footer_guessed_unknown: &'static str,
        /// A setting the owner picked.
        your_choice: &'static str,
        /// `/footer` alone: `{footer}` the joins, `{why}`.
        footer_now: &'static str,
        /// `/footer` with a word it does not take.
        footer_unknown: &'static str,
        /// `/show` alone.
        show_usage: &'static str,
        /// `/cache` alone: `{cache}` how it is kept now.
        cache_now: &'static str,
        /// Why full motion holds still.
        motion_held: &'static str,
        /// `/motion` alone: `{motion}`, `{still}` why it holds still or nothing.
        motion_now: &'static str,
        /// Links set clickable.
        links_clickable: &'static str,
        /// Links set plain.
        links_plain: &'static str,
        /// `/links` alone: `{links}` how they are drawn.
        links_now: &'static str,
        /// Images set to be drawn so: `{images}`.
        images_set: &'static str,
        /// `/images` alone: `{images}` how they are drawn.
        images_now: &'static str,
        /// `/reader` alone: `{reader}` on or off.
        reader_now: &'static str,
        /// `/thinking` with a word it does not take.
        thinking_unknown: &'static str,
        /// A command that is none: `{name}`.
        no_command: &'static str,
        /// A switch of model where the host fixes it.
        model_locked: &'static str,
        /// `/context` alone with a known window: `{used}` of `{window}` tokens, `{share}` percent, `{from}` where the
        /// window came from.
        context_known: &'static str,
        /// `/context` alone with no window known: `{used}` tokens, `{model}`.
        context_unknown: &'static str,
        /// What the guides cost each turn: `{tokens}`.
        guides_cost: &'static str,
        /// `/context` with a word that is no count: `{word}`.
        context_unknown_word: &'static str,
        /// `/context` set the window: `{model}`, `{tokens}`.
        context_set: &'static str,
        /// `/init` with a word it does not take.
        init_usage: &'static str,
        /// `/init`'s question where the guide is a CLAUDE.md.
        init_claude_title: &'static str,
        /// Its yes.
        init_claude_yes: &'static str,
        /// What its yes does.
        init_claude_yes_detail: &'static str,
        /// Its no.
        init_claude_no: &'static str,
        /// What its no does.
        init_claude_no_detail: &'static str,
        /// `/memory` in a session without a memory.
        no_memory: &'static str,
        /// A memory that keeps nothing yet: `{folder}`.
        memory_empty: &'static str,
        /// The list `/memory` opens: `{folder}`.
        memory_title: &'static str,
        /// `/memory` with a word it does not take.
        memory_usage: &'static str,
        /// A file of the memory that cannot be read: `{name}`.
        cannot_read: &'static str,
        /// `/todo clean` with nothing done: `{name}`.
        todos_nothing_done: &'static str,
        /// `/todo clean`'s question: `{done}` items, `{name}`.
        todos_drop_title: &'static str,
        /// Its yes.
        todos_drop_yes: &'static str,
        /// What its yes does.
        todos_drop_yes_detail: &'static str,
        /// Its no.
        todos_drop_no: &'static str,
        /// An item added: `{name}` the list, `{item}`.
        todos_added: &'static str,
        /// `/skill` where there is none.
        no_skills: &'static str,
        /// The list `/skill` opens.
        skills_title: &'static str,
        /// `/skill` with a name that is none: `{name}`, `{known}` those there are.
        no_skill: &'static str,
        /// Where a list names nothing.
        there_are_none: &'static str,
        /// Items dropped by `/todo clean`: `{n}`, `{name}`.
        todos_dropped: &'static str,
        /// `/todo open` with no list yet: `{name}`.
        todos_no_file: &'static str,
        /// The panel opened in a narrow window.
        panel_needs_room: &'static str,
        /// A token budget: `{left}` of `{max}`.
        budget_tokens_left: &'static str,
        /// A money budget: `{left}` of `{max}`.
        budget_money_left: &'static str,
        /// A money budget without prices: `{max}`.
        budget_uncounted: &'static str,
        /// `/gate` where no gate guards the session.
        no_gate: &'static str,
        /// Beside an "always" answer in `/gate`.
        gate_always_detail: &'static str,
        /// Beside a rule from the settings in `/gate`.
        gate_from_settings: &'static str,
        /// `/gate`'s title: `{mode}`, `{always}` how many "always" answers.
        gate_title: &'static str,
        /// No "always" answer yet.
        gate_no_always: &'static str,
        /// "Always" answers given: `{n}`.
        gate_always_given: &'static str,
        /// An "always" taken back: `{rule}`.
        taken_back: &'static str,
        /// A rule that was no "always": `{rule}`.
        not_always: &'static str,
        /// The thinking set hidden.
        thinking_hidden: &'static str,
        /// The thinking set folded.
        thinking_folded: &'static str,
        /// The thinking set open.
        thinking_open: &'static str,
        /// Beside a message to rewind to: files changed since.
        rewind_files_changed: &'static str,
        /// Beside a message to rewind to: no file changed since.
        rewind_no_file_changed: &'static str,
        /// Beside a message to rewind to: compacted since, files changed.
        rewind_compacted_files: &'static str,
        /// Beside a message to rewind to: compacted since, no file changed.
        rewind_compacted_nothing: &'static str,
        /// The rewind list with nothing to rewind to.
        rewind_none: &'static str,
        /// The rewind list's title.
        rewind_title: &'static str,
        /// Rewind choice: the conversation and the files.
        rewind_both: &'static str,
        /// What it does.
        rewind_both_detail: &'static str,
        /// Rewind choice: the conversation only.
        rewind_talk: &'static str,
        /// What it does.
        rewind_talk_detail: &'static str,
        /// Rewind choice: the files only.
        rewind_files: &'static str,
        /// What it does.
        rewind_files_detail: &'static str,
        /// Rewind choice: a fork.
        rewind_fork: &'static str,
        /// What it does.
        rewind_fork_detail: &'static str,
        /// The rewind's second question.
        rewind_how: &'static str,
        /// The rewind's second question where only the files can go back.
        rewind_how_files: &'static str,
        /// A rewind while Glut works.
        rewind_waits: &'static str,
        /// A rewind of the conversation that cannot be.
        rewind_compacted: &'static str,
        /// A rewind of the files without a snapshot.
        no_snapshot: &'static str,
        /// A rewind while the input holds text.
        rewind_input_full: &'static str,
        /// The conversation rewound.
        rewound: &'static str,
        /// A fork while Glut works.
        fork_waits: &'static str,
        /// A fork that cannot be.
        fork_compacted: &'static str,
        /// A fork made.
        forked: &'static str,
        /// `/steps` without snapshots.
        steps_no_snapshots: &'static str,
        /// A step in `/steps`: `{turn}`, `{prompt}` the message it served.
        step_of: &'static str,
        /// `/steps` where no step changed a file.
        steps_none: &'static str,
        /// `/steps`' title.
        steps_title: &'static str,
        /// `/diff` without snapshots.
        review_no_snapshots: &'static str,
        /// `/diff` where no turn changed a file.
        review_no_change: &'static str,
        /// A hunk rejected while Glut works.
        hunk_waits: &'static str,
        /// A hunk put back.
        hunk_put_back: &'static str,
        /// A comment on a hunk.
        comment_sent: &'static str,
        /// A hunk that cannot go back: `{path}`, `{reason}`.
        cannot_put_back: &'static str,
        /// Files to go back while Glut works.
        files_wait: &'static str,
        /// The vi mode on.
        vim_on: &'static str,
        /// The vi mode off.
        vim_off: &'static str,
        /// The footer's joins set: `{footer}`.
        footer_set: &'static str,
        /// Full motion set while something holds the screen still.
        motion_full_held: &'static str,
        /// Motion set: `{motion}`.
        motion_set: &'static str,
        /// The reader mode off while `--reader` holds it for this run.
        reader_off_next: &'static str,
        /// The reader mode on.
        reader_on: &'static str,
        /// The reader mode off.
        reader_off: &'static str,
        /// A theme that is none: `{name}`, `{known}` those there are.
        no_theme: &'static str,
        /// A theme set for this session alone: `{name}`.
        theme_session: &'static str,
        /// A theme set: `{name}`.
        theme_set: &'static str,
        /// The host refuses a paid call now: `{reason}`.
        not_now: &'static str,
        /// The host stopped the turn: `{reason}`.
        stopped_by: &'static str,
        /// An ask the host's app answered.
        answered_elsewhere: &'static str,
        /// The editor set: `{command}`.
        editor_set: &'static str,
        /// The model list of one provider: `{provider}`.
        model_of: &'static str,
        /// The model list of every provider.
        model_title: &'static str,
        /// Providers whose model list is not kept yet: `{providers}`.
        models_missing: &'static str,
        /// A project with no session yet.
        no_session_yet: &'static str,
        /// The project or session this window is in.
        this_one: &'static str,
        /// `/projects`' last entry.
        another_folder: &'static str,
        /// What it does.
        another_folder_detail: &'static str,
        /// `/projects`' title.
        projects_title: &'static str,
        /// `x` on the project open now.
        project_open: &'static str,
        /// A project pinned: `{name}`.
        project_pinned: &'static str,
        /// A project no longer pinned: `{name}`.
        project_unpinned: &'static str,
        /// A project hidden: `{name}`.
        project_hidden: &'static str,
        /// A switch to the project open now.
        project_same: &'static str,
        /// A session's title where it has none.
        no_message: &'static str,
        /// `/resume` where no other session is kept.
        sessions_none: &'static str,
        /// `/resume`'s title.
        sessions_title: &'static str,
        /// `/resume`'s first entry, the panes kept: `{layout}`.
        as_it_stood: &'static str,
        /// ⌫ on a session open in a pane.
        session_open_undeletable: &'static str,
        /// ⌫ on a session gone already.
        session_gone: &'static str,
        /// What deleting a session does.
        delete_detail: &'static str,
        /// What deleting a session with one image does.
        delete_detail_image: &'static str,
        /// What deleting a session with images does: `{n}` of them.
        delete_detail_images: &'static str,
        /// Sessions forked from it: `{n}`.
        delete_forks_stay: Plural,
        /// The delete question's yes: `{title}`.
        delete_yes: &'static str,
        /// The delete question's no.
        delete_no: &'static str,
        /// The delete question.
        delete_title: &'static str,
        /// A session deleted.
        session_deleted: &'static str,
        /// A session deleted with its image.
        session_deleted_image: &'static str,
        /// A session deleted with images: `{n}`.
        session_deleted_images: &'static str,
        /// A provider without a default model.
        no_default_model: &'static str,
        /// The provider list's title.
        provider_title: &'static str,
        /// A session picked while Glut works.
        session_opens_later: &'static str,
        /// A session picked that another pane holds.
        session_in_other_pane: &'static str,
        /// A command picked while the input holds a draft: `{command}`.
        draft_stays: &'static str,
        /// `x` on a subagent that has ended.
        subagent_ended: &'static str,
        /// A subagent stopped: `{task}`.
        subagent_stopping: &'static str,
        /// A subagent that cannot be stopped on its own.
        subagent_not_alone: &'static str,
        /// `/agents` with no subagent yet.
        no_subagent_yet: &'static str,
        /// `/agents`' title.
        subagents_title: &'static str,
        /// Why a model's message is held: the owner's Enter lets it go, `{pane}` its sender.
        held_enter: &'static str,
        /// Under a held message: `{why}`.
        held_why: &'static str,
        /// The cache kept by reading.
        cache_ping: &'static str,
        /// The cache kept an hour.
        cache_hour: &'static str,
        /// The cache not kept.
        cache_off: &'static str,
        /// A role set: `{name}`, `{about}` where it comes from and its tools.
        role_played: &'static str,
        /// The role taken off.
        role_none: &'static str,
        /// A role that is none: `{name}`, `{known}` those there are.
        no_role: &'static str,
        /// A role that narrows the tools in a session without a gate: `{name}`.
        role_needs_gate: &'static str,
        /// Where a role comes from: the project.
        source_project: &'static str,
        /// Where a role comes from: the owner.
        source_yours: &'static str,
        /// Where a role or a theme comes from: Glut itself.
        source_glut: &'static str,
        /// A role that names no tool.
        no_tool: &'static str,
        /// A role whose file names no tools.
        role_all_tools: &'static str,
        /// A role's source and tools: `{source}`, `{tools}`.
        role_source: &'static str,
        /// The role list for an agent in the background.
        background_role_title: &'static str,
        /// The role list's entry for no role.
        all_rounder: &'static str,
        /// What it does.
        all_rounder_detail: &'static str,
        /// `/role`'s title.
        role_title: &'static str,
        /// The first ⌃C on an empty line.
        quit_again: &'static str,
        /// An image that cannot be decoded.
        image_undecodable: &'static str,
        /// The reading mode with nothing in the stream.
        nothing_to_read: &'static str,
        /// An image attached for a model that sees none: `{image}`, `{model}`.
        image_unseen: &'static str,
        /// An image file attached: `{name}`, `{image}` its line.
        image_attached_named: &'static str,
        /// An image from the clipboard attached: `{image}`.
        image_attached: &'static str,
        /// An image opened in the system's viewer: `{n}`.
        image_opens: &'static str,
        /// An image Glut does not keep: `{n}`.
        image_not_kept: &'static str,
        /// Copied by way of the terminal.
        through_terminal: &'static str,
        /// A step's choice: show its change.
        step_show: &'static str,
        /// What it does.
        step_show_detail: &'static str,
        /// A step's choice: the files back.
        step_restore: &'static str,
        /// What it does.
        step_restore_detail: &'static str,
        /// A step's choices: `{turn}`.
        step_title: &'static str,
        /// A theme for a light terminal.
        theme_light: &'static str,
        /// A theme of Axiomata's.
        theme_axiomata: &'static str,
        /// `/theme`'s title.
        theme_title: &'static str,
        /// The effort list's entry for the provider's own.
        effort_default: &'static str,
        /// What it does.
        effort_default_detail: &'static str,
        /// `/effort`'s title.
        effort_title: &'static str,
        /// `/editor`'s title: `{now}`, and `{file}`, `{line}` written as `/editor` takes them.
        editor_title: &'static str,
        /// How many sessions a project keeps: `{n}`.
        sessions_count: Plural,
        /// How many characters were copied: `{n}`.
        copied: Plural,
        /// `a` in the To-dos tab while the input holds a message.
        todo_input_full: &'static str,
        /// A plan approved to run in auto where there is no sandbox.
        plan_auto_unsandboxed: &'static str,
        /// A kept pane's session is open in another pane already.
        kept_open_elsewhere: &'static str,
        /// A kept pane's session is empty or gone.
        kept_gone: &'static str,
        /// `/send` without a text.
        send_usage: &'static str,
        /// `/bg as` without a role.
        bg_as_usage: &'static str,
        /// `/bg` with a word it does not take: `{word}`.
        bg_unknown: &'static str,
        /// `/split as` without a role.
        split_as_usage: &'static str,
        /// `/split` with a side, which it no longer takes: `{word}`.
        split_side: &'static str,
        /// `/split` with a word it does not take: `{word}`.
        split_unknown: &'static str,
        /// ⌃P with one pane.
        one_pane_split: &'static str,
        /// `/peek` with a name that is none: `{name}`.
        no_background_peek: &'static str,
        /// `/quit` while others work: `{who}`.
        still_works_quit: &'static str,
        /// A project switch's no.
        switch_no: &'static str,
        /// What it does.
        switch_no_detail: &'static str,
        /// A project switch's yes: `{name}`.
        switch_yes: &'static str,
        /// What it does.
        switch_yes_detail: &'static str,
        /// A project switch while panes work: `{who}`.
        switch_question: &'static str,
        /// `/close` on the last pane.
        last_pane_quit: &'static str,
        /// `/close` or `/hide` on the main pane.
        main_pane_stays: &'static str,
        /// `/close` on a pane at work.
        close_working: &'static str,
        /// `/close` on a worktree's pane whose turn is stopping.
        close_stopping: &'static str,
        /// A worktree's pane working again when its leaving was picked.
        close_works_again: &'static str,
        /// A worktree kept: `{place}`, `{branch}`.
        worktree_kept: &'static str,
        /// A worktree discarded: `{branch}`.
        worktree_gone: &'static str,
        /// What became of a closed pane: its turn stopped.
        closed_stopped: &'static str,
        /// What became of a closed worktree pane.
        closed_kept: &'static str,
        /// What became of a closed pane: its session is listed.
        closed_resume: &'static str,
        /// A pane closed: `{n}`, `{kept}` what became of its session.
        pane_closed: &'static str,
        /// A split where the host keeps one pane.
        host_one_pane: &'static str,
        /// A split beyond the most panes: `{n}`.
        most_panes_bg: &'static str,
        /// A split without room while the main pane is wide.
        no_room_narrower: &'static str,
        /// A split without room.
        no_room: &'static str,
        /// A split while another pane starts.
        pane_starting: &'static str,
        /// Why no worktree is offered: `{reason}`.
        no_worktree_offered: &'static str,
        /// `tree` outside a git repository.
        no_repository: &'static str,
        /// A worktree left where it is: `{place}`.
        worktree_stays: &'static str,
        /// A worktree removed again: `{branch}`.
        worktree_gone_again: &'static str,
        /// A pane that could not start: `{n}`, `{reason}`.
        pane_not_started: &'static str,
        /// An agent in the background ready: `{name}`.
        background_waits: &'static str,
        /// A new pane ready while the keys stay: `{n}`.
        pane_ready: &'static str,
        /// `/send` with no other pane.
        one_pane_send: &'static str,
        /// `/send` to a name that is none: `{name}`.
        no_pane_or_agent: &'static str,
        /// `/send` naming none among several.
        send_which: &'static str,
        /// `/send plan` before any plan.
        no_plan_send: &'static str,
        /// A message sent: the pane starts on it.
        sent_now: &'static str,
        /// A message sent: the pane takes it after its turn.
        sent_after_turn: &'static str,
        /// A message sent: the pane takes it once free.
        sent_when_free: &'static str,
        /// A message sent: it waits for Enter there.
        sent_on_enter: &'static str,
        /// A message sent: `{label}` its pane, `{when}` one of the four above.
        sent_to: &'static str,
        /// The kept layout's no.
        layout_no: &'static str,
        /// What it does.
        layout_no_detail: &'static str,
        /// `glut -c`'s question about the kept layout.
        layout_question: &'static str,
        /// No layout kept.
        no_layout: &'static str,
        /// The layout back into a window of several panes.
        layout_one_pane_only: &'static str,
        /// The layout back while Glut works.
        layout_waits: &'static str,
        /// A kept worktree whose folder is no repository any more.
        no_repository_now: &'static str,
        /// A kept pane that cannot come back: `{branch}`, `{reason}`.
        kept_pane_away: &'static str,
        /// The kept layout came back.
        layout_back: &'static str,
        /// The kept layout came back with gaps.
        layout_back_partly: &'static str,
        /// A name that names no agent in the background: `{name}`.
        no_background_agent: &'static str,
        /// `/show` beyond the most panes: `{n}`.
        most_panes_hide: &'static str,
        /// `/show` without room.
        no_room_show: &'static str,
        /// `/close` of an agent in a worktree: `{name}`.
        agent_in_worktree: &'static str,
        /// An agent in the background ended: `{name}`, `{kept}`.
        agent_ended: &'static str,
        /// `/hide` on the last pane.
        last_pane_stays: &'static str,
        /// A pane sent to the background: `{name}`.
        hidden_works_on: &'static str,
        /// `/swap` with one pane.
        one_pane_swap: &'static str,
        /// `/swap` naming none among several.
        swap_which: &'static str,
        /// `/swap` with a name that is no pane on the screen: `{name}`.
        no_pane_shown: &'static str,
        /// `/swap` with the main pane.
        main_already: &'static str,
        /// A pane made the main one: `{n}`.
        main_now: &'static str,
        /// Where a new pane works: this folder.
        where_here: &'static str,
        /// What it means.
        where_here_detail: &'static str,
        /// Where a new pane works: a new worktree.
        where_tree: &'static str,
        /// What it means: `{branch}` its branch.
        where_tree_detail: &'static str,
        /// Where a new pane works, asked: `{n}`.
        where_title: &'static str,
        /// A question's plain no.
        no: &'static str,
        /// The seventh agent's no.
        more_no_detail: &'static str,
        /// The seventh agent's yes.
        more_yes: &'static str,
        /// What it does.
        more_yes_detail: &'static str,
        /// The seventh agent's question: `{n}` agents.
        more_title: &'static str,
        /// What a worktree holds cannot be read.
        holds_unreadable: &'static str,
        /// A worktree whose repository was on no branch.
        no_branch_merge: &'static str,
        /// A worktree pane's leaving: keep.
        leave_keep: &'static str,
        /// What it does: `{branch}`, `{held}` what it holds.
        leave_keep_detail: &'static str,
        /// A worktree pane's leaving: take over.
        leave_take_over: &'static str,
        /// What it does: `{branch}`, `{held}`, `{into}` the branch it merges into.
        leave_take_over_detail: &'static str,
        /// A worktree pane's leaving: discard.
        leave_discard: &'static str,
        /// What it does: `{branch}`, `{held}`.
        leave_discard_detail: &'static str,
        /// The question how a worktree pane ends.
        leave_title: &'static str,
        /// The discard question's no.
        discard_no_detail: &'static str,
        /// The discard question's yes.
        discard_yes: &'static str,
        /// What the discard loses: `{branch}`, `{held}`.
        discard_lost: &'static str,
        /// What the discard loses where it cannot be read: `{branch}`, `{reason}`.
        discard_whatever: &'static str,
        /// The discard question: `{branch}`.
        discard_title: &'static str,
        /// Two or more names: `{rest}` all but the last, joined by commas, `{last}`.
        list_and: &'static str,
        /// A worktree with nothing of its own.
        nothing_new: &'static str,
        /// git cannot be started: `{reason}`.
        git_cannot_run: &'static str,
        /// git ended for taking too long: `{secs}` seconds.
        git_too_long: &'static str,
        /// git's output beyond what Glut reads.
        git_too_much: &'static str,
        /// A repository whose git folder is no plain one: `{folder}`.
        no_plain_git: &'static str,
        /// A worktree asked for inside a worktree.
        is_worktree: &'static str,
        /// A branch name Glut refuses: `{branch}`.
        bad_branch: &'static str,
        /// A worktree's place taken already: `{place}`.
        there_already: &'static str,
        /// A worktree made where it does not belong.
        worktree_misplaced: &'static str,
        /// A kept worktree that is gone: `{branch}`, `{place}`.
        worktree_of_gone: &'static str,
        /// A worktree outside Glut's place for them: `{place}`.
        not_glut_place: &'static str,
        /// A worktree its repository no longer lists: `{place}`, `{repo}`.
        no_worktree_of: &'static str,
        /// A kept worktree on another branch: `{branch}`.
        worktree_other_branch: &'static str,
        /// git's count that is no number.
        git_no_number: &'static str,
        /// A take-over that would write protected files: `{branch}`, `{files}`.
        protected_changes: &'static str,
        /// A take-over where the repository was on no branch.
        repo_no_branch: &'static str,
        /// No branch checked out.
        no_branch: &'static str,
        /// A take-over where the repository moved: `{now}`, `{into}`.
        repo_other_branch: &'static str,
        /// A take-over of a worktree with changes not committed: `{branch}`.
        tree_uncommitted: &'static str,
        /// A take-over while the repository merges.
        repo_merging: &'static str,
        /// A take-over while the repository has changes not committed: `{folder}`.
        repo_uncommitted: &'static str,
        /// A merge that stopped: `{reason}`.
        merge_stopped: &'static str,
        /// A merge that could not be read: `{reason}`.
        merge_unreadable: &'static str,
        /// A merge that could not be committed: `{reason}`.
        merge_uncommitted: &'static str,
        /// A worktree taken over: `{branch}`, `{into}`.
        merged: &'static str,
        /// A worktree that would lose work: `{branch}`, `{held}`.
        tree_holds: &'static str,
        /// A worktree whose content cannot be read: `{branch}`, `{reason}`.
        tree_unreadable: &'static str,
        /// A worktree that is not Glut's: `{place}`.
        not_glut_worktree: &'static str,
        /// Commits a worktree holds: `{n}`.
        commits_count: Plural,
        /// Files not committed in a worktree: `{n}`.
        uncommitted_count: Plural,
        /// What parts the names of a list but the last two.
        list_comma: &'static str,
        /// Two counts side by side: `{first}`, `{second}`.
        two_counts: &'static str,
        /// A note and what follows from it: `{first}`, `{second}`.
        clause_join: &'static str,
        /// A worktree that could not be removed: `{place}`, `{reason}`.
        worktree_stays_why: &'static str,
        /// Glut could not start anew in another project: `{folder}`, `{reason}`.
        restart_failed: &'static str,
        /// While the first session is built.
        starting: &'static str,
        /// `glut -c` in a folder with no session.
        no_earlier_session: &'static str,
        /// The terminal cannot be used: `{reason}`.
        terminal_unusable: &'static str,
        /// The line printed at the end: `{which}` the pane or nothing, `{input}`, `{output}` tokens, `{cost}` or
        /// nothing.
        session_ended: &'static str,
        /// Under the conversation printed into the terminal's scrollback.
        scrollback_shown: &'static str,
        /// The scrollback could not be printed: `{reason}`.
        scrollback_failed: &'static str,
        /// No editor chosen.
        no_editor: &'static str,
        /// An editor whose command is empty.
        editor_empty: &'static str,
        /// An editor that ended badly: `{program}`, `{status}`.
        editor_ended: &'static str,
        /// An editor that cannot start: `{program}`, `{reason}`.
        editor_unstartable: &'static str,
        /// A folder that cannot be used: `{path}`, `{reason}`.
        cannot_use: &'static str,
        /// A file that cannot be written: `{path}`, `{reason}`.
        cannot_write: &'static str,
        /// A file that cannot be read: `{path}`, `{reason}`.
        cannot_read_path: &'static str,
        /// An editor for the input that ended badly: `{program}`, `{status}`.
        editor_ended_input: &'static str,
        /// The guides a session read: `{guides}`.
        guides_read: &'static str,
        /// A skill left out: `{note}` why.
        skill_left_out: &'static str,
        /// A provider's model list that could not be had: `{provider}`, `{reason}`.
        no_model_list: &'static str,
        /// A model its provider does not list: `{model}`, `{provider}`.
        model_not_listed: &'static str,
        /// A provider missing from providers.json: `{provider}`.
        no_provider_entry: &'static str,
        /// A turn stopped before it began.
        stopped_before_turn: &'static str,
        /// A summary that broke off: `{reason}`.
        summary_broke_off: &'static str,
        /// A switch of model that broke off: `{reason}`.
        switch_broke_off: &'static str,
        /// A handoff of an empty conversation.
        nothing_to_hand_over: &'static str,
        /// A model that wrote no handoff.
        no_handoff_written: &'static str,
        /// A model that called tools instead of writing the handoff.
        handoff_tool_calls: &'static str,
        /// No folder for the handoff.
        no_handoff_folder: &'static str,
        /// A handoff that broke off: `{reason}`.
        handoff_broke_off: &'static str,
        /// `/handoff read` without a handoff.
        no_handoff_here: &'static str,
        /// A path that is no regular file: `{path}`.
        not_regular_file: &'static str,
        /// No folder for handoffs once read.
        no_read_folder: &'static str,
        /// The first start's question for a provider.
        setup_pick_provider: &'static str,
        /// The setup without a provider chosen.
        setup_no_provider: &'static str,
        /// The setup's question for a key: `{provider}`.
        setup_key: &'static str,
        /// The setup without a key given.
        setup_no_key: &'static str,
        /// Where the key was kept: `{place}`.
        setup_key_kept: &'static str,
        /// The setup done: `{provider}`.
        setup_provider_done: &'static str,
        /// The setup's question for a model: `{provider}`.
        setup_pick_model: &'static str,
        /// The setup's model chosen.
        setup_model_done: &'static str,
        /// While the setup asks for the models: `{provider}`.
        setup_asking_models: &'static str,
        /// The setup without a model list: `{reason}`.
        setup_no_list: &'static str,
        /// The setup's prompt for a model.
        setup_model_prompt: &'static str,
        /// The setup without a model chosen.
        setup_no_model: &'static str,
        /// The setup stopped.
        setup_stopped: &'static str,
        /// A file Glut did not write, left alone: `{path}`.
        not_glut_file: &'static str,
        /// `/projects` with an empty path.
        projects_no_folder: &'static str,
        /// A path that names no folder: `{folder}`.
        no_such_folder: &'static str,
        /// A path that is no folder: `{folder}`.
        not_a_folder: &'static str,
        /// A path that is one of Glut's worktrees: `{folder}`.
        is_glut_worktree: &'static str,
        /// When: under a minute ago.
        just_now: &'static str,
        /// When: `{n}` minutes ago.
        minutes_ago: &'static str,
        /// When: `{n}` hours ago.
        hours_ago: &'static str,
        /// When: a day ago.
        yesterday: &'static str,
        /// `glut watch` with a name that is none: `{name}`.
        no_session_or_log: &'static str,
        /// `glut watch` in a folder with no session.
        nothing_to_watch: &'static str,
        /// A watched log that is full.
        log_full_note: &'static str,
        /// The watched session's state: its log is full.
        watch_log_full: &'static str,
        /// The watched session's state: at work.
        watch_working: &'static str,
        /// The watched session's state: at rest.
        watch_idle: &'static str,
        /// `glut watch`'s header: before the session's name.
        watching: &'static str,
        /// `glut watch`'s header: only to read.
        read_only: &'static str,
        /// `glut watch` before the first step.
        watch_waiting: &'static str,
        /// `glut watch` following the end.
        watch_following: &'static str,
        /// `glut watch` scrolled up: `{n}` lines.
        watch_lines_up: &'static str,
        /// `glut watch`'s footer: `{input}`, `{output}` tokens, `{place}` following or how far up.
        watch_footer: &'static str,
        /// ⌃V with no image in the clipboard.
        no_clipboard_image: &'static str,
        /// The program that reads images from the clipboard is missing: `{program}`.
        clipboard_program_missing: &'static str,
        /// A file gone: `{path}`.
        path_gone: &'static str,
        /// In a kept layout's details: an agent, `{name}`.
        layout_agent: &'static str,
        /// In a kept layout's details: a role, `{role}`.
        layout_role: &'static str,
        /// In a kept layout's details: a branch, `{branch}`.
        layout_branch: &'static str,
        /// In a kept layout's details: a plain pane.
        layout_a_pane: &'static str,
        /// Another model's message in the stream: `{pane}`, `{text}`.
        from_pane_model: &'static str,
        /// When: `{n}` days ago.
        days_ago: Plural,
        /// How many agents in the background a layout keeps: `{n}`.
        agents_count: Plural,
        /// The keys a screen reader says, by the system's names: ⌃ ⇧ ⌥ (each ending in `+`), ⏎ ⎋ ⇥ ⌫, ↑ ↓ ← →,
        /// Page Up, Page Down — in this order.
        key_words: &'static [&'static str],
        /// The setup's prompt for a provider's number.
        setup_provider_prompt: &'static str,
        /// The command line's help (`glut --help`), by its English text as clap's derive writes it — English's own
        /// table is empty: every help text of the command tree, a test holds.
        cli: &'static [(&'static str, &'static str)],
        /// The help's headings: usage (with its colon), options, arguments, commands.
        cli_headings: &'static [&'static str],
        /// Beside a model that reasons, in the model list.
        model_reasons: &'static str,
        /// A subagent's tile before its first step.
        agent_starting: &'static str,
        /// The conversation compacted by the model's side: `{before}` and `{after}` thousands of tokens.
        compacted_tokens: &'static str,
        /// The permission mode changed: `{mode}` its name, as written.
        mode_now: &'static str,
        /// Over a model's thinking while it streams.
        thinking_live: &'static str,
        /// Over a model's thinking once it ended.
        thought: &'static str,
        /// The head of a thinking block: `{state}` one of the two above, `{size}` its tokens, short.
        thought_size: &'static str,
        /// A subagent tile's counts: `{calls}` how many calls (the plural below), `{input}` and `{output}` tokens.
        agent_stats: &'static str,
        /// At a subagent tile's foot: a click opens its view.
        tile_opens: &'static str,
        /// A subagent's calls: `{n}`.
        agent_calls: Plural,
        /// A turn ended with an answer, in a notification (T53): `{time}` how long it ran.
        notify_done: &'static str,
        /// A turn ended without an answer (an error, refusals, the budget): `{time}`.
        notify_ended: &'static str,
        /// The gate asks, in a notification: `{what}` the kind of call.
        notify_asks: &'static str,
        /// An ask's kind: a shell command.
        notify_ask_command: &'static str,
        /// An ask's kind: a change of a file.
        notify_ask_file: &'static str,
        /// An ask's kind: the network.
        notify_ask_network: &'static str,
        /// An ask's kind: an MCP tool.
        notify_ask_mcp: &'static str,
        /// An ask's kind: a subagent.
        notify_ask_agent: &'static str,
        /// An ask's kind: the memory or Glut's own files.
        notify_ask_glut: &'static str,
        /// An ask's kind: another tool, `{tool}` its name.
        notify_ask_other: &'static str,
        /// How long a turn ran: `{n}` seconds.
        notify_secs: &'static str,
        /// How long a turn ran: `{n}` minutes.
        notify_mins: &'static str,
        /// A notification names a few; `{n}` more.
        notify_more: &'static str,
        /// `/notify on`.
        notify_on: &'static str,
        /// `/notify asks`.
        notify_asks_only: &'static str,
        /// `/notify off`.
        notify_off: &'static str,
        /// `/notify system`.
        notify_system: &'static str,
        /// `/notify after`: `{n}` seconds.
        notify_after_set: &'static str,
        /// `/notify after` without seconds it takes: `{most}` the most.
        notify_after_bad: &'static str,
        /// `/notify` alone: `{when}` the choice, `{after}` the seconds, `{way}` how it goes out.
        notify_now: &'static str,
        /// An export's heading of the owner's messages (T54).
        export_owner: &'static str,
        /// An export's heading of the model's answers.
        export_glut: &'static str,
        /// An export's heading of the model's thinking.
        export_thinking: &'static str,
        /// An export's heading of the system prompt.
        export_system: &'static str,
        /// An export's line before a call: `{name}` the tool.
        export_tool: &'static str,
        /// An export's line before a result: `{name}` the tool.
        export_result: &'static str,
        /// `/export` wrote a file: `{path}`.
        export_written: &'static str,
        /// `/export copy` put the Markdown on the clipboard.
        export_copied: &'static str,
        /// `/export` before the first turn ended.
        export_empty: &'static str,
        /// `/export`'s file is there already: `{path}`.
        export_exists: &'static str,
        /// `glut export` found nothing by the name: `{what}`.
        export_unknown: &'static str,
        /// The path list's frame (T68): `{folder}` the folder read.
        paths_title: &'static str,
        /// The path list names the first; `{n}` more fit.
        paths_more: &'static str,
        /// `/resume`'s search (T55): how many more messages of a session hold the words: `{count}`.
        found_more: &'static str,
        /// `/resume`'s search while the kept sessions are read.
        found_reading: &'static str,
        /// `/resume`'s hint: ⌃A searches every project.
        found_folder: &'static str,
        /// `/resume` searching every project: ⌃A goes back to the folder's.
        found_all: &'static str,
        /// `glut --session` named a session of another folder: `{folder}`.
        session_other_folder: &'static str,
        /// `/mcp` without a server (T56).
        mcp_none: &'static str,
        /// `/mcp`'s list, its head.
        mcp_head: &'static str,
        /// A server being started.
        mcp_starting: &'static str,
        /// A server connected: `{count}` tools.
        mcp_connected: &'static str,
        /// A server that wants a sign-in: `{name}`.
        mcp_wants: &'static str,
        /// A server that failed: `{reason}`.
        mcp_failed: &'static str,
        /// Beside a project's server.
        mcp_project: &'static str,
        /// `/mcp login` on a server Glut starts: `{name}`.
        mcp_stdio_no_login: &'static str,
        /// `/mcp login` on a server with its own Authorization header: `{name}`.
        mcp_header_login: &'static str,
        /// A server the session does not have: `{name}`.
        mcp_no_server: &'static str,
        /// `/mcp` failed: `{reason}`.
        mcp_error: &'static str,
        /// A server that answers without a sign-in: `{name}`.
        mcp_open: &'static str,
        /// A server with OAuth for registered clients only: `{name}`.
        mcp_registered_only: &'static str,
        /// A sign-in that did not succeed: `{name}`, `{reason}`.
        mcp_sign_in_failed: &'static str,
        /// Where the owner signs in: `{name}`, `{host}`, `{address}`.
        mcp_sign_in_at: &'static str,
        /// The browser was opened.
        mcp_browser_opened: &'static str,
        /// The browser was not opened: `{name}`.
        mcp_paste_back: &'static str,
        /// Signed in and connected: `{name}`, `{count}` tools.
        mcp_signed_in: &'static str,
        /// A server that wants a sign-in, said once: `{name}`.
        mcp_wants_note: &'static str,
        /// A server that did not start: `{name}`, `{reason}`.
        mcp_not_up: &'static str,
        /// A secret with spaces or control characters.
        secret_odd: &'static str,
        /// A secret not given.
        secret_none: &'static str,
        /// `/mcp logout` with nothing kept: `{name}`.
        mcp_out_nothing: &'static str,
        /// A typed token forgotten: `{name}`.
        mcp_out_token: &'static str,
        /// A grant revoked and forgotten: `{name}`.
        mcp_out_revoked: &'static str,
        /// A grant forgotten, no revocation offered: `{name}`.
        mcp_out_here: &'static str,
        /// A grant forgotten, the revocation failed: `{name}`, `{reason}`.
        mcp_out_failed: &'static str,
        /// How `/mcp` is used.
        mcp_usage: &'static str,
        /// A server added: `{name}`.
        mcp_added: &'static str,
        /// A server added and connected: `{name}`, `{count}` tools.
        mcp_up: &'static str,
        /// `/mcp remove` of a server not in the owner's file: `{name}`.
        mcp_not_owners: &'static str,
        /// A server removed: `{name}`.
        mcp_removed: &'static str,
        /// An address pasted back where no sign-in waits: `{name}`.
        mcp_no_waiting: &'static str,
        /// The hidden entry's prompt for a token: `{name}`, `{host}`.
        secret_token: &'static str,
        /// The hidden entry's prompt for a client's secret: `{id}`.
        secret_client: &'static str,
        /// The key line while a secret is typed.
        hint_secret: &'static str,
        /// A project's server not approved (T56): `{name}`.
        mcp_refused: &'static str,
        /// A sign-in whose tokens come from another host: `{name}`, `{host}`.
        mcp_token_elsewhere: &'static str,
        /// In the input's place while a sign-in's address waits to be opened.
        mcp_open_ask: &'static str,
        /// The key line while a sign-in's address waits to be opened.
        hint_opening: &'static str,
        /// The browser could not be opened.
        mcp_open_failed: &'static str,
        /// The owner left the address to open themselves.
        mcp_open_left: &'static str,
        /// `/btw` without a question (T57).
        btw_usage: &'static str,
        /// A side question's model wanted a tool, which was dropped.
        btw_wanted_tools: &'static str,
        /// A side question that failed: `{reason}`.
        btw_failed: &'static str,
        /// The side question's card while nothing of the answer came.
        btw_waiting: &'static str,
        /// The key line while a side question's card is open.
        hint_btw: &'static str,
        /// A background agent's card while it asks (T69): `{target}`.
        agent_asks_live: &'static str,
        /// A card's live line while the model writes.
        agent_writing: &'static str,
        /// A card's live line while it works and shows nothing yet.
        agent_working_live: &'static str,
        /// A background agent that asks the owner.
        agent_asks_state: &'static str,
        /// A background agent that holds a message.
        agent_holds_state: &'static str,
        /// A background agent at work.
        agent_works_state: &'static str,
        /// A background agent at rest.
        agent_rests_state: &'static str,
        /// A tile of several subagents that ended: `{n}`, two or more.
        tiles_subagents: &'static str,
        /// `/notify tiles` alone: `{now}` on or off.
        notify_tiles_now: &'static str,
        /// Tiles switched on.
        notify_tiles_on: &'static str,
        /// Tiles switched off.
        notify_tiles_off: &'static str,
        /// `~/.glut/secrets.json` cannot be used (T58): `{reason}`; Glut's own formats mask alone.
        secrets_file_broken: &'static str,
        /// Beside a tool call whose result had keys hidden: `{n}` of them.
        secrets_hidden_note: &'static str,
        /// The owner's message holds what looks like a key: `{kinds}`, the kinds.
        secrets_in_message: &'static str,
        /// Send the message with the keys hidden.
        secrets_send_masked: &'static str,
        /// What the masked choice does.
        secrets_send_masked_detail: &'static str,
        /// Send the message as typed.
        secrets_send_as_is: &'static str,
        /// Back to the input to edit the message.
        secrets_back: &'static str,
        /// `/secrets` where the host turned masking off.
        secrets_hosted: &'static str,
        /// Masking switched on.
        secrets_on: &'static str,
        /// Masking switched off.
        secrets_off: &'static str,
        /// `/secrets` alone while masking is off.
        secrets_off_now: &'static str,
        /// `/secrets` alone with nothing hidden yet.
        secrets_none: &'static str,
        /// Above the list of hidden values.
        secrets_heading: &'static str,
        /// One hidden value: `{n}` its number, `{kind}`, `{chars}` its length, `{seen}` where it was seen.
        secrets_row: &'static str,
        /// How `/secrets` is used.
        secrets_usage: &'static str,
        /// `/folder` alone with no folder beside the working one.
        folders_none: &'static str,
        /// Above the list of the folders beside the working one (T60c).
        folders_heading: &'static str,
        /// `/folder add`: `{folder}` added.
        folder_added: &'static str,
        /// `/folder add` of `{folder}`, added before.
        folder_known: &'static str,
        /// `/folder add` refused, `{reason}` why.
        folder_refused: &'static str,
        /// `/folder remove`: `{folder}` taken away.
        folder_removed: &'static str,
        /// `/folder remove` of `{folder}`, which is none.
        folder_unknown: &'static str,
        /// How `/folder` is used.
        folder_usage: &'static str,
        /// `/folder add` of `{folder}`, which lies in `{other}` or holds it.
        folder_overlaps: &'static str,
        /// After an undo: a folder beside the working one not put back, `{what}` which and why.
        files_beside_not_restored: &'static str,
        /// How `/loop` is used.
        loop_usage: &'static str,
        /// `/loop` with `{word}` where its interval stands.
        loop_interval_bad: &'static str,
        /// A loop's task is a command a round may not run: `{name}`.
        loop_command_refused: &'static str,
        /// `/loop rounds` with no number from 1 to `{most}`.
        loop_rounds_bad: &'static str,
        /// `/loop budget` with no amount above 0.
        loop_budget_bad: &'static str,
        /// `/loop rounds` alone: `{n}` the most rounds now.
        loop_rounds_now: &'static str,
        /// `/loop rounds` kept `{n}`.
        loop_rounds_set: &'static str,
        /// `/loop budget` alone: `{usd}` the most a loop may cost now.
        loop_budget_now: &'static str,
        /// `/loop budget` kept `{usd}`.
        loop_budget_set: &'static str,
        /// A loop started: `{name}` its agent, `{every}` its interval, `{task}`.
        loop_starts: &'static str,
        /// A loop ended: `{name}`, `{why}`, `{rounds}` rounds run, `{cost}` what they cost.
        loop_ended: &'static str,
        /// Why a loop ended: its model said the goal is reached, `{reason}` why.
        loop_why_goal: &'static str,
        /// Why a loop ended: the owner stopped it.
        loop_why_stopped: &'static str,
        /// Why a loop ended: a round ended `{reason}`.
        loop_why_failed: &'static str,
        /// Why a loop ended: `{n}` rounds ran, its most.
        loop_why_rounds: &'static str,
        /// Why a loop ended: its budget `{max}` is spent.
        loop_why_budget: &'static str,
        /// `/loop` with no loop running.
        loop_none: &'static str,
        /// `/loop stop` alone while several loops run.
        loop_which: &'static str,
        /// `/loop stop` of `{name}`, which runs no loop.
        loop_unknown: &'static str,
        /// Above the list of the loops running.
        loops_heading: &'static str,
        /// One loop listed: `{name}`, `{every}`, `{round}` rounds run of `{rounds}`, `{when}`, `{cost}`, `{task}`.
        loop_line: &'static str,
        /// In a loop's line: a round runs now.
        loop_round_runs: &'static str,
        /// In a loop's line: the next round in `{time}`.
        loop_next_in: &'static str,
        /// `/loop` while `{n}` agents work in the background already.
        loop_too_many: &'static str,
        /// Why a loop ended: its task started no turn.
        loop_no_turn: &'static str,
        /// `/loop as` another role in a pane that plays `{role}`.
        loop_role_kept: &'static str,
        /// Added where a loop starts on a model without prices.
        loop_uncounted: &'static str,
        /// A loop's card once the loop ended: `{rounds}` of `{most}` rounds.
        loop_card_ended: &'static str,
        /// Said in a loop's own pane as it starts: how the loop ends.
        loop_pane_hint: &'static str,
        /// `~/.glut/schedules.json` cannot be read.
        schedule_file_broken: &'static str,
        /// A plan's name that cannot be one.
        schedule_name_bad: &'static str,
        /// A new plan without its task.
        schedule_task_missing: &'static str,
        /// A plan's task that is another of the owner's commands.
        schedule_task_command: &'static str,
        /// `--at` without a time of day.
        schedule_at_bad: &'static str,
        /// `--days` without days.
        schedule_days_bad: &'static str,
        /// `--every` without an interval Glut takes.
        schedule_every_bad: &'static str,
        /// `--days` given to a plan that runs `--every`.
        schedule_days_without_at: &'static str,
        /// A new plan without its times.
        schedule_when_missing: &'static str,
        /// `--mode` without a mode.
        schedule_mode_bad: &'static str,
        /// `--allow` or `--deny` with no rule.
        schedule_rule_bad: &'static str,
        /// `--max-usd` without an amount Glut takes.
        schedule_usd_bad: &'static str,
        /// `--max-minutes` out of its range.
        schedule_minutes_bad: &'static str,
        /// A plan's folder that is not there.
        schedule_folder_gone: &'static str,
        /// launchd refused a plan's agent.
        schedule_launchd_failed: &'static str,
        /// A plan's name that names none.
        schedule_unknown: &'static str,
        /// Above what a new plan may do.
        schedule_made: &'static str,
        /// Above what a changed plan may do.
        schedule_changed: &'static str,
        /// When and where a plan runs.
        schedule_shown_when: &'static str,
        /// A plan's task.
        schedule_shown_task: &'static str,
        /// A plan's model, fixed at making.
        schedule_shown_model: &'static str,
        /// A plan's mode and what it lets the run do.
        schedule_shown_mode: &'static str,
        /// What `ask` lets a plan's run do.
        schedule_means_ask: &'static str,
        /// What `build` lets a plan's run do.
        schedule_means_build: &'static str,
        /// What `plan` lets a plan's run do.
        schedule_means_plan: &'static str,
        /// What `dont_ask` lets a plan's run do.
        schedule_means_dont_ask: &'static str,
        /// What `auto` lets a plan's run do.
        schedule_means_auto: &'static str,
        /// The rules a plan allows.
        schedule_shown_allow: &'static str,
        /// The rules a plan refuses.
        schedule_shown_deny: &'static str,
        /// A plan's folders beside its own.
        schedule_shown_folders: &'static str,
        /// Where a plan's run ends.
        schedule_shown_limits: &'static str,
        /// Whose rules count beside a plan's.
        schedule_shown_owner_rules: &'static str,
        /// When launchd runs a plan.
        schedule_shown_logged_in: &'static str,
        /// A plan changed while it is paused.
        schedule_shown_paused: &'static str,
        /// A plan that runs every day.
        schedule_when_at: &'static str,
        /// A plan that runs on some days.
        schedule_when_at_days: &'static str,
        /// A plan that runs every so often.
        schedule_when_every: &'static str,
        /// `glut schedule list` without plans.
        schedule_none: &'static str,
        /// A paused plan's next time.
        schedule_paused_mark: &'static str,
        /// A plan that never ran.
        schedule_never: &'static str,
        /// How a plan's last run ended.
        schedule_last: &'static str,
        /// One plan in `glut schedule list`.
        schedule_line: &'static str,
        /// `glut schedule pause` done.
        schedule_paused: &'static str,
        /// `glut schedule resume` done.
        schedule_resumed: &'static str,
        /// `glut schedule remove` done.
        schedule_removed: &'static str,
        /// A plan's run while the one before still runs.
        schedule_running_already: &'static str,
        /// How a plan's run ended, told.
        schedule_run_line: &'static str,
        /// What a plan's run asked and was refused.
        schedule_asks_refused: &'static str,
        /// A plan's model without prices.
        schedule_no_prices: &'static str,
        /// `tell` in a plan's run.
        schedule_no_panes: &'static str,
        /// A run that answered.
        schedule_outcome_answered: &'static str,
        /// A run that failed.
        schedule_outcome_failed: &'static str,
        /// A run that spent its budget.
        schedule_outcome_budget: &'static str,
        /// A run past its time limit.
        schedule_outcome_timeout: &'static str,
        /// A run that refusals ended.
        schedule_outcome_refused: &'static str,
        /// A run that was stopped.
        schedule_outcome_stopped: &'static str,
        /// A run that did not start because the one before still ran.
        schedule_outcome_skipped: &'static str,
        /// A run that could not start.
        schedule_outcome_unstarted: &'static str,
        /// A run that reached its subscription's usage limit (PV4).
        schedule_outcome_plan_limit: &'static str,
        /// The Runs tab's keys while its list has them.
        keys_runs: &'static str,
        /// `d` on the Runs tab: the question before a plan is deleted.
        runs_delete_ask: &'static str,
        /// The Runs tab's top line while loops run (T60a).
        runs_loops: &'static str,
        /// The Runs tab without plans.
        runs_none: &'static str,
        /// When a plan runs next, on its tile.
        runs_next: &'static str,
        /// An opened plan's limits and model.
        runs_limits: &'static str,
        /// Above an opened plan's last runs.
        runs_last_ten: &'static str,
        /// `r` or `/schedule run`: a plan's run started beside Glut.
        runs_started: &'static str,
        /// `/schedule` with words it does not take.
        schedule_usage: &'static str,
        /// `/schedule add` with a quote left open.
        schedule_quote_open: &'static str,
        /// Said at the start: what the plans ran since Glut last started.
        runs_since: &'static str,
        /// One plan in `runs_since`: how often it ran and how its last run ended.
        runs_since_one: &'static str,
        /// The list `/login` opens.
        login_title: &'static str,
        /// A subscription signed in: `{who}` the account.
        login_state_in: &'static str,
        /// A subscription not signed in.
        login_state_out: &'static str,
        /// The question before a sign-out: `{id}`.
        login_sign_out_title: &'static str,
        /// The sign-out's yes.
        login_sign_out_yes: &'static str,
        /// The sign-out's no.
        login_sign_out_no: &'static str,
        /// A sign-in or sign-out that failed: `{id}`, `{reason}`.
        login_failed: &'static str,
        /// Where to sign in: `{host}`, `{address}`.
        login_sign_in_at: &'static str,
        /// Over SSH: how the address comes back: `{id}`.
        login_paste_back: &'static str,
        /// A sign-in done: `{id}`, `{who}`.
        login_signed_in: &'static str,
        /// Once, after the first sign-in: what the plan's limit means. `{page}` ChatGPT's usage page.
        login_welcome: &'static str,
        /// A sign-out with nothing signed in: `{id}`.
        login_out_nothing: &'static str,
        /// A sign-out done and revoked: `{id}`.
        login_out_revoked: &'static str,
        /// A sign-out done here, its revocation unconfirmed: `{id}`, `{reason}`.
        login_out_unconfirmed: &'static str,
        /// An address pasted back with no sign-in waiting: `{id}`.
        login_no_waiting: &'static str,
        /// The footer's word where money would stand: the session runs on a ChatGPT plan.
        plan_footer: &'static str,
        /// The meter's line where the price would stand: `{page}` ChatGPT's usage page.
        meter_plan: &'static str,
        /// The program a plan would start lies where its commands write.
        schedule_program_writable: &'static str,
        /// A plan's run whose PATH reaches where its commands write.
        schedule_path_writable: &'static str,
        /// An `auto` plan's run without the sandbox.
        schedule_auto_unsandboxed: &'static str,
        /// `r` on the Runs tab: the question before a plan runs now.
        runs_run_ask: &'static str,
        /// `/secrets allow` with a number no value has: `{n}`.
        secrets_no_such: &'static str,
        /// Value `{n}` let through for this session.
        secrets_allowed: &'static str,
        /// Value `{n}` let through always.
        secrets_allowed_always: &'static str,
        /// Added to what `/secrets on|off|allow` said: it holds from the next read, the model is told.
        secrets_next_read: &'static str,
        /// `/secrets add` with a pattern that cannot be used: `{reason}`.
        secrets_bad_regex: &'static str,
        /// A format of the owner's added: `{name}`.
        secrets_added: &'static str,
        /// `/secrets remove` of a format the owner has not: `{name}`.
        secrets_no_format: &'static str,
        /// A format of the owner's removed: `{name}`.
        secrets_removed: &'static str,
        /// Exported to `{path}` while keys are masked (T58): what the owner typed may still hold one.
        export_written_masked: &'static str,
        /// Copied as Markdown while keys are masked.
        export_copied_masked: &'static str,
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

    /// Whether `translated` keeps `term`: as a word of its own — "Commit" as a verb too ("committen", "commité",
    /// and Chinese 提交, Japanese コミット, as git's own translations say it).
    fn kept_as(translated: &str, term: &str) -> bool {
        if term != "Commit" {
            return holds(translated, term);
        }
        let text = translated.to_lowercase();
        // "commit" at a word's start, any ending after it: committen, commité, committato.
        let as_word = text.match_indices("commit").any(|(at, _)| {
            !text[..at]
                .chars()
                .next_back()
                .is_some_and(char::is_alphabetic)
        });
        as_word || translated.contains("提交") || translated.contains("コミット")
    }

    /// What `english` holds that `translated` lost: a glossary word, a `/command`, a `--flag`, a `<name>`, a key.
    pub fn kept_in(english: &str, translated: &str) -> Vec<String> {
        let mut lost: Vec<String> = KEPT
            .iter()
            .filter(|term| holds(english, term) && !kept_as(translated, term))
            .map(|term| (*term).to_owned())
            .collect();
        // Around a word: punctuation, quotes, backticks and an English possessive (`--mcp-config`'s).
        let trim = |word: &'_ str| {
            let word = word.trim_end_matches("'s").trim_end_matches("’s");
            word.trim_matches(|c: char| "(),;:.…\"“”`".contains(c))
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
            assert_eq!(texts.tab_names.len(), 7, "{}", lang.code());
            assert_eq!(texts.tab_words.len(), 7, "{}", lang.code());
            assert_eq!(texts.sort_names.len(), 4, "{}", lang.code());
            assert_eq!(texts.key_words.len(), 13, "{}", lang.code());
            assert!(
                texts.key_words[..3].iter().all(|key| key.ends_with('+')),
                "{}",
                lang.code()
            );
            // A finished walk finds the file list open by its title's start.
            assert!(
                texts.files_looking.starts_with(texts.files_title),
                "{}",
                lang.code()
            );
            assert!(
                texts.files_truncated.starts_with(texts.files_title),
                "{}",
                lang.code()
            );
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
