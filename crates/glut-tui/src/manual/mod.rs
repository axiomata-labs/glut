//! The commands' manual pages (the help box, `/help`): what each `/` command does in full, in each of Glut's
//! languages — one file per language, English the source, every command in `commands::COMMANDS`' order. The texts
//! are Markdown files in the package's `manual/<lang>/`, embedded here and rendered by the website.

mod de;
mod en;
mod es;
mod fr;
mod it;
mod ja;
mod pt;
mod zh;

use glut::lang::Lang;

/// The pages of `lang`.
fn pages(lang: Lang) -> &'static [(&'static str, &'static str)] {
    match lang {
        Lang::En => &en::PAGES,
        Lang::De => &de::PAGES,
        Lang::Fr => &fr::PAGES,
        Lang::It => &it::PAGES,
        Lang::Es => &es::PAGES,
        Lang::Zh => &zh::PAGES,
        Lang::Ja => &ja::PAGES,
        Lang::Pt => &pt::PAGES,
    }
}

/// Command `name`'s page in the language read now, else in English; empty for no command of that name.
pub fn page(name: &str) -> &'static str {
    let find = |pages: &'static [(&'static str, &'static str)]| {
        pages
            .iter()
            .find(|(page, _)| *page == name)
            .map(|(_, text)| *text)
    };
    let text = find(pages(glut::lang::current()))
        .or_else(|| find(&en::PAGES))
        .unwrap_or_default();
    // A Markdown file ends with a line break the help box does not show.
    text.strip_suffix('\n').unwrap_or(text)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::COMMANDS;

    #[test]
    fn every_language_has_every_commands_page_in_order_keeping_its_words() {
        let names: Vec<&str> = COMMANDS.iter().map(|command| command.name).collect();
        for lang in Lang::ALL {
            let written: Vec<&str> = pages(lang).iter().map(|(name, _)| *name).collect();
            assert_eq!(written, names, "{}: every command, in order", lang.code());
            for ((_, english), (name, text)) in en::PAGES.iter().zip(pages(lang)) {
                assert!(
                    !text.trim().is_empty(),
                    "{}: /{name} has no page",
                    lang.code()
                );
                let lost = crate::texts::check::kept_in(english, text);
                assert!(lost.is_empty(), "{}: /{name} lost {lost:?}", lang.code());
            }
        }
    }
}
