キーの代わりにサブスクリプションでサインインします — 現在は ChatGPT Plus または Pro プランです。

/login — サブスクリプションの一覧と各状態。⏎ でサインインまたはサインアウト
/login <provider> — すぐにサインインします（例：/login chatgpt）
/login <provider> <address> — SSH 経由：ブラウザーが最後に表示したアドレスを貼り戻します
/login openrouter — OpenRouter が Glut 専用のキーを発行し、入力したキーと同じように保存します。上限や削除は openrouter.ai/settings/keys で

まずアドレスが表示され、⏎ でブラウザーが開きます。Glut はサインインをキーチェーン（または `~/.glut/credentials.json`）に保存し、自動で更新します。サインアウトすると OpenAI 側でも取り消されます。

プランでは呼び出しごとの料金はかかりませんが、プランの利用上限があります。上限に達するとターンが終わり、Glut は ChatGPT の設定 → 利用状況 を案内します。各アプリの利用もそこで制限できます。シェルでは glut providers login|logout <provider> です。
