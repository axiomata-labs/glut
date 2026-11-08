//! Requests between agents and their answers (#142, agents that work together): a model's `tell` with `answer`
//! asks another agent for something; the turn it starts there answers it — its last reply goes back to the asker as
//! a message marked as the answer to that request. Glut keeps the books, not the models: which turn owes which
//! answer, where it goes, and what the asker hears when the other agent never gets to it (the owner set it aside, its
//! turn was refused or the one before it did not finish, the pane closed, the window ended, an hour passed).
//!
//! The screen ([`crate::panes::Panes`]) delivers; this module only remembers. Between windows a request travels by a
//! random number, never by the asker's count: no window answers a request it was not sent, whatever it guesses.

use std::time::{Duration, Instant};

use glut::FinishReason;
use tokio::sync::mpsc;

/// The most answers a window owes at once, and the most requests its panes wait on in other windows: past them a
/// request is refused, not forgotten.
const MOST_OPEN: usize = 64;
/// How long a request stays open: past it the asker hears that no answer came, and a late one finds nobody.
const LIFE: Duration = Duration::from_secs(60 * 60);

/// Why a request will never be answered by a turn (#142).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Unanswered {
    /// The owner there dropped it (Esc), or the conversation it waited in went (`/clear`, another session).
    SetAside,
    /// The turn it started was refused before it began (a hook).
    Refused,
    /// The turn before it ended without finishing: what waited behind it waits for the owner now.
    Unfinished,
}

impl Unanswered {
    /// What the asker reads.
    pub fn why(self) -> &'static str {
        match self {
            Unanswered::SetAside => "the owner there set the request aside",
            Unanswered::Refused => "the turn it started was refused",
            Unanswered::Unfinished => "its turn before yours ended unfinished",
        }
    }
}

/// Where an owed answer goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Back {
    /// A pane of this window, by its session id, and its request's number.
    Pane { id: u64, request: u32 },
    /// A pane of another window, by its address, and the request's random number on the way.
    Window { address: String, nonce: u64 },
}

/// An answer this window owes: the pane whose turn gives it, and where it goes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Owed {
    /// This window's number for the request, marked in the message the pane got.
    pub token: u32,
    /// The answering pane's session id, and how the asker named it.
    pub pane: u64,
    pub label: String,
    pub back: Back,
    pub at: Instant,
}

/// A request a pane of this window sent to another window, whose answer it waits for.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Asked {
    /// The asker's number for it, and the random one it travels by.
    request: u32,
    nonce: u64,
    pane: u64,
    /// The address it went to, and the window's name in it.
    address: String,
    at: Instant,
}

impl Asked {
    fn window(&self) -> &str {
        self.address
            .split_once('/')
            .map_or(self.address.as_str(), |(window, _)| window)
    }
}

/// A request to another window that will not be answered: the asking pane, its number, the address.
pub type Unheard = (u64, u32, String);

/// The open requests of one window, both ways.
pub struct Requests {
    next: u32,
    owed: Vec<Owed>,
    asked: Vec<Asked>,
    /// Requests to another window that did not get there, by their random number: they wait for nothing.
    failed: mpsc::UnboundedSender<u64>,
    failures: mpsc::UnboundedReceiver<u64>,
}

impl Default for Requests {
    fn default() -> Self {
        let (failed, failures) = mpsc::unbounded_channel();
        Requests {
            next: 0,
            owed: Vec::new(),
            asked: Vec::new(),
            failed,
            failures,
        }
    }
}

impl Requests {
    /// A fresh number: a request's, as its asker knows it, and a token, as the answering turn is marked.
    pub fn number(&mut self) -> u32 {
        self.next = self.next.wrapping_add(1);
        self.next
    }

    /// Whether another request may be opened.
    pub fn room(&self) -> Result<(), String> {
        if self.owed.len() + self.asked.len() >= MOST_OPEN {
            Err(format!(
                "{MOST_OPEN} requests are open in this window already: wait for their answers"
            ))
        } else {
            Ok(())
        }
    }

    /// Owes `back` the answer of the turn of pane `pane` that takes the message marked `token`.
    pub fn owe(&mut self, token: u32, pane: u64, label: String, back: Back) {
        self.owed.push(Owed {
            token,
            pane,
            label,
            back,
            at: Instant::now(),
        });
    }

    /// The answer owed for `token` by pane `pane`, taken off the books — no other pane's turn gives it.
    pub fn take(&mut self, token: u32, pane: u64) -> Option<Owed> {
        let at = self
            .owed
            .iter()
            .position(|owed| owed.token == token && owed.pane == pane)?;
        Some(self.owed.remove(at))
    }

    /// The answers whose pane is not among `open` any more, taken off the books: their askers hear it closed.
    pub fn closed(&mut self, open: &[u64]) -> Vec<Owed> {
        let (gone, kept) = std::mem::take(&mut self.owed)
            .into_iter()
            .partition(|owed| !open.contains(&owed.pane));
        self.owed = kept;
        gone
    }

    /// Drops what was owed to, or asked by, a pane of this window that closed: nobody is there to read it.
    pub fn forget_asker(&mut self, open: &[u64]) {
        self.owed
            .retain(|owed| !matches!(owed.back, Back::Pane { id, .. } if !open.contains(&id)));
        self.asked.retain(|asked| open.contains(&asked.pane));
    }

    /// Pane `pane` asked `address` in another window, as its request `request`: the random number it travels by.
    pub fn asked(&mut self, request: u32, pane: u64, address: String) -> u64 {
        let nonce = nonce();
        self.asked.push(Asked {
            request,
            nonce,
            pane,
            address,
            at: Instant::now(),
        });
        nonce
    }

    /// The asking pane and its number for the request whose random number is `nonce`, taken off the books: only a
    /// request a pane here sent and still waits on.
    pub fn answered(&mut self, nonce: u64) -> Option<(u64, u32)> {
        let at = self.asked.iter().position(|asked| asked.nonce == nonce)?;
        let asked = self.asked.remove(at);
        Some((asked.pane, asked.request))
    }

    /// Whom a task sending a request to another window tells that it did not get there.
    pub fn failed(&self) -> mpsc::UnboundedSender<u64> {
        self.failed.clone()
    }

    /// Takes the requests that did not get to their window off the books.
    pub fn settle_failures(&mut self) {
        while let Ok(nonce) = self.failures.try_recv() {
            self.asked.retain(|asked| asked.nonce != nonce);
        }
    }

    /// The requests waiting on a window among `ended` (whose process is gone), taken off the books.
    pub fn left(&mut self, ended: &[String]) -> Vec<Unheard> {
        if ended.is_empty() {
            return Vec::new();
        }
        let (gone, kept): (Vec<Asked>, Vec<Asked>) = std::mem::take(&mut self.asked)
            .into_iter()
            .partition(|asked| ended.iter().any(|window| window == asked.window()));
        self.asked = kept;
        gone.into_iter()
            .map(|asked| (asked.pane, asked.request, asked.address))
            .collect()
    }

    /// What stayed open past [`LIFE`] by `now`, taken off the books: answers owed, and requests to other windows.
    pub fn expired(&mut self, now: Instant) -> (Vec<Owed>, Vec<Unheard>) {
        let (old, kept): (Vec<Owed>, Vec<Owed>) = std::mem::take(&mut self.owed)
            .into_iter()
            .partition(|owed| now.saturating_duration_since(owed.at) >= LIFE);
        self.owed = kept;
        let (stale, kept): (Vec<Asked>, Vec<Asked>) = std::mem::take(&mut self.asked)
            .into_iter()
            .partition(|asked| now.saturating_duration_since(asked.at) >= LIFE);
        self.asked = kept;
        let stale = stale
            .into_iter()
            .map(|asked| (asked.pane, asked.request, asked.address))
            .collect();
        (old, stale)
    }
}

/// A random number no other window can guess: the process's hash keys (random at its start) over the time and a
/// count.
fn nonce() -> u64 {
    use std::hash::{BuildHasher, Hasher};
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNT: AtomicU64 = AtomicU64::new(0);
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u64(COUNT.fetch_add(1, Ordering::Relaxed));
    hasher.write_u128(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |since| since.as_nanos()),
    );
    hasher.finish()
}

/// The answer a turn gives to the request that started it: its last reply when it answered, else what stopped it and
/// what it had said — at most `most` bytes, the rest left in `label`'s conversation.
pub fn answer_text(reason: FinishReason, reply: Option<&str>, label: &str, most: usize) -> String {
    let reply = reply.map(str::trim).filter(|reply| !reply.is_empty());
    let why = match reason {
        FinishReason::Answered => {
            let text = reply.map_or_else(
                || format!("{label} ended its turn without a word"),
                str::to_owned,
            );
            return bounded(text, label, most);
        }
        FinishReason::Cancelled => "it was stopped",
        FinishReason::Failed => "its model failed",
        FinishReason::Budget => "it spent its budget",
        FinishReason::MaxTurns => "it reached its step limit",
        FinishReason::Timeout => "it ran out of time",
        FinishReason::Refused => "too many of its calls were refused",
    };
    let text = match reply {
        Some(said) => format!("{label} did not finish ({why}); it had said: {said}"),
        None => format!("{label} did not finish ({why})"),
    };
    bounded(text, label, most)
}

/// `text` in at most `most` bytes, the rest said to stand in `label`'s conversation.
fn bounded(text: String, label: &str, most: usize) -> String {
    if text.len() <= most {
        return text;
    }
    let note = format!("\n[… the rest stands in {label}'s conversation]");
    let mut cut = most.saturating_sub(note.len());
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}{note}", &text[..cut])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_answer_is_owed_once_by_its_pane_and_a_closed_pane_owes_it_no_more() {
        let mut requests = Requests::default();
        let token = requests.number();
        requests.owe(token, 2, "pane 2".into(), Back::Pane { id: 1, request: 1 });
        let other = requests.number();
        requests.owe(
            other,
            3,
            "pane 3".into(),
            Back::Window {
                address: "w-ab".into(),
                nonce: 7,
            },
        );
        assert!(requests.take(token, 3).is_none(), "another pane's turn");
        assert_eq!(requests.take(token, 2).unwrap().pane, 2);
        assert!(requests.take(token, 2).is_none(), "once");
        let closed = requests.closed(&[1, 2]);
        assert_eq!(closed.len(), 1);
        assert_eq!(closed[0].token, other);
        assert!(requests.room().is_ok());
    }

    #[test]
    fn an_answer_from_a_window_counts_only_by_the_random_number_it_was_sent() {
        let mut requests = Requests::default();
        let nonce = requests.asked(4, 1, "docs-xy/2".into());
        assert_ne!(nonce, requests.asked(5, 1, "docs-xy".into()));
        assert_eq!(requests.answered(4), None, "the asker's count is no key");
        assert_eq!(requests.answered(nonce), Some((1, 4)));
        assert_eq!(requests.answered(nonce), None, "once");
        let failed = requests.asked(6, 1, "gone-ab/2".into());
        requests.failed().send(failed).unwrap();
        requests.settle_failures();
        requests.asked(7, 1, "gone-ab".into());
        assert!(requests.left(&[]).is_empty());
        let left = requests.left(&["gone-ab".into()]);
        assert_eq!(left, vec![(1, 7, "gone-ab".to_owned())]);
        assert_eq!(requests.asked.len(), 1, "docs-xy's stays");
        assert_eq!(requests.expired(Instant::now()).1, Vec::new());
        let (old, stale) = requests.expired(Instant::now() + LIFE);
        assert!(old.is_empty() && stale == vec![(1, 5, "docs-xy".to_owned())]);
    }

    #[test]
    fn the_answer_is_the_last_reply_or_why_there_is_none_and_holds_its_size() {
        assert_eq!(
            answer_text(FinishReason::Answered, Some(" done "), "pane 2", 100),
            "done"
        );
        assert!(
            answer_text(FinishReason::Cancelled, Some("half"), "pane 2", 100)
                .contains("did not finish (it was stopped); it had said: half")
        );
        let long = answer_text(FinishReason::Answered, Some(&"ä".repeat(100)), "pane 2", 60);
        assert!(
            long.len() <= 60 && long.ends_with("pane 2's conversation]"),
            "{long}"
        );
    }
}
