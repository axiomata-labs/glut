# Contributing to Glut

Thank you for wanting to help. Glut is young and has one maintainer, so a short note before a large change saves both
of us time: open an issue first and say what you have in mind.

## Reporting a bug

Open an [issue](https://github.com/axiomata-labs/glut/issues) with:

- what you did, what you expected, what happened instead;
- `glut --version`, your system (macOS version, or the Linux distribution and kernel), your terminal;
- the provider and model, if a model was involved;
- what the screen showed — copy text rather than screenshots where you can. `/export` writes a session as Markdown.

A **security** problem is not an issue: see [SECURITY.md](SECURITY.md).

## Building

```sh
git clone https://github.com/axiomata-labs/glut
cd glut
cargo build                       # the library and the program `glut`
cargo run --bin glut              # the terminal UI in the current folder
```

Glut depends on [`axiomata-files`](https://github.com/axiomata-labs/Axiomata-Foundation), the guarded file service, from
its own repository. To work on both at once, check it out beside Glut and patch it locally in `.cargo/config.toml`
(not checked in):

```toml
[patch."https://github.com/axiomata-labs/Axiomata-Foundation"]
axiomata-files = { path = "../Axiomata-Foundation/crates/axiomata-files" }
```

## The website

The site is built from the same files: the documents in `docs/` and beside this one, the manual's pages, and the
layout, pages and files of `site/`. No framework, nothing to install:

```sh
cargo run -p xtask -- site serve   # http://127.0.0.1:8000, built again when a source changes
cargo run -p xtask -- site check   # every link and anchor between the pages leads somewhere
```

A link in a document to another document becomes a link between their pages; a link to any other file of the
repository leads to it on GitHub.

## Before a pull request

Every change passes the same gate:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
```

Tests that call a real model are `#[ignore]`d (`crates/glut/tests/live.rs`, run with the `GLUT_LIVE_*` variables).
The Linux sandbox is tested in a container; the CI runs it on every push.

Please also:

- **Read [AGENTS.md](AGENTS.md).** Its "traps" are the invariants that keep Glut safe — the gate, the sandbox, the
  masks, what a model may and may not reach. A change that touches one of them needs a test that would fail without
  it, and a word in the pull request about why it holds.
- **Keep the style.** Lines of at most 120 characters; rustdoc on every public item; comments say *why*, not *what*;
  no commented-out code.
- **Translate what you add.** Every text the interface shows exists in all eight languages
  (`crates/glut-tui/src/texts/`), and every command has a manual page in each (`crates/glut-tui/manual/<lang>/`).
  If you cannot write a language, write English in its place and say so — the tests check that no key is missing.
- **A new setting** needs a command or a view in the interface; a file in `~/.glut` is only where it is kept.

## Licensing

By contributing you agree that your work is licensed as Glut is: under either of the Apache License 2.0 or the MIT
license, at the user's option, without additional terms.
