# Security policy

Glut runs a language model's commands on your machine. Its gate, its sandbox and its other defences are described in
[docs/security.md](docs/security.md), together with the risks they knowingly leave open.

## Reporting a vulnerability

Please **do not open a public issue**. Report it privately through GitHub:
[Report a vulnerability](https://github.com/axiomata-labs/glut/security/advisories/new) (the repository's *Security* tab).

Tell us what an attacker can do and how: the steps, the version (`glut --version`), the system, and — if a model
plays a part — what text it was given or read. A proof of concept helps; one that harms someone else's system does
not.

You will hear back within a week. Once a fix is out, the advisory is published with your name, if you want it there.

## What counts

Anything that lets a model, a file it reads, a tool's output, a web page, an MCP server, an editor or another program
do more than Glut's documented limits allow — for example:

- a command that escapes the sandbox, or writes, reads or connects where the policy refuses;
- a way past the gate: something that runs without the question it should have needed;
- a secret that reaches the model, a log or a file despite the masks, where the masks claim to catch it;
- text that Glut presents as its own (`[glut]`) or as the owner's while it is not;
- a provider key or a subscription token sent to a host it does not belong to.

What [docs/security.md](docs/security.md) lists as a known limit is not a vulnerability by itself, but a practical
attack through it still interests us.

## Supported versions

Glut is at 0.x: fixes go into the newest release only.
