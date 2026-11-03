OpenRouter でモデルを提供するホスト（他のプロバイダーは各モデルを一つのホストで提供します）。

/route — Glut がいま送る順序。各ホストに理由・価格・速度
/route prefer <hosts> — これらを先に、この順で（xiaomi deepinfra）
/route avoid <hosts> — これらは使わない
/route quant <levels> — これらの量子化だけ（fp8 bf16）
/route strict on|off — 優先ホストだけ。ほかを予備にしない
/route off — このモデルの固定を外す
/route rule on|off — すべてのモデルのルール
/route rule margin|uptime|speed|wait <n> — ルールの数値

OpenRouter は一つのモデルを多くのホストで、量子化・価格・速度を変えて提供します。Glut はモデルのホストを一日一回読み、ルールで並べます：フル精度（bf16、fp16）を fp8 より先に、ただし 40 % 以上高ければ fp8。量子化を明かさないホストは fp8 と fp4 の間（モデル製作者自身のホストはフル扱い）、fp4 はほかに提供者がいないときだけ。稼働率 95 % 未満、毎秒 30 トークン未満、最初の Token まで 10 秒超のホストは最後へ。順序は予備を許して毎回のリクエストに付くので、ルールのせいで失敗することはありません。モデルの固定はルールより優先されます。すべて `~/.glut/providers.json` に保存されます。
