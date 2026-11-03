Summarise the conversation so far, so it takes less of the model's window.

/compact — the model writes a summary that takes the place of the older messages

The system prompt, your latest message and whole steps stay; what the summary replaces is kept in the session's result store, never thrown away. Glut also compacts by itself shortly before the window is full (`compact` in `~/.glut/settings.json`). /context shows how full the window is.
