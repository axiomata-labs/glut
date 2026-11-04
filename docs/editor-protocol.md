# Glut's editor protocol, v1

How an editor plugin (`glut.nvim`, `glut-vscode` for VS Code and its forks, `glut-jetbrains`) and Glut talk, so Glut knows the file, the cursor
and the selection the owner works on, reads the editor's buffers, and shows its edits as the editor's own diffs. The
plan is T64c in `docs/plans/glut-tui.md`; the pattern is Gemini CLI's published IDE companion spec and Claude Code's
IDE integration, the protocol Glut's own.

## Roles

- **The plugin listens, Glut connects.** The plugin is an MCP server on loopback; Glut's MCP client connects to it
  as a server named `ide`, which `/mcp` never lists and no `mcp.json` names.
- **The plugin only shows and hands over.** Glut's gate, sandbox, guards and snapshots decide and act as ever: the
  plugin never writes a file for Glut, and a diff's answer is only the answer to Glut's own question.

## Transport

- MCP over HTTP: `POST http://127.0.0.1:<port>/mcp`, one JSON-RPC 2.0 message per request, answered with
  `Content-Type: application/json` (no event stream needed). A notification is answered `202` with no body.
- The plugin binds `127.0.0.1` alone, on a port the system picks.
- A connection with a question still waiting for its answer (a diff waits for the owner, up to Glut's ten minutes)
  is never closed for idleness; one with nothing waiting may be, after a minute, and one that has not shown the token
  after ten seconds.
- The plugin judges a request by its head before it keeps any of its body: without the token or not `POST /mcp` it
  answers and closes the connection; a length that is no plain number, a second length or any transfer encoding is
  refused. Glut never takes the editor's answer about a file it did not ask about: `readBuffer` and `diagnostics`
  echo the path, and Glut compares it.
- **Every request** carries `Authorization: Bearer <token>`; the plugin refuses any other with `401` before reading
  its body, comparing in constant time.
- Protocol version: Glut sends MCP `2025-06-18` in `initialize`; the plugin answers with the same and
  `serverInfo: {"name": "<editor>", "version": "<plugin version>"}`, capability `tools`.

## Discovery

The plugin writes `~/.glut/ide/<port>.json` once it listens, and removes it when it stops:

```json
{
  "version": 1,
  "port": 51234,
  "pid": 4711,
  "ideName": "Neovim",
  "workspaceFolders": ["/Users/me/project"],
  "authToken": "<32 hex characters from the system's random source>"
}
```

- `~/.glut/ide/` is `0700`, the file `0600`, written under another name and renamed into place.
- Where Glut's home is moved (`GLUT_HOME`, an absolute folder), the plugin writes to `$GLUT_HOME/.glut/ide/`: the
  folder Glut looks in. A relative `GLUT_HOME` counts for nothing, as in Glut.
- A fresh token at every start; never logged, never shown.
- A terminal the plugin starts Glut in carries `GLUT_IDE_PORT=<port>`.

Glut picks the editor so:

1. `GLUT_IDE_PORT`, where it names a file of a live process.
2. Else every file whose `workspaceFolders` holds Glut's working folder; one is taken, several are asked about once.
3. Glut reads only files owned by its own user, `0600`, with `version` 1; a file whose `pid` is not a live process
   of the same user is left alone (the next plugin start replaces it).
4. Before it sends the token, Glut checks that the process listening on `port` is `pid`: the port of an editor that
   died may be another program's.

## Tools

Results are MCP tool results: `{"content": [{"type": "text", "text": "<JSON>"}]}`; a refusal is
`{"isError": true, "content": [{"type": "text", "text": "<why>"}]}`. Paths are absolute; lines and characters are
1-based.

### Offered to the model (in `tools/list`)

| Tool | Arguments | Result JSON |
|---|---|---|
| `openFiles` | — | `{"files": [{"path", "active": bool, "dirty": bool}]}` — the buffers of real files, the active first |
| `diagnostics` | `{"path"}` | `{"path", "diagnostics": [{"line", "character", "severity": "error"\|"warning"\|"info"\|"hint", "message", "source"}]}` |

### Glut's own (callable, never in `tools/list`)

| Tool | Arguments | Result JSON |
|---|---|---|
| `getContext` | — | `{"activeFile": {"path", "cursor": {"line", "character"}, "selection": {"start": {…}, "end": {…}, "text"} \| null} \| null}` |
| `readBuffer` | `{"path"}` | `{"loaded": bool, "path"?, "text"?, "dirty"?}` — only a buffer of exactly that path the editor has loaded, its path echoed; it never opens a path |
| `openDiff` | `{"id", "path", "oldText": string \| null, "newText", "title", "editable"?}` | answered once decided: `{"outcome": "accepted" \| "rejected" \| "closed", "text"?}` — `text` only with `accepted`, where the owner changed the right side: the whole file as they left it |
| `closeDiff` | `{"id"}` | `{"closed": bool}` — the diff `id` goes; its `openDiff` answers `closed` |
| `changed` | `{"paths": [...]}` | `{}` — Glut wrote these files: the editor reads them again where no unsaved change stands |
| `notify` | `{"message"}` | `{}` — a word of Glut's to the owner, shown as plain text (control characters as spaces) |

- `openDiff` answers only when the owner decided (accepting writes nothing — Glut writes the file after its gate
  allows it) or the diff was closed; Glut waits for it as long as for any question of its gate and closes it with
  `closeDiff` when the TUI's card answered first.
- An `id` is unique across Glut's windows (`<pid>-<session>-<n>`): two Glut sessions may share one editor.
- An answer is the owner's deliberate act: the plugin never steals the focus for a diff and never accepts on a single
  stray key; Glut takes an accept sooner than 400 ms after the diff opened as a refusal.
- The selection in `getContext` is at most 16 KB, cut at the end; the plugin leaves out a buffer that is no file on
  disk.

## What Glut does with it

- With each of the owner's messages a short note of `getContext`, and the lines of each `@path#L10-22` reference
  read through `readBuffer` — never of a file outside the run's folders (judged where it really leads) or one the
  gate's read rules or the sandbox's read-deny list hold back; masked like a tool's result, framed as data. A
  `/loop` round carries none.
- `read_file` reads a loaded buffer through `readBuffer` (unsaved changes too) after every check of its own; an edit
  of a buffer with unsaved changes is refused until the owner saved it.
- A gate's question about `edit_file` or `write_file` goes to the editor as `openDiff` beside the TUI's card, with
  the call's whole text (never cut as its event is). Accepted with a `text` of the owner's, that text is written
  instead of the model's — the snapshot first, through the folder's guard — and the model is told so in a note of
  Glut's with the lines the owner changed.
- An LSP rename's question opens one `openDiff` per file (at most ten; more, and `notify` says to answer in Glut):
  an answer in any of them answers the whole rename, all or none, and the other tabs close; a tab closed unanswered
  leaves the others asking. Its diffs come with `"editable": false` — a rename replaces the old name or nothing, so
  the right side takes no change; a text of the owner's on one is no yes, the card stays.
- Each `openDiff` is a connection of its own, made after Glut sees again (`lsof`) that the port is the editor's
  process: the file's text goes out and the owner's may come back.
