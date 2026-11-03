Glut がファイルや入力を開くエディターを選びます。

/editor — システムで見つかったエディター
/editor <command> — 自分のコマンド。`{file}` と `{line}` を置きたい場所に書きます

`{file}` がなければ、ファイルは末尾に追加されます。ウィンドウを持つエディターは、ファイルが閉じられるまで待つ必要があります（`code --wait`）。⌃X E で入力をそのエディターで編集し、/todo open と /memory open もそれを使います。`~/.glut/prefs.json` に保存されます。

/editor nvim +{line} {file}
