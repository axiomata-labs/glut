Glut が ~/.glut に保存するデータが際限なく増えないようにします。

/clean — Glut が保存しているもの（項目ごと）と、期限に従って削除できるもの
/clean now — すぐにバックグラウンドで整理
/clean keep — 期限の一覧
/clean keep <kind> <days>d — 期限を設定：snapshots、journals、sessions、results、lean

削除されるもの：期限（7 日）を過ぎた実行のスナップショット — フォルダーのスナップショットがすべて期限切れになると、フォルダーの有無にかかわらずストア全体を削除；未完了の実行のジャーナル（7 日）；90 日間書き込まれていないセッションと、そのイベントログと長い結果；セッションのない長い結果は 30 日後；保持中のどのセッションも参照していない画像；lean.jsonl の 90 日より古い行。エクスポートは数えるだけです：あなたのファイルだからです。

Glut は 1 日 1 回、起動時に自動で整理し — ここでも `glut run` でも — 解放した量を伝えます。ビルド出力とキャッシュ（CACHEDIR.TAG のあるフォルダー、node_modules/ の下、Cargo プロジェクトの target/ の下）はスナップショットを取りません。

期限は `~/.glut/clean.json` に保存されます。コマンドライン：`glut clean`、`glut clean --dry-run`、`glut clean keep sessions 30d`；`glut clean --dry-run --json` はプログラム向けにレポートを書き出します。
