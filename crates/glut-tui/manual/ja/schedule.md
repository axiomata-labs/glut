Glut を閉じている間に実行するプラン: 決まった時刻か、一定の間隔でタスクを実行します。

/schedule — 実行タブ（⌃X R）: すべてのプランと次の時刻、前回の結果
/schedule list — プランをストリームに一覧します
/schedule add <name> [flags] <task> — プランを作ります。同じ名前のプランがあれば変更します
/schedule run <name> — 今すぐ、Glut の横で実行します
/schedule pause <name> — 再開するまで止めます
/schedule resume <name> — 決まった時刻に再び実行されます
/schedule remove <name> — プランとその記録を削除します; セッションは残ります

フラグは `glut schedule add` のものです:
- `--at 07:00` 時刻。`--days mo-fr` または `sa,su` を添えて曜日を指定（なければ毎日）
- `--every 2h` 10m から 7d までの間隔; --at の代わりに指定
- `--mode ask|build|plan|dont_ask|auto` ルールなしで許可すること; 指定がなければ ask
- `--allow <rule>`、`--deny <rule>` Claude Code 形式のルール、たとえば `Bash(cargo test:*)`; 繰り返し指定可
- `--folder <path>` 横に置く専用のフォルダー; 繰り返し指定可
- `--max-usd 1`、`--max-minutes 30` 一回の実行にかけてよい最大額と時間: 指定がなければ 1 $ と 30 分
- `--max-tokens 1M` サブスクリプション上で一回の実行が使ってよい最大 token 数: 指定がなければ 1M
- `--replace` 指定したルールとフォルダーが、プランのものに加わらずに置き換わります
- `--cwd <path>`、`--provider <id>`、`--model <id>` そのフォルダー（なければこのフォルダー）、プロバイダー、モデル; 作成時に固定されます

タスクはメッセージ、または /skill <name> [what for] です。実行を始めるのは launchd で、ログインしている間だけです; スリープ中に過ぎた時刻は、復帰時に一度だけ補われます。答える人がいないので、確認はすべて拒否されて記録されます: 必要なものは許可しておいてください。実行ごとに別のセッションが保存され、macOS の通知で知らされます。実行タブでは、⏎ でプランや実行のセッションを開き、r で今すぐ実行、p で一時停止・再開、d で削除します。

/schedule add tests --at 07:00 --days mo-fr --allow "Bash(cargo test:*)" テストを実行して、失敗を報告する
