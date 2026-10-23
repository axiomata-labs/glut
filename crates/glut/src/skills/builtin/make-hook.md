---
name: make-hook
description: Makes or changes one of the owner's hooks in Glut — ~/.glut/hooks.json, a command that runs before or after tool calls and may block them. Use when the owner wants something checked or refused every time ("stop pushes to main", "check each edit").
---
You make or change one of the owner's hooks with the owner. Hooks live in the owner's `~/.glut/hooks.json`, in Claude
Code's shape — never a project's: a project's hooks are never run. A hook only ever vetoes: it may block a call or
the answer, and `SessionStart` may add context; it never allows what Glut's rules refuse.

1. Look first: `glut_file` with `action: read`, `scope: owner`, `kind: hooks` (the owner is asked: hook commands can
   hold secrets). A hook is added to what is there — the whole file is written, so keep every hook already in it.
2. Ask the owner only what is still open, in one short message:
   - the event: `PreToolUse` (before a call — it can block it), `PostToolUse` (after a call — it can block, and the
     model then gets the hook's reason instead of the result), `Stop` (before the answer — it can block it, and the
     run goes on), `SessionStart` (its output becomes context). A `Stop` hook must look at `stop_hook_active` in its
     input and exit 0 when it is true, or it blocks every answer until the run's limit;
   - for the tool events, a `matcher`: a regex on the tool's name, not anchored — `bash` also matches
     `bash_output`, so write `^bash$`, `^(edit_file|write_file)$`; none matches every tool. Glut's tool names: `bash`, `read_file`, `write_file`, `edit_file`, `grep`, `glob`, `web_fetch`,
     `task`, an MCP tool as `mcp__<server>__<tool>`;
   - the command: it runs in the owner's shell, sandboxed like the model's commands, gets the call as JSON on stdin
     (`tool_name`, `tool_input`; after the call also `tool_response`). Exit 0 lets things be; exit 2 blocks, with
     stderr as the reason. Other exits and timeouts are only a note. `timeout` in seconds is optional (60).
3. Write the whole file:

   ```json
   {
     "hooks": {
       "PreToolUse": [
         { "matcher": "^bash$",
           "hooks": [ { "type": "command", "command": "jq -e '(.tool_input.command // \"\") | test(\"git push.*main\") | not' >/dev/null || { echo 'no pushes to main' >&2; exit 2; }" } ] }
       ]
     }
   }
   ```
   A longer check is better a script of the owner's that the hook calls; say so rather than writing it here. A hook
   is a guard rail, not a wall: a command can be written many ways (`git -C . push`, a branch in a variable), and a
   hook that crashes lets things be — Glut's rules are what refuses. Say so when the owner relies on one.
   Never put a key, a token or a password into a hook: name an environment variable and tell the owner.
4. Show the owner the whole new file and say what each hook does and when it blocks. Only after their yes call
   `glut_file` `action: write`, `scope: owner`, `kind: hooks`; Glut asks them once more with the content, and keeps
   the old file as `hooks.json.bak`.
5. Say that it counts from the next session.
