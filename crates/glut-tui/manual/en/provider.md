Switch to another model provider.

/provider — the providers you set up
/provider <id> — switches to that one, on its default model
/provider key [id] — changes only that provider's key (the default provider without an id)

Providers are kept in `~/.glut/providers.json`, their keys in the Keychain (or `~/.glut/credentials.json`); `glut providers add <id>` adds one. The conversation goes on, and the next start begins with the provider you picked.

The key is typed hidden and tried against the provider's model list before it replaces the old one; if that fails you choose whether to keep it anyway, and otherwise the old key stays. The session takes a kept key at once. A provider that signs in, needs no key or takes its key from an environment variable is told so. In the shell: `glut providers key <id>`.
