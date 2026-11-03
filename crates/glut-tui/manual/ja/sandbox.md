Glut が実行するコマンドの扱いと、コマンドが Git フォルダに書き込めるかどうか。

/sandbox — ここでのサンドボックス（macOS: Seatbelt、Linux: bubblewrap と seccomp）と Git のスイッチ
/sandbox git read-only — どのコマンドも Git フォルダに書き込みません: サンドボックスからはどれもコミットしません
/sandbox git writable — コマンドは再び Git フォルダに書き込めます。コミットもできます
/sandbox required on — サンドボックスのない場所ではコマンドは実行されません（bubblewrap がない、権限のないコンテナなど）。off でルールだけに守られて実行されます

サンドボックスは、コマンドにあなたのフォルダへの書き込みだけを許し、秘密と .env ファイル以外のすべてを読ませ、ネットワークには Glut のプロキシ経由でのみ届かせます。Linux では、まだ存在しない名前を拒否できません: macOS が拒否するものをコマンドが作ると — 新しい .envrc、.git、.git/commondir — 現れた瞬間に ~/.glut/quarantine へ移され、コマンドの後にもう一度確認されます。ちょうどその瞬間に動くあなたのツールはまだそれに出会う可能性があります。Git フォルダが読み取り専用なら、そこには何も作られません。

スイッチは次のコマンドから、すべてのセッションとウィンドウで有効で、~/.glut/sandbox.json に保存されます — コマンドが書くことはありません。glut sandbox show と glut sandbox git read-only|writable は Glut の外で同じことをします。
