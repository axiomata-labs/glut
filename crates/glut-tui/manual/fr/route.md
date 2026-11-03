Quels hôtes servent le modèle sur OpenRouter (les autres servent chaque modèle depuis un seul hôte).

/route — l'ordre que Glut envoie maintenant, chaque hôte avec sa raison, son prix et sa vitesse
/route prefer <hosts> — ceux-ci d'abord, dans cet ordre (xiaomi deepinfra)
/route avoid <hosts> — ceux-ci jamais
/route quant <levels> — seulement ces quantifications (fp8 bf16)
/route strict on|off — seulement les hôtes préférés, aucun autre en secours
/route off — les épinglages de ce modèle s'en vont
/route rule on|off — la règle pour tous les modèles
/route rule margin|uptime|speed|wait <n> — ses nombres

OpenRouter sert un modèle depuis de nombreux hôtes, avec des quantifications, des prix et des vitesses différents. Glut lit les hôtes d'un modèle une fois par jour et les classe selon sa règle : pleine précision (bf16, fp16) avant fp8 sauf si elle coûte plus de 40 % de plus, un hôte qui n'indique aucune quantification entre fp8 et fp4 — celui du fabricant du modèle comme plein —, fp4 seulement si personne d'autre ne le sert ; un hôte sous 95 % de disponibilité, sous 30 tokens par seconde ou au-delà de 10 secondes jusqu'au premier jeton passe à la fin. L'ordre part avec chaque requête, secours permis, si bien qu'aucune requête n'échoue à cause de la règle. Les épinglages d'un modèle passent avant la règle. Tout est gardé dans `~/.glut/providers.json`.
