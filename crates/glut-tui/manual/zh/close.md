关闭一个 Pane，或结束一个后台代理。

/close — 关闭此 Pane（主 Pane 除外）；当它正在工作时，再次 /close 会停止它并关闭
/close <name> — 结束该后台代理，其一轮被停止
/close <n> — 在当前位置按编号、名称或角色关闭 Pane n；你仍留在自己的 Pane
单独 /close，若可关闭的不止此 Pane — 显示列表，此 Pane 在最前：⏎ 关闭所选项

会话保留供 /resume 使用。位于 Worktree 中的 Pane 会询问如何结束该 Worktree：保留它及其分支，接管它（合并到你当前所在的分支），或丢弃它。⌃X 0 也能关闭此 Pane。
