Sign in to a subscription instead of a key — today your ChatGPT Plus or Pro plan.

/login — the subscriptions, each with how it stands; ⏎ signs in, or out
/login <provider> — signs in to that one at once, as /login chatgpt
/login <provider> <address> — over SSH: the address the browser ended on, pasted back
/login openrouter — OpenRouter hands Glut a key of its own, kept like one typed; limit or delete it at openrouter.ai/settings/keys

The address is shown first; ⏎ opens it in your browser. Glut keeps the sign-in in the Keychain (or `~/.glut/credentials.json`) and renews it by itself; signing out revokes it at OpenAI.

On the plan a call costs no money, but the plan's usage limits hold: when one is reached the turn ends and Glut points to ChatGPT Settings → Usage, where you also limit what each app may use. In the shell: glut providers login|logout <provider>.
