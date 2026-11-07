//! The command line's help in the language read now (T48): clap's derive writes it in
//! English; the tree is built, then every about and help text is looked up by its English words in the catalog, and
//! the headings are the language's. Command syntax (`[OPTIONS]`, `<MODEL>`) and clap's own errors stay as clap
//! writes them.

use clap::Command;
use clap::builder::{Resettable, StyledStr};
use glut::lang::Lang;

/// `cmd` with its help in the language read now; in English, as clap writes it.
pub fn localized(mut cmd: Command) -> Command {
    // The help and version flags and the `help` subcommand exist only once the tree is built.
    cmd.build();
    if glut::lang::current() == Lang::En {
        return cmd;
    }
    localize(cmd)
}

/// One command of the tree, its arguments and its subcommands.
///
/// A translated text replaces the long form too: the catalog holds one text per item, and a doc comment's second
/// paragraph (clap's long help) would otherwise show in English under the translated first.
fn localize(mut cmd: Command) -> Command {
    let texts = crate::texts::t();
    let said = |english: &str| {
        texts
            .cli
            .iter()
            .find(|(source, _)| *source == english)
            .map(|(_, text)| *text)
    };
    if let Some(about) = cmd.get_about().and_then(|about| said(&about.to_string())) {
        cmd = cmd.about(about).long_about(Resettable::<StyledStr>::Reset);
    }
    let (options, arguments) = (texts.cli_headings[1], texts.cli_headings[2]);
    let ids: Vec<String> = cmd
        .get_arguments()
        .map(|arg| arg.get_id().to_string())
        .collect();
    for id in ids {
        cmd = cmd.mut_arg(id, |arg| {
            let heading = if arg.is_positional() {
                arguments
            } else {
                options
            };
            let help = arg.get_help().and_then(|help| said(&help.to_string()));
            let arg = arg.help_heading(heading);
            match help {
                Some(help) => arg.help(help).long_help(Resettable::<StyledStr>::Reset),
                None => arg,
            }
        });
    }
    let subcommands: Vec<String> = cmd
        .get_subcommands()
        .map(|sub| sub.get_name().to_owned())
        .collect();
    for name in subcommands {
        cmd = cmd.mut_subcommand(name, localize);
    }
    cmd.subcommand_help_heading(texts.cli_headings[3])
        .help_template(format!(
            "{{before-help}}{{about-with-newline}}\n{} {{usage}}\n\n{{all-args}}{{after-help}}",
            texts.cli_headings[0]
        ))
}

/// The about and help texts of `cmd`'s tree that `lang`'s catalog has no row for — for the tests that hold every
/// language complete.
pub fn untranslated(cmd: &Command, lang: Lang) -> Vec<String> {
    let rows = crate::texts::of(lang).cli;
    let mut missing = Vec::new();
    let mut seen = |text: Option<&StyledStr>| {
        let text = text.map(ToString::to_string).unwrap_or_default();
        if !text.is_empty()
            && !rows.iter().any(|(source, _)| *source == text)
            && !missing.contains(&text)
        {
            missing.push(text);
        }
    };
    seen(cmd.get_about());
    for arg in cmd.get_arguments() {
        seen(arg.get_help());
    }
    for sub in cmd.get_subcommands() {
        for text in untranslated(sub, lang) {
            if !missing.contains(&text) {
                missing.push(text);
            }
        }
    }
    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_help_row_keeps_its_flags_commands_and_terms() {
        for lang in Lang::ALL.into_iter().filter(|lang| *lang != Lang::En) {
            for (english, text) in crate::texts::of(lang).cli {
                let lost = crate::texts::check::kept_in(english, text);
                assert!(lost.is_empty(), "{}: {lost:?} lost in {text}", lang.code());
            }
            assert_eq!(crate::texts::of(lang).cli_headings.len(), 4);
        }
    }

    #[test]
    fn a_help_reads_in_the_language_and_keeps_its_syntax() {
        let cmd = || {
            Command::new("glut").about("Glut, in the terminal").arg(
                clap::Arg::new("cwd")
                    .long("cwd")
                    .help("The folder the session works in"),
            )
        };
        glut::lang::set_for_thread(Some(Lang::De));
        let help = localized(cmd()).render_help().to_string();
        assert!(help.contains("Glut, im Terminal"), "{help}");
        assert!(help.contains("Verwendung: glut [OPTIONS]"), "{help}");
        assert!(
            help.contains("Der Ordner, in dem die Session arbeitet"),
            "{help}"
        );
        assert!(help.contains("Hilfe anzeigen"), "{help}");
        glut::lang::set_for_thread(None);
        let english = localized(cmd()).render_help().to_string();
        assert!(
            english.contains("The folder the session works in"),
            "{english}"
        );
    }
}
