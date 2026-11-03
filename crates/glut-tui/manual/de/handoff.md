Die Arbeit an eine spätere Session dieses Ordners übergeben.

/handoff write — das Modell schreibt, was getan wurde, wie es steht und was als Nächstes kommt
/handoff read — nimmt die Übergabe des Ordners in diese Session

Eine Übergabe liegt in `~/.glut/handoffs`, eine pro Ordner. Nach dem Lesen wird sie beiseitegelegt, damit keine spätere Session sie zweimal liest. Sie zu schreiben ist ein eigener, bezahlter Aufruf; das Gespräch bleibt, wie es ist.
