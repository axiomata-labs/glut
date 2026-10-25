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
