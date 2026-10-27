//! `/login` (`docs/plans/glut-providers.md`, PV4): the subscriptions the owner may sign in to — today the ChatGPT
//! plan — with how each stands; Enter signs in, or out. The sign-in goes as an MCP server's does (T56): the address
//! shown first, the browser opened only on the owner's ⏎, over SSH the address the browser ended on pasted back
//! (`/login chatgpt <address>`).

use std::path::Path;
use std::sync::Arc;

use glut::subscription::chatgpt::{self, Server, SignedOut};
use tokio::sync::{mpsc, oneshot};

use crate::mcp::Step;
use crate::session::{Parts, Report};
use crate::transcript::Tone;

/// The key a sign-in waits under for an address pasted back: apart from the MCP servers' names.
pub fn waiting_key(id: &str) -> String {
    format!("provider:{id}")
}

/// One subscription as `/login` lists it: its provider id, and who is signed in, if anyone.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// The provider's id.
    pub id: String,
    /// The account signed in (its email, else its id); `None` while nobody is.
    pub who: Option<String>,
}

/// Every subscription of `home`, with who is signed in.
pub fn standings(home: &Path) -> Vec<Standing> {
    glut::providers::subscriptions(home)
        .into_iter()
        .map(|(id, account)| {
            let who = glut::subscription::load(home, &account)
                .ok()
                .flatten()
                .filter(glut::subscription::Record::signed_in)
                .map(|record| record.email.unwrap_or(record.subject));
            Standing { id, who }
        })
        .collect()
}

fn note(reports: &mpsc::UnboundedSender<Report>, text: String, tone: Tone) {
    let _ = reports.send(Report::Mcp(Step::Note(text, tone)));
}

/// Signs in to subscription `id` beside the screen: its account made where it was none, the address shown, the
/// browser opened on the owner's word, the tokens kept once it came back.
pub fn sign_in(parts: &Arc<Parts>, id: String, reports: mpsc::UnboundedSender<Report>) {
    let home = parts.home.clone();
    tokio::spawn(async move {
        let failed = |reason: String| tr!(login_failed, id = id, reason = reason);
        let account = match glut::providers::signing(&home, &id) {
            Ok(account) => account,
            Err(reason) => return note(&reports, failed(reason), Tone::Fault),
        };
        let kept = glut::subscription::load(&home, &account).ok().flatten();
        let first = kept
            .as_ref()
            .is_none_or(|record| record.client_id.is_empty());
        let sign_in = match chatgpt::begin(&home, kept, Server::openai()).await {
            Ok(sign_in) => sign_in,
            Err(reason) => return note(&reports, failed(reason), Tone::Fault),
        };
        note(
            &reports,
            tr!(
                login_sign_in_at,
                host = sign_in.host(),
                address = sign_in.address()
            ),
            Tone::Heed,
        );
        if glut::oauth::can_open() {
            let _ = reports.send(Report::Mcp(Step::Open {
                address: sign_in.address().to_owned(),
            }));
        } else {
            note(&reports, tr!(login_paste_back, id = id), Tone::Quiet);
        }
        let waiting = waiting_key(&id);
        let number = crate::mcp::next_sign_in();
        let (paste, pasted) = oneshot::channel();
        let _ = reports.send(Report::Mcp(Step::Waiting {
            name: waiting.clone(),
            id: number,
            paste,
        }));
        let finished = sign_in.finish(async { pasted.await.ok() }).await;
        let _ = reports.send(Report::Mcp(Step::Ended {
            name: waiting,
            id: number,
        }));
        let record = match finished {
            Ok(record) => record,
            Err(failure) => return note(&reports, failed(failure.to_string()), Tone::Fault),
        };
        // Kept under the record's lock: a renewal of another window must not write its older tokens over these.
        let kept = match glut::subscription::Lock::take(&home, &account).await {
            Ok(_lock) => glut::subscription::save(&home, &account, &record),
            Err(err) => Err(err),
        };
        if let Err(err) = kept {
            return note(&reports, failed(err.to_string()), Tone::Fault);
        }
        let who = glut::permissions::printable(&record.email.unwrap_or(record.subject));
        note(
            &reports,
            tr!(login_signed_in, id = id, who = who),
            Tone::Fine,
        );
        // Once, at the first sign-in: what the plan's limit means here (OpenAI's welcome, in Glut's words).
        if first {
            note(
                &reports,
                tr!(login_welcome, page = chatgpt::USAGE_PAGE),
                Tone::Heed,
            );
        }
    });
}

/// Signs out of subscription `id` beside the screen: revoked at its provider, its tokens gone here.
pub fn sign_out(parts: &Arc<Parts>, id: String, reports: mpsc::UnboundedSender<Report>) {
    let home = parts.home.clone();
    tokio::spawn(async move {
        let account = match glut::providers::signing(&home, &id) {
            Ok(account) => account,
            Err(reason) => {
                return note(
                    &reports,
                    tr!(login_failed, id = id, reason = reason),
                    Tone::Fault,
                );
            }
        };
        // Under the record's lock: a renewal racing the sign-out would keep the account signed in.
        let _lock = match glut::subscription::Lock::take(&home, &account).await {
            Ok(lock) => lock,
            Err(err) => {
                return note(
                    &reports,
                    tr!(login_failed, id = id, reason = err),
                    Tone::Fault,
                );
            }
        };
        let Some(record) = glut::subscription::load(&home, &account).ok().flatten() else {
            return note(&reports, tr!(login_out_nothing, id = id), Tone::Quiet);
        };
        let (kept, done) = chatgpt::sign_out(&Server::openai(), &record).await;
        if let Err(err) = glut::subscription::save(&home, &account, &kept) {
            return note(
                &reports,
                tr!(login_failed, id = id, reason = err),
                Tone::Fault,
            );
        }
        let (text, tone) = match done {
            SignedOut::Nothing => (tr!(login_out_nothing, id = id), Tone::Quiet),
            SignedOut::Revoked => (tr!(login_out_revoked, id = id), Tone::Fine),
            SignedOut::NotConfirmed(reason) => (
                tr!(
                    login_out_unconfirmed,
                    id = id,
                    reason = glut::permissions::printable(&reason)
                ),
                Tone::Heed,
            ),
        };
        note(&reports, text, tone);
    });
}
