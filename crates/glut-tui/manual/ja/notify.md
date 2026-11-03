ウィンドウが前面にないとき、ターンの終了や Glut の確認を知らせます。

/notify on — ターンの終了とすべての確認
/notify asks — 確認のみ
/notify off — 何も知らせない
/notify system — macOS 自身の通知で（クリックするとターミナルではなくスクリプトエディタが開きます）
/notify after <s> — ターンの終了は、その秒数以上実行した場合にだけ知らせます（既定は 10、最大 3600）
/notify tiles on|off — ウィンドウ右上のタイル

/notify だけを入力すると現在の設定を表示します。Glut は、ターミナル自身の通知が使えると分かっていればそれを使い、ベルも鳴らします。通知には Glut、プロジェクト、Pane、起きたことが書かれ、モデルの言葉は含まれません。`~/.glut/prefs.json` に保存され、`~/.glut/settings.json` の `notify` より優先されます。
