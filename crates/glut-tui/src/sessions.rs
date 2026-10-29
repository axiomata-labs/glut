//! Glut sessions talk across programs (`docs/plans/glut-tui.md`, T83): every Glut window on this machine listens on
//! a socket of its own under `~/.glut/sessions.d/`, which only the owner's own user may open, and answers two
//! questions — who are you (its name and its panes) and take this message. A sandboxed command never reaches it
//! (the sandbox denies every socket but the run's proxy): only Glut itself sends, for the owner's `/send` or a model's
//! `tell`.
//!
//! One request per connection, one JSON line each way. What the screen does with a message — at once, after the turn,
//! held for the owner's Enter — is the screen's to decide ([`crate::panes::Panes::on_incoming`]); this module only
//! carries it.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

/// The folder in `~/.glut` the windows' sockets lie in.
pub const DIR: &str = "sessions.d";
/// The longest line read from a socket: a message of [`crate::tell`]'s size, JSON-escaped, and its wrapping.
const MOST_LINE: u64 = 128 * 1024;
/// How long one exchange with another window may take: a window that does not answer is passed by. Longer than the
/// receiving side's own budget ([`READ`] and [`ANSWER`]), so a message the other window took is never reported lost.
const TALK: Duration = Duration::from_secs(5);
/// How long a window waits for a request's line, and for its own screen to answer it.
const READ: Duration = Duration::from_secs(2);
const ANSWER: Duration = Duration::from_secs(2);
/// The most requests a window serves at once: another program holding connections open cannot use up its files.
const MOST_SERVED: usize = 16;
/// The most panes a window's card may list, and the longest text of one of their fields.
const MOST_SEATS: usize = 16;
const MOST_FIELD: usize = 80;
/// How often a window looks which others are there, for `tell`'s description and `/sessions`.
pub const LOOK_EVERY: Duration = Duration::from_secs(10);
/// The longest name a window may be given.
const MOST_NAME: usize = 40;

/// Who sent a message.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum By {
    /// The owner, with `/send` in the other window: it goes in at once.
    Owner,
    /// The other window's model, with `tell`.
    Model,
    /// The other window telling that a turn asked about ended (Q8).
    Notice,
}

/// A message to a pane of another window.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Letter {
    /// The sender's address (`axiomata-glut-cf`, `axiomata-glut-cf/2`).
    pub from: String,
    /// The pane it goes to in the receiving window: `""` its main pane, a number, an agent's name or a role.
    pub to: String,
    pub text: String,
    pub by: By,
    /// The sending pane's mode (`default`, `auto`, …): a message from another mode waits for the owner (Q6).
    pub mode: String,
    /// Tell the sender when the turn this message starts ends (Q8).
    #[serde(default)]
    pub notify: bool,
    /// Messages between models without the owner's word so far, on the sender's side (Q7).
    #[serde(default)]
    pub chain: u32,
    /// What the sending pane may do and where it works (T83, Q6b): a model's message starts a turn only in a pane that
    /// can do no more than that — as `tell` between the panes of one window. None: it waits for the owner.
    #[serde(default)]
    pub rights: Option<Rights>,
}

/// What a sending pane may do, and its folder.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Rights {
    pub sender: crate::app::Sender,
    pub folder: PathBuf,
}

/// One pane or background agent of a window, as `/sessions` and `tell` list it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Seat {
    /// How it is reached: the window's name, then `/` and the pane's number or the agent's name.
    pub address: String,
    pub project: String,
    pub model: String,
    pub mode: String,
    pub role: Option<String>,
    pub works: bool,
}

/// A window as it answers "who are you".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Card {
    pub name: String,
    /// It takes no messages from outside (`/sessions off`).
    pub off: bool,
    pub seats: Vec<Seat>,
}

/// What one window asks another.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "lowercase")]
pub enum Ask {
    Who,
    Send(Letter),
}

/// What a window answers.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Reply {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub card: Option<Card>,
    /// What became of a message: how it was delivered, or why not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sent: Option<Result<String, String>>,
}

/// A request another window made, for the screen to answer.
pub struct Incoming {
    pub ask: Ask,
    pub reply: oneshot::Sender<Reply>,
}

/// This window's place among the others: its name, its socket, the others it saw last.
pub struct Link {
    dir: PathBuf,
    socket: PathBuf,
    name: Mutex<String>,
    seen: Mutex<Vec<(Card, PathBuf)>>,
}

impl Link {
    /// Listens on a socket of this window's own under `home`'s `~/.glut/sessions.d/`, named after the project folder
    /// `cwd`, and hands what others ask to the returned receiver.
    ///
    /// # Errors
    ///
    /// Why the folder or the socket could not be made: the window then talks to no other.
    pub async fn start(
        home: &Path,
        cwd: &Path,
    ) -> Result<(Arc<Link>, mpsc::Receiver<Incoming>), String> {
        let dir = home.join(glut::providers::DIR).join(DIR);
        glut::providers::make_private_dir(&dir).map_err(|err| err.to_string())?;
        private(&dir)?;
        let socket = dir.join(format!("{}.sock", std::process::id()));
        // A window of this process number that ended without cleaning up left its socket.
        let _ = std::fs::remove_file(&socket);
        let listener = UnixListener::bind(&socket).map_err(|err| err.to_string())?;
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600))
                .map_err(|err| err.to_string())?;
        }
        let link = Arc::new(Link {
            dir,
            socket,
            name: Mutex::new(String::new()),
            seen: Mutex::new(Vec::new()),
        });
        // Named at once, the others not asked yet — a window that does not answer must not hold this one up; the
        // first look settles a name another window has already ([`Link::look`]).
        *link.name.lock().unwrap_or_else(PoisonError::into_inner) =
            fresh_name(&base_name(cwd), &[]);
        let (incoming, inlet) = mpsc::channel(16);
        tokio::spawn(listen(listener, incoming));
        Ok((link, inlet))
    }

    /// This window's name.
    pub fn name(&self) -> String {
        self.name
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }

    /// Renames this window, as `/name` asks (Q3).
    ///
    /// # Errors
    ///
    /// Why not: a name of other characters than letters, digits, `-`, `_` and `.`, too long, or another window's.
    pub fn rename(&self, name: &str) -> Result<(), String> {
        check_name(name)?;
        if self.seen().iter().any(|card| card.name == name) {
            return Err(format!("another Glut is called {name}"));
        }
        *self.name.lock().unwrap_or_else(PoisonError::into_inner) = name.to_owned();
        Ok(())
    }

    /// The other windows as last seen.
    pub fn seen(&self) -> Vec<Card> {
        self.seen
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .map(|(card, _)| card.clone())
            .collect()
    }

    /// Asks every other window who it is, keeps what they said and returns it; a socket nobody listens on any more is
    /// cleared away.
    pub async fn look(&self) -> Vec<Card> {
        let mut found = Vec::new();
        let entries: Vec<PathBuf> = std::fs::read_dir(&self.dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|entry| entry.path())
            .filter(|path| {
                path.extension().is_some_and(|ext| ext == "sock") && *path != self.socket
            })
            .collect();
        // Every window asked at once: one that does not answer holds up none of the others.
        let mut asking = tokio::task::JoinSet::new();
        for path in entries {
            asking.spawn(async move { (talk(&path, &Ask::Who).await, path) });
        }
        while let Some(Ok((answer, path))) = asking.join_next().await {
            match answer {
                Ok(Reply {
                    card: Some(card), ..
                }) => {
                    if let Some(card) = checked(card) {
                        found.push((card, path));
                    }
                }
                Ok(_) => {}
                // A window that does not take the connection may be stopped (⌃Z) or busy: its socket goes only once
                // its process is gone.
                Err(_) if ended(&path) => {
                    let _ = std::fs::remove_file(&path);
                }
                Err(_) => {}
            }
        }
        // Two windows of one folder may have drawn the same name: the younger one takes another.
        let mine = self.name();
        if found
            .iter()
            .any(|(card, path)| card.name == mine && older(path, &self.socket))
        {
            let taken: Vec<String> = found.iter().map(|(card, _)| card.name.clone()).collect();
            let base = mine
                .rsplit_once('-')
                .map_or(mine.as_str(), |(base, _)| base);
            *self.name.lock().unwrap_or_else(PoisonError::into_inner) = fresh_name(base, &taken);
        }
        let cards = found.iter().map(|(card, _)| card.clone()).collect();
        *self.seen.lock().unwrap_or_else(PoisonError::into_inner) = found;
        cards
    }

    /// Whether `word` names another window, or a pane of one (`name/2`), as last seen.
    pub fn names_other(&self, word: &str) -> bool {
        let window = word.split_once('/').map_or(word, |(window, _)| window);
        self.seen().iter().any(|card| card.name == window)
    }

    /// Sends `letter` to `address` (`name` or `name/pane`), looking again for the windows where it is not known.
    ///
    /// # Errors
    ///
    /// Why it did not go: no window of that name, one that takes no messages, a pane it does not have, a window that
    /// did not answer.
    pub async fn send(&self, address: &str, mut letter: Letter) -> Result<String, String> {
        let (window, pane) = address.split_once('/').unwrap_or((address, ""));
        letter.to = pane.to_owned();
        let find = || {
            self.seen
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .iter()
                .find(|(card, _)| card.name == window)
                .map(|(_, path)| path.clone())
        };
        let path = match find() {
            Some(path) => path,
            None => {
                self.look().await;
                find().ok_or_else(|| format!("there is no Glut session {window}"))?
            }
        };
        match talk(&path, &Ask::Send(letter)).await {
            Ok(Reply {
                sent: Some(sent), ..
            }) => sent,
            Ok(_) => Err(format!("{window} did not say what became of it")),
            Err(Talk::Gone) => Err(format!("{window} has ended")),
            Err(Talk::Failed(reason)) => Err(format!("{window} did not answer: {reason}")),
        }
    }
}

impl Drop for Link {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.socket);
    }
}

/// Whether the folder the sockets lie in is this user's own, closed to everyone else, and no link.
fn private(dir: &Path) -> Result<(), String> {
    use std::os::unix::fs::MetadataExt;
    let meta = std::fs::symlink_metadata(dir).map_err(|err| err.to_string())?;
    let own = meta.uid() == rustix::process::getuid().as_raw();
    if meta.is_dir() && own && meta.mode() & 0o077 == 0 {
        Ok(())
    } else {
        Err(format!("{} is not a folder of yours alone", dir.display()))
    }
}

/// The process number a socket's file is named by.
fn pid_of(path: &Path) -> Option<i32> {
    path.file_stem()?.to_str()?.parse().ok()
}

/// Whether the window of the socket at `path` has ended: its process is gone. A name that is no process number is
/// never taken for an ended one.
fn ended(path: &Path) -> bool {
    pid_of(path)
        .and_then(rustix::process::Pid::from_raw)
        .is_some_and(|pid| rustix::process::test_kill_process(pid) == Err(rustix::io::Errno::SRCH))
}

/// Whether the window at `path` started before the one at `mine`, by their process numbers.
fn older(path: &Path, mine: &Path) -> bool {
    matches!((pid_of(path), pid_of(mine)), (Some(theirs), Some(ours)) if theirs < ours)
}

/// A card another window gave, its words held to what a window says: its name and every pane's address in the
/// names' characters, a few panes, short fields — anything else and the card is passed by.
fn checked(mut card: Card) -> Option<Card> {
    check_name(&card.name).ok()?;
    if card.seats.len() > MOST_SEATS {
        return None;
    }
    let short = |text: &str| text.chars().count() <= MOST_FIELD;
    for seat in &mut card.seats {
        check_address(&seat.address).ok()?;
        let window = seat
            .address
            .split_once('/')
            .map_or(seat.address.as_str(), |(window, _)| window);
        if window != card.name
            || !short(&seat.model)
            || !short(&seat.mode)
            || seat.role.as_deref().is_some_and(|role| !short(role))
        {
            return None;
        }
        // Only the folder's name travels into a model's description, never the whole path.
        seat.project = Path::new(&seat.project)
            .file_name()
            .map(|name| name.to_string_lossy().chars().take(MOST_FIELD).collect())
            .unwrap_or_default();
    }
    Some(card)
}

/// Whether `address` is a window's name, or one and a pane's number or name behind a `/`.
pub fn check_address(address: &str) -> Result<(), String> {
    let (window, pane) = address.split_once('/').unwrap_or((address, ""));
    check_name(window)?;
    if address.ends_with('/') {
        return Err("an address ending in /".into());
    }
    if !pane.is_empty() {
        check_name(pane)?;
    }
    Ok(())
}

/// Why an exchange with another window failed.
enum Talk {
    /// Nobody listens there: the window ended.
    Gone,
    Failed(String),
}

/// One exchange with the window listening on `path`.
async fn talk(path: &Path, ask: &Ask) -> Result<Reply, Talk> {
    let exchange = async {
        let mut stream = match UnixStream::connect(path).await {
            Ok(stream) => stream,
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::ConnectionRefused | std::io::ErrorKind::NotFound
                ) =>
            {
                return Err(Talk::Gone);
            }
            Err(err) => return Err(Talk::Failed(err.to_string())),
        };
        same_owner(&stream).map_err(Talk::Failed)?;
        let mut line = serde_json::to_string(ask).map_err(|err| Talk::Failed(err.to_string()))?;
        line.push('\n');
        stream
            .write_all(line.as_bytes())
            .await
            .map_err(|err| Talk::Failed(err.to_string()))?;
        let answer = read_line(&mut stream).await.map_err(Talk::Failed)?;
        serde_json::from_str(&answer).map_err(|err| Talk::Failed(err.to_string()))
    };
    tokio::time::timeout(TALK, exchange)
        .await
        .unwrap_or_else(|_| Err(Talk::Failed("it took too long".into())))
}

/// Whether the other end of `stream` runs as this process's own user: nobody else's program may talk to a window.
fn same_owner(stream: &UnixStream) -> Result<(), String> {
    let theirs = stream.peer_cred().map_err(|err| err.to_string())?.uid();
    if theirs == rustix::process::getuid().as_raw() {
        Ok(())
    } else {
        Err("another user's program".into())
    }
}

/// One line from `stream`, at most [`MOST_LINE`] bytes.
async fn read_line(stream: &mut UnixStream) -> Result<String, String> {
    let mut line = String::new();
    BufReader::new(stream.take(MOST_LINE + 1))
        .read_line(&mut line)
        .await
        .map_err(|err| err.to_string())?;
    if line.len() as u64 > MOST_LINE {
        return Err("a line too long".into());
    }
    Ok(line)
}

/// Takes the other windows' requests, one per connection, from this user's programs alone.
async fn listen(listener: UnixListener, incoming: mpsc::Sender<Incoming>) {
    let served = Arc::new(tokio::sync::Semaphore::new(MOST_SERVED));
    loop {
        let stream = match listener.accept().await {
            Ok((stream, _)) => stream,
            // Out of files for a moment, a connection given up before it was taken: listen on.
            Err(_) => {
                tokio::time::sleep(Duration::from_millis(100)).await;
                continue;
            }
        };
        // Past the bound a request is dropped with its connection.
        let Ok(slot) = Arc::clone(&served).try_acquire_owned() else {
            continue;
        };
        let incoming = incoming.clone();
        tokio::spawn(async move {
            let _ = tokio::time::timeout(READ + ANSWER, serve(stream, incoming)).await;
            drop(slot);
        });
    }
}

async fn serve(mut stream: UnixStream, incoming: mpsc::Sender<Incoming>) -> Result<(), String> {
    same_owner(&stream)?;
    let line = tokio::time::timeout(READ, read_line(&mut stream))
        .await
        .map_err(|_| "no request in time".to_owned())??;
    let ask: Ask = serde_json::from_str(&line).map_err(|err| err.to_string())?;
    let (reply, answer) = oneshot::channel();
    incoming
        .send(Incoming { ask, reply })
        .await
        .map_err(|_| "the window is closing".to_owned())?;
    let reply = tokio::time::timeout(ANSWER, answer)
        .await
        .map_err(|_| "the window did not answer".to_owned())?
        .map_err(|_| "the window did not answer".to_owned())?;
    let mut line = serde_json::to_string(&reply).map_err(|err| err.to_string())?;
    line.push('\n');
    stream
        .write_all(line.as_bytes())
        .await
        .map_err(|err| err.to_string())
}

/// Whether `name` may name a window.
fn check_name(name: &str) -> Result<(), String> {
    let fits = !name.is_empty()
        && name.chars().count() <= MOST_NAME
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
        && !name.starts_with(['-', '.']);
    if fits {
        Ok(())
    } else {
        Err(format!(
            "a name of letters, digits, - _ and . — at most {MOST_NAME}"
        ))
    }
}

/// A window's name from its project folder: its last part, lower case, every other character a `-`.
fn base_name(cwd: &Path) -> String {
    let folder = cwd
        .file_name()
        .map(|name| name.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let base: String = folder
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
        .collect();
    let base = base.trim_matches('-');
    let base: String = base.chars().take(MOST_NAME - 5).collect();
    if base.is_empty() {
        "glut".to_owned()
    } else {
        base
    }
}

/// `base` and two letters, then a number where another window has that name already (Q13).
fn fresh_name(base: &str, taken: &[String]) -> String {
    use std::hash::{BuildHasher, Hasher};
    let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    let random = hasher.finish();
    let letter = |at: u64| char::from(b'a' + u8::try_from(at % 26).unwrap_or(0));
    let name = format!("{base}-{}{}", letter(random), letter(random / 26));
    let mut fresh = name.clone();
    let mut number = 2;
    while taken.contains(&fresh) {
        fresh = format!("{name}-{number}");
        number += 1;
    }
    fresh
}

/// Another window for a test, named `name`, beside `link`: it answers who it is and hands on every message it takes.
#[cfg(test)]
pub(crate) fn fake_window(link: &Link, name: &str) -> mpsc::UnboundedReceiver<Letter> {
    let listener = UnixListener::bind(link.dir.join(format!("{name}.sock"))).unwrap();
    let (incoming, mut asked) = mpsc::channel(4);
    let (letters, taken) = mpsc::unbounded_channel();
    let name = name.to_owned();
    tokio::spawn(listen(listener, incoming));
    tokio::spawn(async move {
        while let Some(Incoming { ask, reply }) = asked.recv().await {
            let answer = match ask {
                Ask::Who => Reply {
                    card: Some(Card {
                        name: name.clone(),
                        off: false,
                        seats: Vec::new(),
                    }),
                    sent: None,
                },
                Ask::Send(letter) => {
                    let _ = letters.send(letter);
                    Reply {
                        card: None,
                        sent: Some(Ok("delivered".into())),
                    }
                }
            };
            let _ = reply.send(answer);
        }
    });
    taken
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_window_is_named_after_its_folder_and_never_twice() {
        assert_eq!(base_name(Path::new("/x/Axiomata-Glut")), "axiomata-glut");
        assert_eq!(base_name(Path::new("/")), "glut");
        let name = fresh_name("glut", &[]);
        assert!(name.starts_with("glut-") && name.len() == 7, "{name}");
        let taken = vec![name.clone()];
        let again = fresh_name("glut", &taken);
        assert!(!taken.contains(&again));
        assert!(check_name("my-glut.2").is_ok());
        for bad in ["", "-x", "a/b", "a b", "ä", &"x".repeat(41)] {
            assert!(check_name(bad).is_err(), "{bad}");
        }
    }

    #[test]
    fn a_letter_goes_as_one_json_line_and_back() {
        let ask = Ask::Send(Letter {
            from: "a-xy".into(),
            to: "2".into(),
            text: "line one\nline two".into(),
            by: By::Model,
            mode: "default".into(),
            notify: true,
            chain: 3,
            rights: None,
        });
        let line = serde_json::to_string(&ask).unwrap();
        assert!(!line.contains('\n'));
        assert_eq!(serde_json::from_str::<Ask>(&line).unwrap(), ask);
        let who: Ask = serde_json::from_str(r#"{"op":"who"}"#).unwrap();
        assert_eq!(who, Ask::Who);
    }

    #[tokio::test]
    async fn two_windows_find_each_other_and_a_message_comes_through() {
        let home = std::env::temp_dir().join(format!("glut-sessions-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        // Two links in one process share its number: the second takes another socket name by hand.
        let (first, mut inlet) = Link::start(&home, Path::new("/x/alpha")).await.unwrap();
        let second_socket = first.dir.join("other.sock");
        let listener = UnixListener::bind(&second_socket).unwrap();
        let (incoming, mut second_inlet) = mpsc::channel(4);
        tokio::spawn(listen(listener, incoming));
        tokio::spawn(async move {
            while let Some(Incoming { ask, reply }) = second_inlet.recv().await {
                let answer = match ask {
                    Ask::Who => Reply {
                        card: Some(Card {
                            name: "beta-xy".into(),
                            off: false,
                            seats: Vec::new(),
                        }),
                        sent: None,
                    },
                    Ask::Send(letter) => Reply {
                        card: None,
                        sent: Some(Ok(format!(
                            "{} to {:?}: {}",
                            letter.from, letter.to, letter.text
                        ))),
                    },
                };
                let _ = reply.send(answer);
            }
        });
        let seen = first.look().await;
        assert_eq!(seen.len(), 1);
        assert!(first.names_other("beta-xy/2"));
        let letter = Letter {
            from: first.name(),
            to: String::new(),
            text: "hello".into(),
            by: By::Model,
            mode: "default".into(),
            notify: false,
            chain: 0,
            rights: None,
        };
        let sent = first.send("beta-xy/2", letter).await.unwrap();
        assert!(sent.ends_with(r#"to "2": hello"#), "{sent}");
        assert!(first.send("nobody", letter_to_nobody()).await.is_err());
        assert!(inlet.try_recv().is_err(), "nobody asked the first one");
        let _ = std::fs::remove_dir_all(&home);
    }

    fn letter_to_nobody() -> Letter {
        Letter {
            from: "a".into(),
            to: String::new(),
            text: "x".into(),
            by: By::Owner,
            mode: "default".into(),
            notify: false,
            chain: 0,
            rights: None,
        }
    }
}
