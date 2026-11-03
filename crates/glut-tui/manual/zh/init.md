为此文件夹写一份简短的 `AGENTS.md`，或检查并精简已有的指南。

/init — 没有指南时 Glut 写 `AGENTS.md`；有指南时检查它并保持简短
/init --migrate — 再次询问是否把 `CLAUDE.md` 移入 `AGENTS.md`

只有 `CLAUDE.md` 时，Glut 会询问一次是否把它移入 `AGENTS.md`（`CLAUDE.md` 随后导入它，所有 harness 共用一份指南）；回答否则会被记住。指南每一轮都会随带发送：/context 显示它们的开销。
