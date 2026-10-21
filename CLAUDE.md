# CLAUDE.md

Claude Code's entry point. Everything is in [`AGENTS.md`](AGENTS.md), imported here.

@AGENTS.md

## Sub-agents

The owner's global rules apply with the Rust variants (`rust-test-engineer`, `rust-dependency-auditor`,
`rust-performance-analyzer`). One combined review agent (model `sonnet`, defects and convention breaks only) before a
commit; `security-auditor` in full whenever the sandbox, the gate, the file tools, MCP or hooks change.
