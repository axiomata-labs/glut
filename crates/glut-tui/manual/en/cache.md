Choose how a resting session keeps the provider's prompt cache, so the next turn starts warm.

/cache ping — Glut reads the cache again every few minutes while the session rests, for up to 30 minutes
/cache 1h — Anthropic keeps it an hour (each write costs twice); other providers are read to keep it
/cache off — not kept: a pause past the provider's few minutes starts cold

/cache alone shows the choice now; ping is the default. Kept in `~/.glut/prefs.json`.
