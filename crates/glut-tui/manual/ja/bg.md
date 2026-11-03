バックグラウンドでエージェントを起動します: Pane のないセッションで、名前で呼び出します。

/bg — agent（または agent-2、…）という名前のエージェント; git リポジトリでは Glut が作業場所を尋ねます
/bg <name> — その名前のエージェント（小文字、数字、ダッシュ）
/bg as <role> — 役割を担当し、その名前になります
/bg here — このフォルダーで作業します
/bg tree [branch] — 新しい Worktree で作業します

/send <name> で仕事を渡し、/peek <name> でそのストリームを読み、/show <name> で前に出し、/close <name> で終了します。各エージェントは別々に作業し、別々にコストがかかります: 七つ目の前に、Glut はウィンドウごとに一度確認します。⌃X B は役割を選んで起動し、⌃X A で一覧します。

/bg docs-check as docs
