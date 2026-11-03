1 ターンの最大ステップ数。

/maxsteps — 現在の上限
/maxsteps <n> — ターンは n ステップで止まります（1 から 1000）

1 ステップはモデルの呼び出し 1 回です。上限に達するとターンは止まってそれを伝えます。空の入力で ⏎ を押すと「Go on where you stopped.」が送られ、作業が続きます。本当のブレーキは予算と時間制限です。`~/.glut/prefs.json` に保存。`~/.glut/settings.json` の `maxTurns` ですべての実行に、`glut run --max-turns` で 1 回に設定します。
