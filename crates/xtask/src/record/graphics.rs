//! The kitty graphics protocol as Ghostty takes it, for a scene that shows an image (`terminal ghostty`): Glut sends
//! the picture's PNG once (`a=t`), places it at the cursor in so many cells (`a=p`) and takes it away again (`a=d`).
//! The emulator knows no images, so each command is cut out of the output before it gets there and a [`TOKEN`] put in
//! its place; the take applies the command when the emulator reaches the token, with the cursor where the terminal
//! would have it. What stands on the screen goes into the film beside the text, for the player to lay the picture
//! over its cells.

use std::collections::{HashMap, VecDeque};

/// Put where a graphics command stood: an OSC the emulator ignores, found again as a question is.
pub(crate) const TOKEN: &str = "\x1b]7777;glut-picture\x07";

/// A command's start and end (ST).
const START: &[u8] = b"\x1b_G";
const END: &[u8] = b"\x1b\\";
/// The most bytes one unfinished command is held for the next read: Glut's pictures are a few hundred KiB of base64,
/// so one past this is no command of Glut's — it goes on to the emulator as it is.
const MOST_HELD: usize = 16 << 20;

/// A picture standing on the screen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Picture {
    /// Its image's place in the film.
    pub(crate) image: usize,
    /// Its first cell's column and row.
    pub(crate) x: u16,
    pub(crate) y: u16,
    /// How many columns and rows it takes.
    pub(crate) cols: u16,
    pub(crate) rows: u16,
}

/// One command: its keys and its payload (base64).
#[derive(Debug, Clone, PartialEq)]
struct Command {
    keys: HashMap<String, String>,
    payload: String,
}

impl Command {
    fn number(&self, key: &str) -> Option<u32> {
        self.keys.get(key).and_then(|value| value.parse().ok())
    }

    fn key(&self, key: &str) -> &str {
        self.keys.get(key).map_or("", String::as_str)
    }
}

/// What the terminal holds of the program's pictures.
#[derive(Debug, Default)]
pub(crate) struct Graphics {
    /// The start of a command the last read ended inside.
    held: Vec<u8>,
    /// The commands cut out, in order, each waiting for its token.
    waiting: VecDeque<Command>,
    /// The image being sent in chunks: its number and its base64 so far.
    open: Option<(u32, String)>,
    /// The images sent, by their number: their base64 PNG.
    images: HashMap<u32, String>,
    /// The placements by image and placement number, with where they stand.
    placed: Vec<((u32, u32), Placed)>,
}

/// A placement: the image's number, its first cell and its size in cells.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Placed {
    x: u16,
    y: u16,
    cols: u16,
    rows: u16,
}

impl Graphics {
    /// `bytes` with every graphics command cut out and a [`TOKEN`] in its place; a command not yet ended — or a read
    /// ending inside a command's start — waits for the next read.
    pub(crate) fn cut(&mut self, bytes: &[u8]) -> Vec<u8> {
        let mut input = std::mem::take(&mut self.held);
        input.extend_from_slice(bytes);
        let mut out = Vec::with_capacity(input.len());
        let mut at = 0;
        while let Some(start) = find(&input[at..], START).map(|found| at + found) {
            out.extend_from_slice(&input[at..start]);
            let body = start + START.len();
            let Some(end) = find(&input[body..], END).map(|found| body + found) else {
                if input.len() - start <= MOST_HELD {
                    self.held = input[start..].to_vec();
                }
                return out;
            };
            if let Some(command) = parse(&input[body..end]) {
                self.waiting.push_back(command);
                out.extend_from_slice(TOKEN.as_bytes());
            }
            at = end + END.len();
        }
        // A read may end inside a command's start (`ESC`, `ESC _`): held, or the command would reach the emulator.
        let rest = &input[at..];
        let partial = (1..START.len())
            .rev()
            .find(|&len| rest.ends_with(&START[..len]))
            .unwrap_or(0);
        out.extend_from_slice(&rest[..rest.len() - partial]);
        self.held = rest[rest.len() - partial..].to_vec();
        out
    }

    /// Applies the next command cut out, the cursor at `row`, `col`; returns an image sent whole, by its number and
    /// its base64, for the film to keep.
    pub(crate) fn apply(&mut self, row: u16, col: u16) -> Option<(u32, String)> {
        let command = self.waiting.pop_front()?;
        // A chunk after the first carries only `m`: it belongs to the image being sent.
        let action = if command.keys.contains_key("a") {
            command.key("a")
        } else if self.open.is_some() {
            "t"
        } else {
            ""
        };
        // Any command of its own ends a sending that never finished: nothing more is added to it.
        if command.keys.contains_key("a") {
            self.open = None;
        }
        match action {
            "t" | "T" => {
                if let Some(number) = command.number("i") {
                    // A first chunk starts its image anew.
                    self.open = Some((number, String::new()));
                }
                let (number, data) = self.open.as_mut()?;
                data.push_str(&command.payload);
                if command.key("m") == "1" {
                    return None;
                }
                let number = *number;
                let (_, data) = self.open.take()?;
                self.images.insert(number, data.clone());
                Some((number, data))
            }
            "p" => {
                let number = command.number("i")?;
                let placement = command.number("p").unwrap_or(0);
                let size = |key| {
                    command
                        .number(key)
                        .and_then(|n| u16::try_from(n).ok())
                        .unwrap_or(1)
                };
                self.placed.retain(|(key, _)| *key != (number, placement));
                self.placed.push((
                    (number, placement),
                    Placed {
                        x: col,
                        y: row,
                        cols: size("c"),
                        rows: size("r"),
                    },
                ));
                None
            }
            "d" => {
                match command.key("d") {
                    // Every placement, and with `A` the images' data too.
                    "a" => self.placed.clear(),
                    "A" => {
                        self.placed.clear();
                        self.images.clear();
                    }
                    "i" | "I" => {
                        if let Some(number) = command.number("i") {
                            self.placed.retain(|((image, _), _)| *image != number);
                            if command.key("d") == "I" {
                                self.images.remove(&number);
                            }
                        }
                    }
                    _ => {}
                }
                None
            }
            _ => None,
        }
    }

    /// The pictures standing now, their images by the film's place for them (`index` of an image's number), in the
    /// order of the screen: placed again where it stood, a picture is no change.
    pub(crate) fn standing(&self, index: impl Fn(u32) -> Option<usize>) -> Vec<Picture> {
        let mut standing = self
            .placed
            .iter()
            .filter(|((number, _), _)| self.images.contains_key(number))
            .filter_map(|((number, _), place)| {
                Some(Picture {
                    image: index(*number)?,
                    x: place.x,
                    y: place.y,
                    cols: place.cols,
                    rows: place.rows,
                })
            })
            .collect::<Vec<_>>();
        standing.sort_by_key(|picture| (picture.y, picture.x, picture.image));
        standing
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

/// A command's keys (`a=t,i=1,…`) and its payload after `;`; `None` where it is not text.
fn parse(body: &[u8]) -> Option<Command> {
    let body = std::str::from_utf8(body).ok()?;
    let (control, payload) = body.split_once(';').unwrap_or((body, ""));
    let keys = control
        .split(',')
        .filter_map(|pair| pair.split_once('='))
        .map(|(key, value)| (key.to_owned(), value.to_owned()))
        .collect();
    Some(Command {
        keys,
        payload: payload.to_owned(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_command_split_over_two_reads_is_cut_out_whole_and_text_around_it_stays() {
        let mut graphics = Graphics::default();
        let first = graphics.cut(b"ab\x1b_Ga=t,f=100,i=7,q=2,m=0;QUJD");
        assert_eq!(first, b"ab");
        let second = graphics.cut(b"RA==\x1b\\cd");
        assert_eq!(second, [TOKEN.as_bytes(), b"cd"].concat());
        assert_eq!(graphics.apply(0, 0), Some((7, "QUJDRA==".to_owned())));
    }

    #[test]
    fn a_read_ending_inside_a_command_s_start_loses_nothing() {
        let whole: &[u8] = b"ab\x1b_Ga=t,f=100,i=7,q=2,m=1;AAAA\x1b\\\x1b_Gm=0;BBBB\x1b\\cd";
        // Cut at every byte: the text and both chunks come through, whatever the reads.
        for split in 1..whole.len() {
            let mut graphics = Graphics::default();
            let mut out = graphics.cut(&whole[..split]);
            out.extend(graphics.cut(&whole[split..]));
            assert_eq!(
                out,
                [b"ab", TOKEN.repeat(2).as_bytes(), b"cd"].concat(),
                "split at {split}"
            );
            assert_eq!(graphics.apply(0, 0), None);
            assert_eq!(
                graphics.apply(0, 0),
                Some((7, "AAAABBBB".to_owned())),
                "split at {split}"
            );
        }
        // A lone escape at a read's end is held, then goes on as it was.
        let mut graphics = Graphics::default();
        assert_eq!(graphics.cut(b"x\x1b"), b"x");
        assert_eq!(graphics.cut(b"[6n"), b"\x1b[6n");
    }

    #[test]
    fn a_sending_cut_short_takes_nothing_from_the_next_image() {
        let mut graphics = Graphics::default();
        graphics.cut(
            b"\x1b_Ga=t,f=100,i=1,q=2,m=1;AAAA\x1b\\\x1b_Ga=d,d=A,q=2\x1b\\\x1b_Gm=0;BBBB\x1b\\",
        );
        assert_eq!(graphics.apply(0, 0), None);
        assert_eq!(graphics.apply(0, 0), None, "the delete ends the sending");
        assert_eq!(graphics.apply(0, 0), None, "a stray chunk makes no image");
        graphics.cut(b"\x1b_Ga=t,f=100,i=2,q=2,m=0;CCCC\x1b\\\x1b_Ga=d,d=I,i=2,q=2\x1b\\");
        assert_eq!(graphics.apply(0, 0), Some((2, "CCCC".to_owned())));
        graphics.apply(0, 0);
        graphics.cut(b"\x1b_Ga=p,i=2,p=1,c=1,r=1,q=2\x1b\\");
        graphics.apply(0, 0);
        assert!(
            graphics.standing(|_| Some(0)).is_empty(),
            "d=I took its data too"
        );
    }

    #[test]
    fn an_image_in_chunks_is_placed_at_the_cursor_and_taken_away_again() {
        let mut graphics = Graphics::default();
        let cut = graphics.cut(
            b"\x1b_Ga=t,f=100,i=3,q=2,m=1;AAAA\x1b\\\x1b_Gm=0;BBBB\x1b\\\x1b_Ga=p,i=3,p=1,c=19,r=10,C=1,q=2\x1b\\",
        );
        assert_eq!(cut, TOKEN.repeat(3).into_bytes());
        assert_eq!(graphics.apply(0, 0), None, "more to come");
        assert_eq!(graphics.apply(0, 0), Some((3, "AAAABBBB".to_owned())));
        assert_eq!(graphics.apply(21, 2), None);
        let index = |number| (number == 3).then_some(0);
        assert_eq!(
            graphics.standing(index),
            [Picture {
                image: 0,
                x: 2,
                y: 21,
                cols: 19,
                rows: 10
            }]
        );
        graphics.cut(b"\x1b_Ga=d,d=i,i=3,q=2\x1b\\");
        graphics.apply(0, 0);
        assert!(graphics.standing(index).is_empty());
        // Its data stays: placed again, it stands again.
        graphics.cut(b"\x1b_Ga=p,i=3,p=1,c=19,r=10,C=1,q=2\x1b\\");
        graphics.apply(5, 2);
        assert_eq!(graphics.standing(index).len(), 1);
        graphics.cut(b"\x1b_Ga=d,d=A,q=2\x1b\\");
        graphics.apply(0, 0);
        assert!(graphics.standing(index).is_empty());
    }
}
