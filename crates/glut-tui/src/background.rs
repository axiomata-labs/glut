//! The terminal's own background colour (T72), asked once as the screen is taken (OSC 11): Glimm's glow is blended over
//! it, so it runs out into the terminal's colour instead of ending in a box. Where the terminal does not tell — no
//! answer, a colour in no form we read, no terminal at all — it is unknown and Glimm is drawn without a glow. A guess
//! (`COLORFGBG` names only a palette index, not its colour) is not made: a glow blended over a wrong colour shows its
//! edge as a visible rectangle, worse than no glow.

use std::io::{IsTerminal, Write};
use std::os::fd::AsFd;
use std::sync::OnceLock;
use std::time::{Duration, Instant};

/// A colour: red, green, blue.
pub type Rgb = (u8, u8, u8);

/// How long a terminal has to answer. The closing DA1 query ends the wait at once for one that does not know OSC 11
/// (it answers DA1 and nothing before it), so this is only for one that says nothing at all.
const ANSWER_WAIT: Duration = Duration::from_millis(250);

static KNOWN: OnceLock<Option<Rgb>> = OnceLock::new();

/// The terminal's background colour, when `probe` found it out.
pub fn known() -> Option<Rgb> {
    KNOWN.get().copied().flatten()
}

/// Asks the terminal for its background colour, once (a later call changes nothing). Call it with the terminal in raw
/// mode and before anything else reads the keys: the answer comes in on the same input.
pub fn probe() {
    KNOWN.get_or_init(|| {
        if cfg!(test) || !std::io::stdin().is_terminal() || !std::io::stdout().is_terminal() {
            return None;
        }
        ask().ok().flatten()
    });
}

/// Sends the OSC 11 query and DA1, and reads until the DA1 answer or the time is up.
fn ask() -> std::io::Result<Option<Rgb>> {
    let mut out = std::io::stdout();
    write!(out, "\x1b]11;?\x07\x1b[c")?;
    out.flush()?;
    let stdin = std::io::stdin();
    let fd = stdin.as_fd();
    let until = Instant::now() + ANSWER_WAIT;
    let mut got: Vec<u8> = Vec::new();
    let mut buf = [0u8; 512];
    while Instant::now() < until {
        let left = until.saturating_duration_since(Instant::now());
        let timeout = rustix::event::Timespec {
            tv_sec: left.as_secs() as i64,
            tv_nsec: i64::from(left.subsec_nanos()),
        };
        let mut fds = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        if rustix::event::poll(&mut fds, Some(&timeout))? == 0 {
            break;
        }
        let read = rustix::io::read(fd, &mut buf)?;
        if read == 0 {
            break;
        }
        got.extend_from_slice(&buf[..read]);
        if let Some(color) = parse_reply(&got) {
            // The answer is in; the DA1 answer behind it may still be on its way — take what is there so it does not
            // reach the keys as typing.
            drain(fd);
            return Ok(Some(color));
        }
        if ends_with_da1(&got) {
            return Ok(None);
        }
    }
    Ok(None)
}

/// Reads and drops what is already waiting on the input, for a few milliseconds more.
fn drain(fd: std::os::fd::BorrowedFd<'_>) {
    let until = Instant::now() + Duration::from_millis(40);
    let mut buf = [0u8; 512];
    while Instant::now() < until {
        let mut fds = [rustix::event::PollFd::new(
            &fd,
            rustix::event::PollFlags::IN,
        )];
        let timeout = rustix::event::Timespec {
            tv_sec: 0,
            tv_nsec: 10_000_000,
        };
        match rustix::event::poll(&mut fds, Some(&timeout)) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                if rustix::io::read(fd, &mut buf).unwrap_or(0) == 0 {
                    break;
                }
            }
        }
    }
}

/// Whether `bytes` hold a complete DA1 answer (`CSI ? … c`).
fn ends_with_da1(bytes: &[u8]) -> bool {
    let text = String::from_utf8_lossy(bytes);
    let mut from = 0;
    while let Some(found) = text[from..].find("\x1b[?") {
        let start = from + found + 3;
        let rest = &text[start..];
        match rest.find(|c: char| !(c.is_ascii_digit() || c == ';')) {
            Some(end) if rest[end..].starts_with('c') => return true,
            Some(_) => from = start,
            None => return false,
        }
    }
    false
}

/// The colour in an OSC 11 answer (`ESC ] 11 ; rgb:RRRR/GGGG/BBBB` ended by BEL or `ESC \`), each part of one to four
/// hex digits.
fn parse_reply(bytes: &[u8]) -> Option<Rgb> {
    let text = String::from_utf8_lossy(bytes);
    let at = text.find("\x1b]11;")?;
    let body = &text[at + 5..];
    let end = body.find(['\x07', '\x1b'])?;
    let value = body[..end].strip_prefix("rgb:")?;
    let mut parts = value.split('/');
    let part = |s: &str| -> Option<u8> {
        if s.is_empty() || s.len() > 4 || !s.chars().all(|c| c.is_ascii_hexdigit()) {
            return None;
        }
        let max = (16u32.pow(s.len() as u32) - 1) as f32;
        let n = u32::from_str_radix(s, 16).ok()? as f32;
        Some((n / max * 255.0).round() as u8)
    };
    let color = (
        part(parts.next()?)?,
        part(parts.next()?)?,
        part(parts.next()?)?,
    );
    parts.next().is_none().then_some(color)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_reply_in_any_width_and_ending_is_read() {
        assert_eq!(
            parse_reply(b"\x1b]11;rgb:1e1e/1e1e/2e2e\x07"),
            Some((0x1e, 0x1e, 0x2e))
        );
        assert_eq!(
            parse_reply(b"\x1b]11;rgb:0505/0505/0505\x1b\\"),
            Some((5, 5, 5))
        );
        assert_eq!(
            parse_reply(b"\x1b]11;rgb:ff/00/80\x07"),
            Some((255, 0, 128))
        );
        assert_eq!(parse_reply(b"\x1b]11;rgb:f/0/8\x07"), Some((255, 0, 136)));
        assert_eq!(
            parse_reply(b"\x1b]11;rgb:ffff/ffff/ffff\x07\x1b[?62;c"),
            Some((255, 255, 255))
        );
    }

    #[test]
    fn what_is_not_a_colour_is_not_read() {
        assert_eq!(parse_reply(b""), None);
        assert_eq!(parse_reply(b"\x1b[?62;4c"), None);
        // Not complete yet: no terminator.
        assert_eq!(parse_reply(b"\x1b]11;rgb:1e1e/1e1e/2e"), None);
        // Another form, a part too long, too many parts, no hex.
        assert_eq!(parse_reply(b"\x1b]11;#1e1e2e\x07"), None);
        assert_eq!(parse_reply(b"\x1b]11;rgb:12345/00/00\x07"), None);
        assert_eq!(parse_reply(b"\x1b]11;rgb:00/00/00/00\x07"), None);
        assert_eq!(parse_reply(b"\x1b]11;rgb:zz/00/00\x07"), None);
    }

    #[test]
    fn the_da1_answer_ends_the_wait() {
        assert!(ends_with_da1(b"\x1b[?62;4;22c"));
        assert!(ends_with_da1(b"\x1b]11;rgb:00/00/00\x07\x1b[?1;2c"));
        assert!(!ends_with_da1(b"\x1b[?62;4"));
        assert!(!ends_with_da1(b""));
    }

    #[test]
    fn under_test_nothing_is_asked() {
        probe();
        assert_eq!(known(), None);
    }
}
