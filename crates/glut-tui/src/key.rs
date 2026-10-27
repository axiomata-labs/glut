//! `/provider key [id]` (`docs/plans/glut-tui.md`, T70): a provider's key changed alone — asked for without echo in
//! the screen's hidden entry (as `/mcp`'s sign-in), checked against the provider's model list before the old one is
//! replaced, kept or not as the owner says. Only the key under `<id>@<host>` changes; what the work beside the screen
//! comes to arrives as [`Step`]s, as `/mcp`'s does.

use std::sync::Arc;

use glut::providers::{self, Kept, Providers};
use tokio::sync::{mpsc, oneshot};

use crate::session::{Parts, Report};
use crate::transcript::Tone;

/// What the key's change came to, for the screen.
pub enum Step {
    /// A note for the stream, in the owner's words already.
    Note(String, Tone),
    /// The new key does not prove itself: why — said before this comes —, and where the owner's word on keeping it
    /// anyway goes back. The task holds the key until then; no word at all keeps the old one.
    Ask {
        reason: String,
        answer: oneshot::Sender<bool>,
    },
    /// The new key was kept (`kept`), or the old one stays: the provider whose key it was.
    Kept { id: String, kept: bool },
}

fn note(reports: &mpsc::UnboundedSender<Report>, text: String, tone: Tone) {
    let _ = reports.send(Report::Key(Step::Note(text, tone)));
}

/// `/provider key [id]`, after the owner typed the new key hidden: the key checked against the provider's model list
/// before it replaces the old one, and kept only then — or as the owner says when the check fails (Q1).
pub fn change(
    parts: Arc<Parts>,
    id: String,
    account: String,
    typed: String,
    reports: mpsc::UnboundedSender<Report>,
) {
    // The same as a key the terminal reads: nothing empty, nothing pasted as a whole line (T56).
    let typed = typed.trim().to_owned();
    if typed.chars().any(|c| c.is_control() || c == ' ') {
        return note(&reports, tr!(secret_odd).to_owned(), Tone::Fault);
    }
    if typed.is_empty() {
        return note(&reports, tr!(secret_none).to_owned(), Tone::Quiet);
    }
    if typed.len() > providers::MAX_KEY_BYTES as usize {
        return note(
            &reports,
            tr!(key_too_long, max = providers::MAX_KEY_BYTES),
            Tone::Fault,
        );
    }
    tokio::spawn(async move {
        let providers = match Providers::load(&parts.home) {
            Ok(providers) => providers,
            Err(err) => {
                return note(
                    &reports,
                    tr!(key_error, reason = err.to_string()),
                    Tone::Fault,
                );
            }
        };
        let store = match providers.store(&parts.home) {
            Ok(store) => store,
            Err(err) => {
                return note(
                    &reports,
                    tr!(key_error, reason = err.to_string()),
                    Tone::Fault,
                );
            }
        };
        note(
            &reports,
            tr!(key_checking, id = crate::transcript::clean(&id)),
            Tone::Quiet,
        );
        let ask = |reason: &str| {
            let reason = reason.to_owned();
            let (answer, kept) = oneshot::channel();
            let _ = reports.send(Report::Key(Step::Ask { reason, answer }));
            async move { kept.await.unwrap_or(false) }
        };
        let checked = providers::proves(&providers, &id, &typed).await;
        let kept =
            providers::change_key(store.as_ref(), &account, &typed, async { checked }, ask).await;
        match kept {
            Ok(Kept::New) => {
                let _ = reports.send(Report::Key(Step::Kept { id, kept: true }));
            }
            Ok(Kept::Old) => {
                let _ = reports.send(Report::Key(Step::Kept { id, kept: false }));
            }
            Err(err) => note(
                &reports,
                tr!(key_error, reason = err.to_string()),
                Tone::Fault,
            ),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parts() -> Arc<Parts> {
        Arc::new(crate::session::for_tests().0)
    }

    /// What the next report is, when it is a step of the key's change.
    fn step(reports: &mut mpsc::UnboundedReceiver<Report>) -> Step {
        match reports.try_recv() {
            Ok(Report::Key(step)) => step,
            _ => panic!("a step of the key's change was expected"),
        }
    }

    #[test]
    fn a_key_with_spaces_nothing_or_too_long_is_refused_before_anything_is_checked() {
        let (reports, mut got) = mpsc::unbounded_channel();
        let long = "k".repeat(providers::MAX_KEY_BYTES as usize + 1);
        for typed in ["two words", "  ", long.as_str()] {
            change(
                parts(),
                "openrouter".into(),
                "openrouter@openrouter.ai".into(),
                typed.into(),
                reports.clone(),
            );
        }
        for wanted in ["spaces or control", "nothing kept", "bytes"] {
            match step(&mut got) {
                Step::Note(text, _) => assert!(text.contains(wanted), "{text}"),
                _ => panic!("a note was expected"),
            }
        }
        assert!(got.try_recv().is_err(), "nothing else was started");
    }

    /// T70 (Q1) end to end, with no server: the provider cannot be reached, so the new key does not check out — the
    /// owner's word decides, the old key stays until it is "keep it anyway", and no message holds a key.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_key_that_does_not_check_out_is_asked_about_and_the_old_one_stays_unless_kept() {
        let parts = parts();
        let home = parts.home.clone();
        std::fs::create_dir_all(home.join(".glut")).unwrap();
        // The file store, so the test never reads the Keychain; a provider that nothing listens at.
        std::fs::write(
            home.join(".glut/providers.json"),
            r#"{"credentialStore":"file","providers":{"box":{"kind":"openai-compatible",
                "root":"http://localhost:9/v1","model":"m"}}}"#,
        )
        .unwrap();
        let list = Providers::load(&home).unwrap();
        let store = list.store(&home).unwrap();
        store.set("box@localhost:9", "sk-old").unwrap();
        let (reports, mut got) = mpsc::unbounded_channel();
        let mut said = Vec::new();
        for (word, kept) in [(false, "sk-old"), (true, "sk-new-key")] {
            change(
                Arc::clone(&parts),
                "box".into(),
                "box@localhost:9".into(),
                "sk-new-key".into(),
                reports.clone(),
            );
            let mut asked = false;
            loop {
                let next = tokio::time::timeout(std::time::Duration::from_secs(30), got.recv())
                    .await
                    .expect("the change comes to an end")
                    .expect("the channel stays open");
                let Report::Key(next) = next else {
                    panic!("only the key's steps are sent")
                };
                match next {
                    Step::Note(text, _) => said.push(text),
                    Step::Ask { reason, answer } => {
                        said.push(reason);
                        assert!(
                            store.get("box@localhost:9").unwrap().as_deref() == Some("sk-old")
                                || asked,
                            "the old key is there while the question is open"
                        );
                        asked = true;
                        answer.send(word).unwrap();
                    }
                    Step::Kept { id, kept } => {
                        assert_eq!((id.as_str(), kept), ("box", word));
                        break;
                    }
                }
            }
            assert!(asked, "the check failed, so the owner was asked");
            assert_eq!(store.get("box@localhost:9").unwrap().as_deref(), Some(kept));
        }
        assert!(
            said.iter().all(|text| !text.contains("sk-new-key")),
            "no message holds the key: {said:?}"
        );
        // Nothing else of the entry changed: the file is as the owner left it.
        assert!(
            std::fs::read_to_string(home.join(".glut/providers.json"))
                .unwrap()
                .contains(r#""model":"m""#)
        );
        let _ = std::fs::remove_dir_all(&home);
    }
}
