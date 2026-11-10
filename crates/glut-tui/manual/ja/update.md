Glut が1日1回新しいバージョンを確認し、フッターに表示するかどうか。

/update on — 1日1回、バックグラウンドで `https://glut-ai.com/latest.txt` から最新バージョン番号を読み取り、現在より新しければフッターに控えめに表示します（`↑ 0.1.2`）
/update off — Glut は何も確認せず、フッターにも何も表示しません

/update だけで、新しい Glut があるかどうかと、インストール方法に応じた更新コマンド（`brew upgrade glut`、`cargo install glut-tui`、またはインストールスクリプト）を表示します。Glut が自分で更新することはなく、計画の実行、ループの1ラウンド、`glut run`、`glut acp`、アリーナでは確認しません。`~/.glut/prefs.json` に保存されます。`~/.glut/settings.json` の `"updateCheck": false` でオフにできます（/update で別の設定をした場合を除く）。
