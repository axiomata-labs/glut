このフォルダー用の短い `AGENTS.md` を書く、または既存のガイドを確認して短くします。

/init — ガイドがなければ Glut が `AGENTS.md` を書き、あれば確認して短く保ちます
/init --migrate — `CLAUDE.md` を `AGENTS.md` に移すかどうかを、もう一度尋ねます

`CLAUDE.md` しかないときは、`AGENTS.md` に移すかどうかを Glut が一度尋ねます（その後は `CLAUDE.md` が `AGENTS.md` を読み込み、どのハーネスにも同じガイドになります）; 「いいえ」は記憶されます。ガイドは毎ターン付いてくるので、/context でそのコストが分かります。
