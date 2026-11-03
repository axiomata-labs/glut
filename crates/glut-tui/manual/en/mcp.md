See the MCP servers and how each stands, sign in to a hosted one, add or remove one.

/mcp — every server: connected with its tools, waiting for a sign-in, or failed
/mcp login <name> — signs in to a hosted server; its address opens in your browser on ⏎
/mcp login <name> <address> — over SSH: the address the browser ended on, pasted back
/mcp logout <name> — signs out and forgets its token
/mcp add <name> <url> — adds a hosted server to `~/.glut/mcp.json` and connects it
/mcp add <name> -- <command> [args] — adds a server Glut starts itself
/mcp remove <name> — takes it out of `~/.glut/mcp.json` and out of the session

A server that offers no sign-in Glut can use asks for a token, typed hidden. A project's own servers are the project's to change: /mcp remove takes only yours. `glut mcp` does the same outside a session.

While one of its tools runs, a server may ask you: a form (⇥ ↑↓ between the fields, space ticks, ⏎ sends, Esc closes, ⌃D declines) or a page to open (o). Only you answer it, never the model; a form that asks for a password or a key is declined, and with nobody there every question is declined.
