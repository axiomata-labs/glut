メインの横の Pane に新しいセッションを開きます。

/split — 新しい Pane; git リポジトリでは Glut が作業場所を尋ねます
/split here — このフォルダーで、あなたの横で作業します
/split tree [branch] — 専用のブランチの新しい Worktree で作業します
/split resume — 以前のセッションを、一覧から選んで開きます
/split as <role> — 役割を担当させます（/role で一覧）

メインの横に最大六つの Pane が、三つずつ二段で並びます。⌃P で Pane を巡回し、⌃X 2 でも分割でき、⌃X } と ⌃X { でメイン Pane を広く・狭くします。ここでの二つの Pane は同じファイルを変更します。Worktree なら、/close が終わらせ方を尋ねるまで分けておけます。

/split tree fix-login as reviewer
