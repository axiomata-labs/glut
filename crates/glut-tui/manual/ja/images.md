画像をストリームでどう描くかを選びます。

/images inline — ターミナルが表示できるなら実際の画像
/images blocks — トゥルーカラーの半ブロック
/images off — 画像の行だけ

/images だけを入力すると現在の選択を表示します。既定では、Glut は可能なターミナルでは inline で、それ以外ではブロックで描きます。`~/.glut/prefs.json` に保存されます。
