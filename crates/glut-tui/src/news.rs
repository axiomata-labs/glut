//! What is new in each version (#158): three to five short lines per release, in every language of the screen,
//! shown once after an update and again by `/news`. The changelog stays the long account for developers; a test
//! keeps every release of it from 0.1.1 on here.

use glut::lang::Lang;

/// One release's highlights.
pub struct Entry {
    pub version: &'static str,
    pub en: &'static [&'static str],
    pub de: &'static [&'static str],
    pub fr: &'static [&'static str],
    pub it: &'static [&'static str],
    pub es: &'static [&'static str],
    pub zh: &'static [&'static str],
    pub ja: &'static [&'static str],
    pub pt: &'static [&'static str],
}

/// The highlights of `entry` in `lang`.
pub fn lines(entry: &Entry, lang: Lang) -> &'static [&'static str] {
    match lang {
        Lang::En => entry.en,
        Lang::De => entry.de,
        Lang::Fr => entry.fr,
        Lang::It => entry.it,
        Lang::Es => entry.es,
        Lang::Zh => entry.zh,
        Lang::Ja => entry.ja,
        Lang::Pt => entry.pt,
    }
}

/// Every release's highlights, newest first.
pub const NEWS: &[Entry] = &[
    Entry {
        version: "0.1.2",
        en: &[
            "A question reads at a glance: what Glut asks about stands apart and in bold, the mode in the card's \
             title.",
            "Under it Glut says why it asks — \"it deletes recursively\", a rule of yours — where that is more than \
             the mode.",
            "The choices say how long they hold: `this session: git push *`, one under the other, so you see how far \
             each reaches.",
            "On Ubuntu the install script names the one AppArmor step the sandbox needs.",
        ],
        de: &[
            "Eine Frage ist auf einen Blick zu lesen: Worum es geht, steht frei und fett, der Modus im Titel der \
             Karte.",
            "Darunter sagt Glut, warum es fragt — „it deletes recursively“, eine deiner Regeln —, wo das mehr ist als \
             der Modus.",
            "Die Auswahl sagt, wie lange sie gilt: `diese Session: git push *`, eine unter der anderen — so siehst du, \
             wie weit jede reicht.",
            "Unter Ubuntu nennt das Installationsskript den einen AppArmor-Schritt, den die Sandbox braucht.",
        ],
        fr: &[
            "Une question se lit d'un coup d'œil : ce que Glut demande est à part et en gras, le mode dans le titre de \
             la carte.",
            "En dessous, Glut dit pourquoi il demande — « it deletes recursively », une de vos règles — quand c'est \
             plus que le mode.",
            "Les choix disent combien de temps ils valent : `cette session : git push *`, l'un sous l'autre, pour voir \
             jusqu'où va chacun.",
            "Sous Ubuntu, le script d'installation indique la seule étape AppArmor dont le bac à sable a besoin.",
        ],
        it: &[
            "Una domanda si legge a colpo d'occhio: ciò che Glut chiede sta a parte e in grassetto, la modalità nel \
             titolo della scheda.",
            "Sotto, Glut dice perché chiede — «it deletes recursively», una tua regola — quando è più della modalità.",
            "Le scelte dicono quanto valgono: `questa sessione: git push *`, una sotto l'altra, così vedi fin dove \
             arriva ciascuna.",
            "Su Ubuntu lo script di installazione indica l'unico passo AppArmor che serve alla sandbox.",
        ],
        es: &[
            "Una pregunta se lee de un vistazo: lo que Glut pregunta va aparte y en negrita, el modo en el título de \
             la tarjeta.",
            "Debajo, Glut dice por qué pregunta — «it deletes recursively», una regla tuya — cuando es más que el \
             modo.",
            "Las opciones dicen cuánto duran: `esta sesión: git push *`, una bajo la otra, para ver hasta dónde llega \
             cada una.",
            "En Ubuntu, el script de instalación indica el único paso de AppArmor que necesita el sandbox.",
        ],
        zh: &[
            "提问一目了然：Glut 要问的内容单独加粗显示，模式写在卡片标题里。",
            "下方说明为什么要问——“it deletes recursively”、你的某条规则——只要原因不仅仅是模式。",
            "选项写明有效多久：`本会话：git push *`，上下对齐，一眼看出各自的范围。",
            "在 Ubuntu 上，安装脚本会给出沙箱所需的那一步 AppArmor 设置。",
        ],
        ja: &[
            "質問がひと目で読めます。Glut が尋ねる内容は独立して太字で、モードはカードのタイトルに表示されます。",
            "その下に、なぜ尋ねるのかを示します（「it deletes recursively」やあなたのルールなど）。理由がモードだけの場合は表示しません。",
            "選択肢は有効期間を示します：`このセッション: git push *`。縦に並ぶので、それぞれの範囲がすぐわかります。",
            "Ubuntu では、サンドボックスに必要な AppArmor の手順をインストールスクリプトが示します。",
        ],
        pt: &[
            "Uma pergunta lê-se num relance: o que o Glut pergunta fica à parte e em negrito, o modo no título do \
             cartão.",
            "Por baixo, o Glut diz porque pergunta — «it deletes recursively», uma regra tua — quando é mais do que o \
             modo.",
            "As opções dizem quanto tempo valem: `esta sessão: git push *`, uma sob a outra, para veres até onde vai \
             cada uma.",
            "No Ubuntu, o script de instalação indica o único passo de AppArmor de que a sandbox precisa.",
        ],
    },
    Entry {
        version: "0.1.1",
        en: &[
            "Agents work together: they see who else works and where (⌃X T, the Team tab), ask each other and get the \
             answer back.",
            "A lead starts a small team of helpers — each in the sandbox, none with more rights than its lead — and \
             takes their work over with `take_work`, judged like its own edits.",
            "Fewer questions: `xargs` over listed files counts as reading.",
            "Glut shows in the footer when a newer version is out (`/update`), and once what is new (`/news`).",
        ],
        de: &[
            "Agenten arbeiten zusammen: Sie sehen, wer sonst arbeitet und wo (⌃X T, Reiter Team), fragen einander und \
             bekommen die Antwort zurück.",
            "Eine Leitung startet ein kleines Team von Helfern — jeder in der Sandbox, keiner mit mehr Rechten als die \
             Leitung — und übernimmt ihre Arbeit mit `take_work`, geprüft wie eigene Änderungen.",
            "Weniger Rückfragen: `xargs` über aufgezählte Dateien gilt als Lesen.",
            "Glut zeigt im Fuß, wenn eine neuere Version da ist (`/update`), und einmal, was neu ist (`/news`).",
        ],
        fr: &[
            "Les agents travaillent ensemble : ils voient qui d'autre travaille et où (⌃X T, l'onglet Équipe), se \
             sollicitent et reçoivent la réponse.",
            "Un meneur lance une petite équipe d'aides — chacune dans le bac à sable, aucune avec plus de droits que \
             lui — et reprend leur travail avec `take_work`, jugé comme ses propres modifications.",
            "Moins de questions : `xargs` sur des fichiers listés compte comme une lecture.",
            "Glut indique dans le pied quand une nouvelle version est sortie (`/update`), et une fois ce qui est neuf \
             (`/news`).",
        ],
        it: &[
            "Gli agenti lavorano insieme: vedono chi altro lavora e dove (⌃X T, la scheda Team), si chiedono aiuto e \
             ricevono la risposta.",
            "Un capo avvia una piccola squadra di aiutanti — ognuno nella sandbox, nessuno con più diritti del capo — \
             e \
             ne prende il lavoro con `take_work`, giudicato come le proprie modifiche.",
            "Meno domande: `xargs` su file elencati conta come lettura.",
            "Glut mostra nel piè di pagina quando c'è una versione più nuova (`/update`), e una volta le novità \
             (`/news`).",
        ],
        es: &[
            "Los agentes trabajan juntos: ven quién más trabaja y dónde (⌃X T, la pestaña Equipo), se piden ayuda y \
             reciben la respuesta.",
            "Un líder inicia un pequeño equipo de ayudantes — cada uno en el sandbox, ninguno con más derechos que el \
             líder — y toma su trabajo con `take_work`, juzgado como sus propios cambios.",
            "Menos preguntas: `xargs` sobre archivos listados cuenta como lectura.",
            "Glut muestra en el pie cuando hay una versión más nueva (`/update`), y una vez lo que es nuevo (`/news`).",
        ],
        zh: &[
            "智能体协同工作：它们能看到还有谁在工作、在哪里工作（⌃X T，团队标签页），互相请求并收到回答。",
            "负责人可以启动一个小助手团队——每个都在沙盒中，权限都不超过负责人——并用 `take_work` 接收它们的成果，像自己的修改一样接受审查。",
            "更少的询问：对列出的文件使用 `xargs` 算作读取。",
            "有新版本时 Glut 会在底栏提示（`/update`），并展示一次新内容（`/news`）。",
        ],
        ja: &[
            "エージェントが協力します。ほかに誰がどこで作業しているかが見え（⌃X T、チームタブ）、互いに依頼して答えを受け取れます。",
            "リーダーは小さなヘルパーチームを起動できます。各ヘルパーはサンドボックス内で動き、リーダー以上の権限は持ちません。成果は \
             `take_work` で取り込み、自分の編集と同じように審査されます。",
            "確認が減りました：列挙したファイルへの `xargs` は読み取りとして扱われます。",
            "新しいバージョンが出るとフッターに表示され（`/update`）、新機能を一度だけ紹介します（`/news`）。",
        ],
        pt: &[
            "Os agentes trabalham juntos: veem quem mais trabalha e onde (⌃X T, a aba Equipe), pedem ajuda uns aos \
             outros e recebem a resposta.",
            "Um líder inicia uma pequena equipe de ajudantes — cada um no sandbox, nenhum com mais direitos que o \
             líder \
             — e traz o trabalho deles com `take_work`, julgado como as próprias edições.",
            "Menos perguntas: `xargs` sobre arquivos listados conta como leitura.",
            "O Glut mostra no rodapé quando há uma versão mais nova (`/update`), e uma vez o que há de novo (`/news`).",
        ],
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    /// Where the README and the library's page name the version, it is this one: a release that forgets them
    /// fails here (they said 0.1.0 through 0.1.1).
    #[test]
    fn the_readme_and_the_librarys_page_name_this_version() {
        let version = env!("CARGO_PKG_VERSION");
        let readme = include_str!("../../../README.md");
        assert!(
            readme.contains(&format!("**Status: {version}, early.**")),
            "README.md's status line names another version than {version}"
        );
        let library = include_str!("../../../docs/library.md");
        assert!(
            library.contains(&format!("The crate is at version `{version}`")),
            "docs/library.md names another version than {version}"
        );
    }

    /// Every release of the changelog from 0.1.1 on has its highlights here, in every language.
    #[test]
    fn every_release_has_its_news_in_every_language() {
        let changelog = include_str!("../../../CHANGELOG.md");
        let releases: Vec<&str> = changelog
            .lines()
            .filter_map(|line| line.strip_prefix("## ["))
            .filter_map(|rest| rest.split_once(']'))
            .map(|(version, _)| version)
            .filter(|version| *version != "Unreleased" && *version != "0.1.0")
            .collect();
        for version in releases {
            let entry = NEWS
                .iter()
                .find(|entry| entry.version == version)
                .unwrap_or_else(|| panic!("no news for {version}"));
            for lang in [
                Lang::En,
                Lang::De,
                Lang::Fr,
                Lang::It,
                Lang::Es,
                Lang::Zh,
                Lang::Ja,
                Lang::Pt,
            ] {
                let lines = lines(entry, lang);
                assert!(
                    (3..=5).contains(&lines.len()),
                    "{version} {lang:?}: {}",
                    lines.len()
                );
            }
        }
        for lang in [
            Lang::En,
            Lang::De,
            Lang::Fr,
            Lang::It,
            Lang::Es,
            Lang::Zh,
            Lang::Ja,
            Lang::Pt,
        ] {
            assert!(
                NEWS.iter()
                    .all(|entry| lines(entry, lang).len() == entry.en.len()),
                "{lang:?}"
            );
        }
    }
}
