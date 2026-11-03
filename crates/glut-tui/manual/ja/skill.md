Skill を始めます: ある種のタスクのための手順で、モデルが読み込んで従います。

/skill — 選べる Skill
/skill <name> [what for] — その Skill を、名前に続けて書いた内容に対して実行します

Glut には自分用を書くための make-agent、make-skill、make-hook が付属します。自分の Skill は `~/.glut/skills` に、プロジェクトのものは `.glut/skills` に置きます（信頼したフォルダーでのみ読み込まれます）; 同じ名前があれば Glut のものは隠れます。

/skill make-hook main への git push をブロックする
