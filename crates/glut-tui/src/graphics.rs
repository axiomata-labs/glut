//! Images drawn by the terminal itself (`docs/plans/glut-tui.md`, T46b/c): the kitty graphics protocol (Ghostty, kitty,
//! WezTerm) and iTerm2's inline images. ratatui knows no images, so a picture's cells are left blank and carry a mark —
//! an underline colour standing for the picture — that the frame's buffer gives up after each draw ([`take`]), as
//! the links' marks do. A picture whose every cell is still marked stands whole on the screen and is placed there
//! ([`Screen::show`]); one scrolled partly away or covered by a list, a question or an overlay (which clear their
//! cells, marks and all) is taken away.

use std::collections::{HashMap, HashSet};
use std::io::{self, Write};
use std::sync::Mutex;

use crossterm::{cursor, queue, style};
use ratatui::buffer::Buffer;
use ratatui::style::Color;

/// The protocol a terminal draws images with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Protocol {
    /// kitty's graphics protocol: an image sent once, placed by its number.
    Kitty,
    /// iTerm2's inline images: an image written into its cells each time it is placed.
    Iterm,
}

/// The protocol of this terminal, as it names itself; `None`: it draws none Glut knows.
pub fn protocol() -> Option<Protocol> {
    let program = std::env::var("TERM_PROGRAM")
        .unwrap_or_default()
        .to_lowercase();
    let term = std::env::var("TERM").unwrap_or_default().to_lowercase();
    if program == "iterm.app" {
        Some(Protocol::Iterm)
    } else if matches!(program.as_str(), "ghostty" | "wezterm")
        || term.contains("kitty")
        || term.contains("ghostty")
    {
        Some(Protocol::Kitty)
    } else {
        None
    }
}

/// The red of a picture's mark: no other underline colour of Glut's is this one (the links' are colour numbers).
const MARK_RED: u8 = 0xFE;
/// The most pictures one run of Glut marks: a mark's number is two bytes.
const MOST: usize = 65_535;
/// A cell's size in pixels, guessed, for the PNG a terminal is sent: it scales the picture into the cells anyway.
const CELL_PIXELS: (u32, u32) = (10, 20);
/// The most bytes of base64 in one kitty escape.
const CHUNK: usize = 4096;

/// A picture as marked: its image, and the cells it takes.
#[derive(Debug, Clone, PartialEq)]
struct Marked {
    image: glut::images::Image,
    columns: u16,
    rows: u16,
}

/// The pictures marked so far, by their number (one more than their place): the same image at the same size keeps
/// its number from frame to frame.
static MARKED: Mutex<Vec<Marked>> = Mutex::new(Vec::new());

/// The mark of `image` drawn in `columns` × `rows` cells; `None` once [`MOST`] pictures are marked.
pub fn mark(image: &glut::images::Image, columns: u16, rows: u16) -> Option<Color> {
    let wanted = Marked {
        image: image.clone(),
        columns,
        rows,
    };
    let mut marked = MARKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let at = match marked.iter().position(|known| *known == wanted) {
        Some(at) => at,
        None if marked.len() < MOST => {
            marked.push(wanted);
            marked.len() - 1
        }
        None => return None,
    };
    let number = u16::try_from(at + 1).ok()?;
    let [high, low] = number.to_be_bytes();
    Some(Color::Rgb(MARK_RED, high, low))
}

/// A picture standing whole on the screen.
#[derive(Debug, Clone, PartialEq)]
pub struct Placed {
    /// Its mark's number: the image at its size.
    pub number: u16,
    /// Its first cell.
    pub x: u16,
    pub y: u16,
    image: glut::images::Image,
    columns: u16,
    rows: u16,
}

/// The pictures of the frame drawn into `buffer` that stand whole — the same picture as often as it stands, in two
/// panes say; every mark wiped, so the frame goes out as it looks.
pub fn take(buffer: &mut Buffer) -> Vec<Placed> {
    let area = buffer.area;
    let mut marks: HashMap<(u16, u16), u16> = HashMap::new();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let Some(cell) = buffer.cell_mut((x, y)) else {
                continue;
            };
            let Color::Rgb(MARK_RED, high, low) = cell.underline_color else {
                continue;
            };
            cell.underline_color = Color::Reset;
            marks.insert((x, y), u16::from_be_bytes([high, low]));
        }
    }
    let known = MARKED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let mut placed = Vec::new();
    for (&(x, y), &number) in &marks {
        // A picture starts where its mark has none of its own to the left or above.
        let starts = (x == 0 || marks.get(&(x - 1, y)) != Some(&number))
            && (y == 0 || marks.get(&(x, y - 1)) != Some(&number));
        let Some(picture) = usize::from(number)
            .checked_sub(1)
            .and_then(|at| known.get(at))
        else {
            continue;
        };
        // Whole, or not at all: a picture half scrolled away or under a list is taken away.
        let whole = starts
            && (0..picture.rows).all(|row| {
                (0..picture.columns)
                    .all(|column| marks.get(&(x + column, y + row)) == Some(&number))
            });
        if whole {
            placed.push(Placed {
                number,
                x,
                y,
                image: picture.image.clone(),
                columns: picture.columns,
                rows: picture.rows,
            });
        }
    }
    placed.sort_by_key(|picture| (picture.number, picture.y, picture.x));
    placed
}

/// What the terminal holds of Glut's pictures: those whose data kitty keeps, the data iTerm2 is sent again, and
/// where each picture stands now.
#[derive(Debug, Default)]
pub struct Screen {
    sent: HashSet<u16>,
    data: HashMap<u16, String>,
    standing: HashMap<u16, Vec<(u16, u16)>>,
}

impl Screen {
    /// The screen was cleared, or left and taken again (an editor, the scrollback): every picture taken away and its
    /// data forgotten — sent again when it is next shown — whatever the terminal itself dropped.
    ///
    /// # Errors
    ///
    /// What writing to the terminal fails at.
    pub fn cleared(&mut self, out: &mut impl Write) -> io::Result<()> {
        if !self.sent.is_empty() {
            // `d=A`: every placement of Glut's and its data.
            queue!(out, style::Print("\x1b_Ga=d,d=A,q=2\x1b\\"))?;
            out.flush()?;
        }
        self.sent.clear();
        self.standing.clear();
        Ok(())
    }

    /// Whether any picture stands or was sent: what leaving the inline drawing has to take away.
    pub fn holds_any(&self) -> bool {
        !self.sent.is_empty() || !self.standing.is_empty()
    }

    /// The frame's `pictures` shown as `protocol` draws them: one gone or moved taken away, one new or moved placed.
    /// Returns whether the screen must be drawn anew from blank — iTerm2 draws a picture into its cells, which only
    /// text written over them takes away.
    ///
    /// # Errors
    ///
    /// What writing to the terminal fails at.
    pub fn show(
        &mut self,
        out: &mut impl Write,
        protocol: Protocol,
        pictures: &[Placed],
    ) -> io::Result<bool> {
        let mut now: HashMap<u16, Vec<(u16, u16)>> = HashMap::new();
        for picture in pictures {
            now.entry(picture.number)
                .or_default()
                .push((picture.x, picture.y));
        }
        let changed: Vec<u16> = self
            .standing
            .keys()
            .chain(now.keys())
            .copied()
            .collect::<HashSet<u16>>()
            .into_iter()
            .filter(|number| self.standing.get(number) != now.get(number))
            .collect();
        if changed.is_empty() {
            return Ok(false);
        }
        let gone = changed
            .iter()
            .any(|number| self.standing.contains_key(number));
        if protocol == Protocol::Iterm && gone {
            // Its cells keep the old picture: the screen is drawn anew, the pictures placed after.
            self.standing.clear();
            return Ok(true);
        }
        queue!(out, cursor::SavePosition)?;
        for number in changed {
            if protocol == Protocol::Kitty && self.standing.remove(&number).is_some() {
                // `d=i`: its placements go, its data stays for the next time it is placed.
                queue!(
                    out,
                    style::Print(format!("\x1b_Ga=d,d=i,i={number},q=2\x1b\\"))
                )?;
            }
            let Some(places) = now.get(&number) else {
                continue;
            };
            let Some(picture) = pictures.iter().find(|picture| picture.number == number) else {
                continue;
            };
            // Recorded before it is drawn: one that cannot be read is not read again every frame.
            self.standing.insert(number, places.clone());
            let needs_data = match protocol {
                Protocol::Kitty => !self.sent.contains(&number),
                Protocol::Iterm => !self.data.contains_key(&number),
            };
            if needs_data {
                let data = match data_of(picture) {
                    Data::Ready(data) => data,
                    Data::Unreadable => continue,
                    // Placed once it is ready: the next frame finds it among the changed.
                    Data::Wait => {
                        self.standing.remove(&number);
                        continue;
                    }
                };
                match protocol {
                    Protocol::Kitty => {
                        send_kitty(out, number, &data)?;
                        self.sent.insert(number);
                    }
                    Protocol::Iterm => {
                        self.data.insert(number, data);
                    }
                }
            }
            for (place, &(x, y)) in places.iter().enumerate() {
                queue!(out, cursor::MoveTo(x, y))?;
                match protocol {
                    // Placement `p` of its image, one for each place it stands; the cursor stays (`C=1`).
                    Protocol::Kitty => queue!(
                        out,
                        style::Print(format!(
                            "\x1b_Ga=p,i={number},p={},c={},r={},C=1,q=2\x1b\\",
                            place + 1,
                            picture.columns,
                            picture.rows
                        ))
                    )?,
                    Protocol::Iterm => {
                        let data = self.data.get(&number).map_or("", String::as_str);
                        queue!(
                            out,
                            style::Print(format!(
                                "\x1b]1337;File=inline=1;width={};height={};preserveAspectRatio=0:{data}\x07",
                                picture.columns, picture.rows
                            ))
                        )?;
                    }
                }
            }
        }
        queue!(out, cursor::RestorePosition)?;
        out.flush()?;
        Ok(false)
    }
}

/// A picture's data for the terminal (T47): ready, none where its image cannot be read, or still made beside the
/// screen.
enum Data {
    Ready(String),
    Unreadable,
    Wait,
}

/// A picture's data as made beside the screen: still making, or made (`None` where it cannot be read).
enum Making {
    Under,
    Made(Option<String>),
}

/// The data of the pictures made beside the screen, by their number, until a frame takes it.
static MADE: Mutex<Vec<(u16, Making)>> = Mutex::new(Vec::new());
/// The most made data waiting for a frame: a picture scrolled away before its data came is made again when it is
/// next shown, its data not kept meanwhile.
const MOST_MADE: usize = 8;

/// `picture`'s data as a PNG in base64, made beside the screen where it listens.
fn data_of(picture: &Placed) -> Data {
    let number = picture.number;
    {
        let mut made = MADE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        match made.iter().position(|(kept, _)| *kept == number) {
            Some(at) if matches!(made[at].1, Making::Under) => return Data::Wait,
            Some(at) => {
                let (_, Making::Made(data)) = made.remove(at) else {
                    return Data::Wait;
                };
                return data.map_or(Data::Unreadable, Data::Ready);
            }
            None => made.push((number, Making::Under)),
        }
    }
    let (image, columns, rows) = (picture.image.clone(), picture.columns, picture.rows);
    let make = move || {
        let data = glut::images::png_within(
            &image,
            u32::from(columns) * CELL_PIXELS.0,
            u32::from(rows) * CELL_PIXELS.1,
        )
        .ok()
        .map(|png| glut::images::to_base64(&png));
        let mut made = MADE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        made.retain(|(kept, _)| *kept != number);
        made.push((number, Making::Made(data)));
        if made
            .iter()
            .filter(|(_, making)| matches!(making, Making::Made(_)))
            .count()
            > MOST_MADE
            && let Some(at) = made
                .iter()
                .position(|(_, making)| matches!(making, Making::Made(_)))
        {
            made.remove(at);
        }
    };
    match crate::pictures::beside(make) {
        Ok(()) => Data::Wait,
        Err(make) => {
            make();
            data_of(picture)
        }
    }
}

/// An image's data sent to kitty as image `number`, in chunks: kept, not yet placed (`a=t`).
fn send_kitty(out: &mut impl Write, number: u16, data: &str) -> io::Result<()> {
    let chunks: Vec<&[u8]> = data.as_bytes().chunks(CHUNK).collect();
    for (at, chunk) in chunks.iter().enumerate() {
        let more = u8::from(at + 1 < chunks.len());
        let chunk = String::from_utf8_lossy(chunk);
        if at == 0 {
            queue!(
                out,
                style::Print(format!(
                    "\x1b_Ga=t,f=100,i={number},q=2,m={more};{chunk}\x1b\\"
                ))
            )?;
        } else {
            queue!(out, style::Print(format!("\x1b_Gm={more};{chunk}\x1b\\")))?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::layout::Rect;

    fn image() -> glut::images::Image {
        glut::images::Image {
            number: 1,
            path: std::path::PathBuf::from("/h/.glut/images/x.png"),
            media: "image/png".into(),
            width: 4,
            height: 2,
            bytes: 1,
        }
    }

    #[test]
    fn a_picture_whole_on_the_screen_is_placed_as_often_as_it_stands_and_a_cut_one_is_not() {
        let whole = mark(&image(), 3, 2).unwrap();
        let cut = mark(&image(), 5, 2).unwrap();
        assert_ne!(whole, cut, "another size, another picture");
        assert_eq!(mark(&image(), 3, 2), Some(whole), "the same keeps its mark");
        let mut buffer = Buffer::empty(Rect::new(0, 0, 12, 4));
        // Twice side by side (two panes), and one with only one row of its two left (scrolled half away).
        for (x0, y0) in [(0, 1), (7, 1)] {
            for (x, y) in [(0, 0), (1, 0), (2, 0), (0, 1), (1, 1), (2, 1)] {
                buffer[(x0 + x, y0 + y)].underline_color = whole;
            }
        }
        for x in 0..5 {
            buffer[(x, 3)].underline_color = cut;
        }
        let placed = take(&mut buffer);
        let places: Vec<(u16, u16)> = placed
            .iter()
            .map(|picture| (picture.x, picture.y))
            .collect();
        assert_eq!(places, [(0, 1), (7, 1)]);
        assert!(
            (0..12).all(|x| (0..4).all(|y| buffer[(x, y)].underline_color == Color::Reset)),
            "the frame goes out unmarked"
        );
    }

    #[test]
    fn kitty_sends_an_image_once_in_chunks_and_takes_away_what_went() {
        let mut out = Vec::new();
        send_kitty(&mut out, 7, &"A".repeat(CHUNK + 10)).unwrap();
        let sent = String::from_utf8(out).unwrap();
        assert!(sent.starts_with("\x1b_Ga=t,f=100,i=7,q=2,m=1;"));
        assert!(sent.contains("\x1b_Gm=0;AAAAAAAAAA\x1b\\"));
        // A picture gone is taken away; on iTerm2 the screen is drawn anew instead.
        let mut screen = Screen::default();
        screen.standing.insert(3, vec![(1, 1)]);
        let mut out = Vec::new();
        assert!(!screen.show(&mut out, Protocol::Kitty, &[]).unwrap());
        assert!(
            String::from_utf8(out)
                .unwrap()
                .contains("\x1b_Ga=d,d=i,i=3,q=2\x1b\\")
        );
        screen.standing.insert(3, vec![(1, 1)]);
        assert!(screen.show(&mut Vec::new(), Protocol::Iterm, &[]).unwrap());
        // Cleared, it forgets what it sent, and says so to the terminal.
        screen.sent.insert(3);
        let mut out = Vec::new();
        screen.cleared(&mut out).unwrap();
        assert!(String::from_utf8(out).unwrap().contains("d=A"));
        assert!(!screen.holds_any());
    }
}
