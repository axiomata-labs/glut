//! Glimm, Glut's mascot (T27, drawn finely in T72): a small flame with eyes, in true colour, drawn in quadrant blocks —
//! each terminal cell 2 × 2 pixels (a pixel is one wide and two tall on the screen). The owner chose design A of
//! three (2026-10-22); T72 redrew it, and the rounder C, on a finer raster. Both drawings are kept (`glimm/a-*.txt`,
//! `glimm/c-*.txt`, [`Design`]), A is the one in use. A drawing is a file of hand-drawn frames per mood, three sizes
//! each (the large one of the welcome, the small one beside a question and in the panel, the mini one before a
//! thought); `crates/glut-tui/examples/glimm_preview.rs` shows a file as it is drawn here.
//!
//! Five moods: resting (breathes, blinks now and then), working (the flame flickers, sparks rise), waiting for the
//! owner (wide eyes and a "!"), done (pleased eyes and a twinkle) and failed (out and grey, only the ember below
//! still glowing, smoke rising). The white-hot heart slowly pulses between yellow and white, and where the terminal's
//! background colour is known (`background.rs`) a glow runs out of the flame into it.

use std::sync::OnceLock;

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

use crate::background::Rgb;

/// What Glimm shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Resting,
    Working,
    Waiting,
    Done,
    Failed,
}

/// Which Glimm: the large one of the welcome (32 × 20 pixels, 16 columns by 10 rows), the small one (20 × 12, 10 by 6)
/// that stood beside a question and in the side panel until 2026-11-05 — its drawing kept, drawn nowhere now (owner:
/// too playful there) —, and the mini one (8 × 4, 4 by 2) in front of a thought while the model
/// thinks — a small flame without eyes and without a glow, only ever working.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Large,
    #[allow(dead_code)]
    Small,
    Mini,
}

/// A drawing of Glimm: its frames are files, one per size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Design {
    /// The teardrop flame with a pointed tip (the owner's choice).
    A,
    /// The rounder, plumper one with large eyes.
    C,
}

impl Design {
    const ALL: [Design; 2] = [Design::A, Design::C];
}

/// The drawing in use: its place in [`Design::ALL`].
const ACTIVE: usize = 0;

/// The files of the drawings, by design and size (large, small, mini).
const TEXTS: [&str; 6] = [
    include_str!("glimm/a-large.txt"),
    include_str!("glimm/a-small.txt"),
    include_str!("glimm/a-mini.txt"),
    include_str!("glimm/c-large.txt"),
    include_str!("glimm/c-small.txt"),
    include_str!("glimm/c-mini.txt"),
];

/// One hand-drawn frame: its name (`rest0`, `work3`, `wait1`, …) and its pixels as letters of [`color`].
#[derive(Debug)]
struct Frame {
    name: String,
    rows: Vec<Vec<char>>,
}

/// A drawing in one size: the pixel grid's width and height and its frames.
#[derive(Debug)]
struct Raster {
    width: usize,
    height: usize,
    frames: Vec<Frame>,
}

/// The letters a drawing may use.
const LETTERS: &str = ".RroaywehmEs";

/// Reads a drawing's file: `#` comments, a line `size <width> <height>` (both even), then frames — `@ <name>` and
/// exactly `height` lines of exactly `width` letters, blank lines between.
fn parse(text: &str) -> Result<Raster, String> {
    let mut size: Option<(usize, usize)> = None;
    let mut frames: Vec<Frame> = Vec::new();
    let mut open: Option<Frame> = None;
    let close = |frame: Option<Frame>, at: usize, size: Option<(usize, usize)>| {
        let Some(frame) = frame else { return Ok(None) };
        let (_, height) =
            size.ok_or_else(|| format!("line {at}: no size line before the frames"))?;
        if frame.rows.len() != height {
            return Err(format!(
                "frame {}: {} lines, not {height}",
                frame.name,
                frame.rows.len()
            ));
        }
        Ok(Some(frame))
    };
    for (index, line) in text.lines().enumerate() {
        let at = index + 1;
        let line = line.trim_end();
        if line.starts_with('#') {
            continue;
        }
        if line.is_empty() {
            frames.extend(close(open.take(), at, size)?);
            continue;
        }
        let Some((width, _)) = size else {
            let mut words = line.split_whitespace();
            let number = |word: Option<&str>| word.and_then(|w| w.parse::<usize>().ok());
            match (words.next(), number(words.next()), number(words.next())) {
                (Some("size"), Some(w), Some(h))
                    if w >= 2 && h >= 2 && w % 2 == 0 && h % 2 == 0 =>
                {
                    size = Some((w, h));
                    continue;
                }
                _ => {
                    return Err(format!(
                        "line {at}: expected `size <even width> <even height>`"
                    ));
                }
            }
        };
        if let Some(name) = line.strip_prefix("@ ") {
            frames.extend(close(open.take(), at, size)?);
            let name = name.trim().to_owned();
            if frames.iter().any(|f| f.name == name) {
                return Err(format!("line {at}: frame {name} twice"));
            }
            open = Some(Frame {
                name,
                rows: Vec::new(),
            });
            continue;
        }
        let frame = open
            .as_mut()
            .ok_or_else(|| format!("line {at}: pixels outside a frame"))?;
        let row: Vec<char> = line.chars().collect();
        if row.len() != width {
            return Err(format!("line {at}: {} characters, not {width}", row.len()));
        }
        if let Some(bad) = row.iter().find(|c| !LETTERS.contains(**c)) {
            return Err(format!("line {at}: unknown letter {bad:?}"));
        }
        frame.rows.push(row);
    }
    frames.extend(close(open.take(), text.lines().count(), size)?);
    let (width, height) = size.ok_or("no size line")?;
    Ok(Raster {
        width,
        height,
        frames,
    })
}

/// A design's raster in a size, read from its file once.
fn raster(design: Design, size: Size) -> &'static Raster {
    static RASTERS: [OnceLock<Raster>; 6] = [const { OnceLock::new() }; 6];
    let at = Design::ALL
        .iter()
        .position(|&d| d == design)
        .unwrap_or_default()
        * 3
        + size as usize;
    RASTERS[at].get_or_init(|| {
        // The files are part of the program and every one is parsed in the tests.
        parse(TEXTS[at]).unwrap_or_else(|err| panic!("Glimm's drawing {at}: {err}"))
    })
}

impl Raster {
    /// The frame called `name`; a drawing without it (the mini one has only its working frames) shows its first.
    fn frame(&self, name: &str) -> &Frame {
        self.frames
            .iter()
            .find(|f| f.name == name)
            .unwrap_or(&self.frames[0])
    }

    /// How many frames are called `<prefix>0`, `<prefix>1`, … without a gap.
    fn count(&self, prefix: &str) -> usize {
        (0..)
            .take_while(|n| self.frames.iter().any(|f| f.name == format!("{prefix}{n}")))
            .count()
    }
}

/// Screen ticks per step of the calm animations (the "!", the twinkle, the smoke) and of the flicker while it works:
/// a small Glimm beside text moves slower, so it does not pull the eye from what it stands beside (owner: the asking
/// one was "zu hektisch").
fn pace(size: Size) -> (usize, usize) {
    match size {
        Size::Large => (2, 1),
        Size::Small => (5, 3),
        Size::Mini => (10, 8),
    }
}

/// The resting flame's frames, step by step: mostly the plain one, a breath, a sway each way (`rest0` leans left,
/// `rest2` right, `rest1` stands a pixel taller, `rest3` is the plain one).
const BREATH: [usize; 12] = [3, 3, 1, 2, 2, 3, 3, 1, 3, 0, 0, 3];
/// Ticks per step of the breath — at the screen's 120 ms tick a breath takes about four seconds.
const BREATH_STEP: usize = 3;
/// A blink every this many ticks (about five seconds), with the eyes shut this many.
const BLINK_EVERY: usize = 42;
const BLINK_TICKS: usize = 2;

/// Ticks of the heart's pulse between yellow and white: three seconds resting, a little over one while it works.
const PULSE_CALM: f32 = 25.0;
const PULSE_WORK: f32 = 12.0;

/// How strong a glow the owner wanted (0.18), and the factor of each frame family on it: working warm and wide,
/// resting narrow, waiting a little more as the "!" shows and a little less as it is off, done as it is, failed none.
const GLOW: f32 = 0.18;
const GLOW_SCALE: [(&str, f32); 7] = [
    ("fail", 0.0),
    ("work", 1.4),
    ("wait0", 1.2),
    ("wait1", 0.8),
    ("wait", 1.0),
    ("rest", 0.6),
    ("done", 1.0),
];
/// The glow's colour, blended over the terminal's background.
const GLOW_COLOR: Rgb = (0xb8, 0x36, 0x0f);

/// A pixel's colour by its letter: the flame from deep red at its edge to white-hot in its heart, the eyes, the ember
/// and the smoke. `.` is no pixel.
fn color(pixel: char, ash: bool) -> Option<Rgb> {
    Some(match (pixel, ash) {
        ('R', false) => (0x6e, 0x1a, 0x0c),
        ('r', false) => (0xb8, 0x36, 0x0f),
        ('o', false) => (0xff, 0x7a, 0x1a),
        ('a', false) => (0xff, 0xa0, 0x40),
        ('y', false) => (0xff, 0xd3, 0x6b),
        ('w', false) => (0xff, 0xf2, 0xcc),
        // Gone out: the flame in greys, darkest at the edge.
        ('R', true) => (0x2e, 0x2e, 0x33),
        ('r', true) => (0x45, 0x45, 0x4c),
        ('o', true) => (0x5c, 0x5c, 0x64),
        ('a', true) => (0x6e, 0x6e, 0x76),
        ('y', true) => (0x83, 0x83, 0x8b),
        ('w', true) => (0x9a, 0x9a, 0xa2),
        ('e', _) => (0x1a, 0x0f, 0x0c),
        ('h', _) => (0xff, 0xf7, 0xe6),
        ('m', _) => (0x2a, 0x14, 0x10),
        ('E', _) => (0xd9, 0x48, 0x0f),
        ('s', _) => (0x80, 0x80, 0x8c),
        _ => return None,
    })
}

/// How far a ping-pong over `count` frames has got after `step` steps: 0, 1, … count-1, count-2, … 1, 0, 1, …
fn ping_pong(step: usize, count: usize) -> usize {
    if count < 2 {
        return 0;
    }
    let period = 2 * count - 2;
    let at = step % period;
    if at < count { at } else { period - at }
}

/// The name of the frame for `mood` at animation step `tick`. Held still (calm and off, TUI9b) Glimm stands at a frame
/// with its eyes open and nothing moving.
fn frame_name(size: Size, mood: Mood, tick: usize, still: bool, works: usize) -> String {
    let (calm, work) = pace(size);
    let step = tick / calm;
    match mood {
        Mood::Resting if still => "rest3".into(),
        Mood::Resting if tick % BLINK_EVERY < BLINK_TICKS => "restblink".into(),
        Mood::Resting => format!("rest{}", BREATH[tick / BREATH_STEP % BREATH.len()]),
        Mood::Working if still => "rest3".into(),
        Mood::Working => format!("work{}", ping_pong(tick / work, works)),
        Mood::Waiting if still || step % 4 < 3 => "wait0".into(),
        Mood::Waiting => "wait1".into(),
        Mood::Done if !still && step % 6 < 3 => "done1".into(),
        Mood::Done => "done0".into(),
        Mood::Failed if still => "fail0".into(),
        Mood::Failed => format!("fail{}", step % 3),
    }
}

/// The heart's mix between yellow (0) and white (1).
fn pulse(mood: Mood, tick: usize, still: bool) -> f32 {
    if still || mood == Mood::Failed {
        return 1.0;
    }
    let period = if mood == Mood::Working {
        PULSE_WORK
    } else {
        PULSE_CALM
    };
    0.5 + 0.5 * (std::f32::consts::TAU * tick as f32 / period).sin()
}

/// `over` blended on `under` by `alpha`.
fn blend(over: Rgb, under: Rgb, alpha: f32) -> Rgb {
    let part = |o: u8, u: u8| {
        (f32::from(u) + (f32::from(o) - f32::from(u)) * alpha)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (
        part(over.0, under.0),
        part(over.1, under.1),
        part(over.2, under.2),
    )
}

/// The glow's alpha at a real distance from the flame (a pixel is one wide and two tall, so the distance is measured in
/// the width of one): rings of 0.5 / 0.28 / 0.12 at 2 / 4 / 6 pixels, joined by straight lines, gone at 7.5.
fn alpha_at(distance: f32) -> f32 {
    const POINTS: [(f32, f32); 5] = [(0.0, 0.5), (2.0, 0.5), (4.0, 0.28), (6.0, 0.12), (7.5, 0.0)];
    for pair in POINTS.windows(2) {
        let ((d0, a0), (d1, a1)) = (pair[0], pair[1]);
        if distance <= d1 {
            return a0 + (a1 - a0) * ((distance - d0) / (d1 - d0)).clamp(0.0, 1.0);
        }
    }
    0.0
}

/// Whether a letter is the flame's body — the glow's source, not an eye or a spark.
fn is_flame(letter: char) -> bool {
    "Rroayw".contains(letter)
}

/// The glow's alpha for every pixel of a frame, from the flame's mask. It fades to nothing at the raster's edge, so a
/// glow never ends in a hard line there.
fn glow_field(frame: &Frame, width: usize, height: usize) -> Vec<Vec<f32>> {
    let flame: Vec<(f32, f32)> = frame
        .rows
        .iter()
        .enumerate()
        .flat_map(|(y, row)| {
            row.iter()
                .enumerate()
                .filter(|&(_, &c)| is_flame(c))
                .map(move |(x, _)| (x as f32, y as f32 * 2.0))
        })
        .collect();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| {
                    if flame.is_empty() {
                        return 0.0;
                    }
                    let (px, py) = (x as f32, y as f32 * 2.0);
                    let near = flame
                        .iter()
                        .map(|&(fx, fy)| (fx - px).powi(2) + (fy - py).powi(2))
                        .fold(f32::MAX, f32::min);
                    let edge = x.min(width - 1 - x).min(2 * y).min(2 * (height - 1 - y));
                    alpha_at(near.sqrt()) * (edge as f32 / 3.0).min(1.0)
                })
                .collect()
        })
        .collect()
}

/// The pixels of Glimm as they are drawn: the frame's letters as colours with the heart's pulse, then the glow into the
/// transparent pixels around the flame where the terminal's background is known.
fn pixels(
    design: Design,
    size: Size,
    mood: Mood,
    tick: usize,
    still: bool,
    background: Option<Rgb>,
) -> (usize, Vec<Vec<Option<Rgb>>>) {
    let raster = raster(design, size);
    let name = frame_name(size, mood, tick, still, raster.count("work"));
    let frame = raster.frame(&name);
    let ash = mood == Mood::Failed;
    let t = pulse(mood, tick, still);
    let heart = blend(
        color('w', ash).unwrap_or_default(),
        color('y', ash).unwrap_or_default(),
        t,
    );
    let mut grid: Vec<Vec<Option<Rgb>>> = frame
        .rows
        .iter()
        .map(|row| {
            row.iter()
                .map(|&c| if c == 'w' { Some(heart) } else { color(c, ash) })
                .collect()
        })
        .collect();
    // The mini flame stands beside a line of text: no glow there.
    let scale = GLOW_SCALE
        .iter()
        .find(|(prefix, _)| frame.name.starts_with(prefix))
        .map_or(1.0, |&(_, scale)| scale);
    let strength = GLOW * scale * (0.85 + 0.15 * t);
    if let (Some(background), true, true) = (background, strength > 0.0, size != Size::Mini) {
        let field = glow_field(frame, raster.width, raster.height);
        for (row, alphas) in grid.iter_mut().zip(&field) {
            for (pixel, alpha) in row.iter_mut().zip(alphas) {
                let alpha = (alpha * strength).min(0.9);
                if pixel.is_none() && alpha >= 0.02 {
                    *pixel = Some(blend(GLOW_COLOR, background, alpha));
                }
            }
        }
    }
    (raster.width, grid)
}

/// One terminal cell: a glyph, its foreground and its background (`None` is the terminal's own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    glyph: char,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
}

/// The glyph of a foreground mask (bit 0 upper left, 1 upper right, 2 lower left, 3 lower right).
const GLYPHS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// The mean of some colours.
fn mean(colors: &[Rgb]) -> Rgb {
    let n = colors.len().max(1) as f32;
    let part =
        |f: fn(&Rgb) -> u8| (colors.iter().map(|c| f32::from(f(c))).sum::<f32>() / n).round() as u8;
    (part(|c| c.0), part(|c| c.1), part(|c| c.2))
}

/// The squared error of some colours against their mean.
fn error(colors: &[Rgb]) -> f32 {
    let m = mean(colors);
    let d = |a: u8, b: u8| (f32::from(a) - f32::from(b)).powi(2);
    colors
        .iter()
        .map(|c| d(c.0, m.0) + d(c.1, m.1) + d(c.2, m.2))
        .sum()
}

/// A 2 × 2 block (upper left, upper right, lower left, lower right; `None` is transparent) as one cell. Four opaque
/// pixels are split into the two groups of least colour error (all 8 partitions tried), the foreground's mask picks
/// the glyph. A block with a transparent pixel keeps the terminal's own background there — never mixed with a colour —
/// and its opaque pixels are one group.
fn cell(block: [Option<Rgb>; 4]) -> Cell {
    let opaque = (0..4)
        .filter(|&i| block[i].is_some())
        .fold(0usize, |m, i| m | 1 << i);
    let pick = |mask: usize| -> Vec<Rgb> {
        (0..4)
            .filter(|i| mask >> i & 1 == 1)
            .filter_map(|i| block[i])
            .collect()
    };
    if opaque == 0 {
        return Cell {
            glyph: ' ',
            fg: None,
            bg: None,
        };
    }
    if opaque != 15 {
        return Cell {
            glyph: GLYPHS[opaque],
            fg: Some(mean(&pick(opaque))),
            bg: None,
        };
    }
    // Group A is the pixels of `mask` (the pixel at bit 3 stays in B), so 8 partitions.
    let (_, mask) = (0..8usize)
        .map(|mask| (error(&pick(mask)) + error(&pick(15 & !mask)), mask))
        .fold(
            (f32::MAX, 0),
            |best, now| if now.0 < best.0 { now } else { best },
        );
    if mask == 0 {
        // One colour: a space on a background of it, no gap a font could leave.
        return Cell {
            glyph: ' ',
            fg: None,
            bg: Some(mean(&pick(15))),
        };
    }
    Cell {
        glyph: GLYPHS[mask],
        fg: Some(mean(&pick(mask))),
        bg: Some(mean(&pick(15 & !mask))),
    }
}

/// A cell as a span.
fn span(cell: Cell) -> Span<'static> {
    if cell.glyph == ' ' && cell.fg.is_none() && cell.bg.is_none() {
        return Span::raw(" ");
    }
    let rgb = |(r, g, b): Rgb| Color::Rgb(r, g, b);
    let mut style = Style::default();
    if let Some(fg) = cell.fg {
        style = style.fg(rgb(fg));
    }
    if let Some(bg) = cell.bg {
        style = style.bg(rgb(bg));
    }
    Span::styled(cell.glyph.to_string(), style)
}

/// Glimm as lines of quadrant blocks, one cell for each 2 × 2 pixels.
fn draw(
    design: Design,
    size: Size,
    mood: Mood,
    tick: usize,
    still: bool,
    background: Option<Rgb>,
) -> Vec<Line<'static>> {
    let (width, grid) = pixels(design, size, mood, tick, still, background);
    grid.chunks(2)
        .map(|pair| {
            let spans = (0..width / 2)
                .map(|col| {
                    let at = |row: usize, x: usize| pair.get(row).and_then(|r| r[col * 2 + x]);
                    span(cell([at(0, 0), at(0, 1), at(1, 0), at(1, 1)]))
                })
                .collect::<Vec<_>>();
            Line::from(spans)
        })
        .collect()
}

/// Glimm as lines of quadrant blocks, in the drawing in use. The reader mode has no pictures (TUI9b), so it is none
/// there; held still, Glimm stands at a frame with its eyes open.
pub fn lines(size: Size, mood: Mood, tick: usize) -> Vec<Line<'static>> {
    if crate::motion::reader() {
        return Vec::new();
    }
    draw(
        Design::ALL[ACTIVE],
        size,
        mood,
        tick,
        crate::motion::still(),
        crate::background::known(),
    )
}

/// How many terminal columns and rows a Glimm of `size` takes.
#[allow(dead_code)]
pub fn cells(size: Size) -> (u16, u16) {
    let raster = raster(Design::ALL[ACTIVE], size);
    ((raster.width / 2) as u16, (raster.height / 2) as u16)
}

/// A one-cell flame for the header, flickering while Glut works.
pub fn spark(working: bool, frame: usize) -> &'static str {
    const SPARKS: [&str; 4] = ["▴", "▵", "▲", "▵"];
    if working && !crate::motion::still() {
        SPARKS[frame % SPARKS.len()]
    } else {
        "▲"
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const MOODS: [Mood; 5] = [
        Mood::Resting,
        Mood::Working,
        Mood::Waiting,
        Mood::Done,
        Mood::Failed,
    ];
    const SIZES: [Size; 3] = [Size::Large, Size::Small, Size::Mini];
    /// Ghostty's own, dark, and a light one.
    const DARK: Rgb = (0x1e, 0x1e, 0x2e);
    const LIGHT: Rgb = (0xf5, 0xf5, 0xf5);
    const RED: Rgb = (255, 0, 0);

    fn painted(grid: &[Vec<Option<Rgb>>]) -> usize {
        grid.iter().flatten().filter(|p| p.is_some()).count()
    }

    #[test]
    fn every_drawing_reads_and_has_the_frames_its_moods_need() {
        let all = [
            "rest0",
            "rest1",
            "rest2",
            "rest3",
            "restblink",
            "work0",
            "work1",
            "work2",
            "work3",
            "work4",
            "work5",
            "wait0",
            "wait1",
            "done0",
            "done1",
            "fail0",
            "fail1",
            "fail2",
        ];
        for design in Design::ALL {
            for (size, width, height) in [
                (Size::Large, 32, 20),
                (Size::Small, 20, 12),
                (Size::Mini, 8, 4),
            ] {
                let raster = raster(design, size);
                assert_eq!(
                    (raster.width, raster.height),
                    (width, height),
                    "{design:?} {size:?}"
                );
                if size == Size::Mini {
                    assert_eq!(raster.count("work"), 4, "{design:?}");
                    continue;
                }
                for name in all {
                    assert!(
                        raster.frames.iter().any(|f| f.name == name),
                        "{design:?} {size:?}: no frame {name}"
                    );
                }
                assert_eq!(raster.count("work"), 6);
                assert_eq!(raster.frames.len(), all.len());
            }
        }
        assert_eq!(cells(Size::Large), (16, 10));
        assert_eq!(cells(Size::Small), (10, 6));
        assert_eq!(cells(Size::Mini), (4, 2));
    }

    #[test]
    fn a_drawing_with_a_fault_is_refused_with_its_line() {
        let good = "size 4 2\n@ a\n..rr\n.rr.\n";
        assert!(parse(good).is_ok());
        assert!(parse("@ a\n....\n").unwrap_err().contains("size"));
        assert!(parse("size 3 2\n").unwrap_err().contains("even"));
        assert!(
            parse("size 4 2\n@ a\n...\n..\n")
                .unwrap_err()
                .contains("line 3")
        );
        assert!(
            parse("size 4 2\n@ a\n....\n")
                .unwrap_err()
                .contains("1 lines")
        );
        assert!(
            parse("size 4 2\n@ a\n..x.\n....\n")
                .unwrap_err()
                .contains("letter")
        );
        assert!(
            parse("size 4 2\n@ a\n....\n....\n\n@ a\n....\n....\n")
                .unwrap_err()
                .contains("twice")
        );
        assert!(parse("size 4 2\n....\n").unwrap_err().contains("outside"));
    }

    #[test]
    fn every_mood_and_frame_keeps_glimms_size() {
        for design in Design::ALL {
            for size in SIZES {
                let raster = raster(design, size);
                let (cols, rows) = (raster.width / 2, raster.height / 2);
                for mood in MOODS {
                    for background in [None, Some(DARK), Some(LIGHT)] {
                        for still in [false, true] {
                            for tick in 0..90 {
                                let drawn = draw(design, size, mood, tick, still, background);
                                assert_eq!(drawn.len(), rows, "{design:?} {size:?} {mood:?}");
                                for line in drawn {
                                    assert_eq!(
                                        line.width(),
                                        cols,
                                        "{design:?} {size:?} {mood:?} {tick}"
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn every_mood_looks_different_and_a_failed_glimm_has_gone_grey_but_its_ember() {
        for design in Design::ALL {
            for size in [Size::Large, Size::Small] {
                let looks: Vec<_> = MOODS
                    .iter()
                    .map(|&mood| draw(design, size, mood, 0, false, None))
                    .collect();
                for (at, look) in looks.iter().enumerate() {
                    for other in &looks[at + 1..] {
                        assert_ne!(look, other, "{design:?} {size:?}: two moods look alike");
                    }
                }
                let (_, failed) = pixels(design, size, Mood::Failed, 0, false, None);
                let colors: Vec<Rgb> = failed.iter().flatten().flatten().copied().collect();
                assert!(
                    colors.contains(&(0xd9, 0x48, 0x0f)),
                    "{design:?} {size:?}: the ember still glows"
                );
                for flame in ['o', 'a', 'y', 'w'] {
                    assert!(
                        !colors.contains(&color(flame, false).unwrap()),
                        "{design:?} {size:?}: {flame} is still coloured"
                    );
                }
            }
        }
    }

    #[test]
    fn a_resting_glimm_breathes_and_blinks_now_and_then() {
        let works = 6;
        let name = |tick| frame_name(Size::Large, Mood::Resting, tick, false, works);
        assert_eq!(name(0), "restblink", "the first ticks of a blink cycle");
        assert_eq!(name(BLINK_TICKS), "rest3");
        assert_ne!(name(BLINK_EVERY - 1), "restblink");
        let frames: Vec<String> = (BLINK_TICKS..BLINK_EVERY).map(name).collect();
        for sway in ["rest0", "rest1", "rest2", "rest3"] {
            assert!(frames.iter().any(|f| f == sway), "{sway} is never shown");
        }
        // Held still, it stands at the plain frame with its eyes open, whatever the tick.
        for tick in 0..100 {
            assert_eq!(
                frame_name(Size::Large, Mood::Resting, tick, true, works),
                "rest3"
            );
        }
    }

    #[test]
    fn a_working_glimm_runs_through_its_flames_and_back() {
        let seen: Vec<String> = (0..10)
            .map(|step| frame_name(Size::Large, Mood::Working, step, false, 6))
            .collect();
        assert_eq!(
            seen,
            [
                "work0", "work1", "work2", "work3", "work4", "work5", "work4", "work3", "work2",
                "work1"
            ]
        );
        assert_eq!(ping_pong(10, 6), 0, "and round again");
        assert_eq!(ping_pong(5, 1), 0);
        // The mini flame has four.
        let mini: Vec<usize> = (0..6).map(|step| ping_pong(step, 4)).collect();
        assert_eq!(mini, [0, 1, 2, 3, 2, 1]);
    }

    #[test]
    fn the_waiting_glimms_bang_blinks_and_the_done_ones_twinkle_comes_and_goes() {
        let large = |mood, tick| frame_name(Size::Large, mood, tick, false, 6);
        let (calm, _) = pace(Size::Large);
        let bangs: Vec<String> = (0..4).map(|s| large(Mood::Waiting, s * calm)).collect();
        assert_eq!(bangs, ["wait0", "wait0", "wait0", "wait1"]);
        assert_eq!(large(Mood::Done, 0), "done1");
        assert_eq!(large(Mood::Done, 3 * calm), "done0");
        let smoke: Vec<String> = (0..4).map(|s| large(Mood::Failed, s * calm)).collect();
        assert_eq!(smoke, ["fail0", "fail1", "fail2", "fail0"]);
    }

    #[test]
    fn a_glimm_held_still_does_not_move() {
        for design in Design::ALL {
            for size in SIZES {
                for mood in MOODS {
                    let first = draw(design, size, mood, 0, true, Some(DARK));
                    for tick in [1, 7, 33, 100] {
                        assert_eq!(
                            first,
                            draw(design, size, mood, tick, true, Some(DARK)),
                            "{design:?} {size:?} {mood:?} {tick}"
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn the_heart_pulses_between_yellow_and_white_and_not_when_still_or_out() {
        let ticks = (0..25).map(|t| pulse(Mood::Resting, t, false));
        let (low, high) = ticks.fold((1.0f32, 0.0f32), |(lo, hi), p| (lo.min(p), hi.max(p)));
        assert!(low < 0.05 && high > 0.95, "{low} {high}");
        assert_eq!(pulse(Mood::Resting, 7, true), 1.0);
        assert_eq!(pulse(Mood::Failed, 7, false), 1.0);
        // Working pulses faster than resting.
        let first_peak = |mood| (0..40).find(|&t| pulse(mood, t, false) > 0.99).unwrap();
        assert!(first_peak(Mood::Working) < first_peak(Mood::Resting));
        // The same frame (both ticks show `wait0`), the heart in another colour.
        let at = |tick| pixels(Design::A, Size::Large, Mood::Waiting, tick, false, None).1;
        assert_ne!(at(0), at(6));
    }

    #[test]
    fn the_encoder_gives_every_mask_its_glyph_and_keeps_transparent_as_the_terminals_own() {
        for (mask, &glyph) in GLYPHS.iter().enumerate().take(15).skip(1) {
            let block = std::array::from_fn(|i| (mask >> i & 1 == 1).then_some(RED));
            let c = cell(block);
            assert_eq!(c.glyph, glyph, "mask {mask}");
            assert_eq!((c.fg, c.bg), (Some(RED), None), "mask {mask}");
        }
        assert_eq!(
            cell([None; 4]),
            Cell {
                glyph: ' ',
                fg: None,
                bg: None
            }
        );
        // One colour in all four: a space on its background.
        assert_eq!(
            cell([Some(RED); 4]),
            Cell {
                glyph: ' ',
                fg: None,
                bg: Some(RED)
            }
        );
    }

    #[test]
    fn two_colours_split_where_they_are_and_three_or_four_merge_the_nearest() {
        let blue = (0, 0, 255);
        let c = cell([Some(RED), Some(RED), Some(blue), Some(blue)]);
        assert_eq!(
            c,
            Cell {
                glyph: '▀',
                fg: Some(RED),
                bg: Some(blue)
            }
        );
        let c = cell([Some(RED), Some(blue), Some(RED), Some(blue)]);
        assert_eq!((c.glyph, c.fg, c.bg), ('▌', Some(RED), Some(blue)));
        // A red, a nearly red and two blues: the reds go together.
        let near = (250, 10, 0);
        let c = cell([Some(RED), Some(near), Some(blue), Some(blue)]);
        assert_eq!((c.glyph, c.bg), ('▀', Some(blue)));
        assert_eq!(c.fg, Some(mean(&[RED, near])));
        // Three opaque pixels and a transparent one: one group, the transparent one the terminal's own.
        let c = cell([Some(RED), Some(blue), None, Some(blue)]);
        assert_eq!((c.glyph, c.bg), ('▜', None));
    }

    #[test]
    fn a_glow_surrounds_the_flame_runs_out_and_never_covers_it() {
        for design in Design::ALL {
            for size in [Size::Large, Size::Small] {
                for mood in [Mood::Resting, Mood::Working, Mood::Waiting, Mood::Done] {
                    let (width, hard) = pixels(design, size, mood, 4, false, None);
                    let (_, glowing) = pixels(design, size, mood, 4, false, Some(DARK));
                    assert!(
                        painted(&glowing) > painted(&hard),
                        "{design:?} {size:?} {mood:?}: no glow"
                    );
                    let height = hard.len();
                    for (y, (a, b)) in hard.iter().zip(&glowing).enumerate() {
                        for (x, (drawn, glow)) in a.iter().zip(b).enumerate() {
                            // What is drawn stays as it is; the glow is only where nothing is.
                            if drawn.is_some() {
                                assert_eq!(drawn, glow, "{design:?} {size:?} {mood:?} {x},{y}");
                            }
                            // Never at the raster's edge: it runs out before.
                            let edge = x == 0 || y == 0 || x == width - 1 || y == height - 1;
                            if edge && drawn.is_none() {
                                assert!(glow.is_none(), "{design:?} {size:?} {mood:?} {x},{y}");
                            }
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn a_failed_glimm_and_the_mini_one_have_no_glow_and_an_unknown_background_none() {
        for design in Design::ALL {
            let (_, hard) = pixels(design, Size::Large, Mood::Failed, 0, false, None);
            let (_, same) = pixels(design, Size::Large, Mood::Failed, 0, false, Some(DARK));
            assert_eq!(hard, same, "{design:?}: a failed Glimm glows");
            let (_, hard) = pixels(design, Size::Mini, Mood::Working, 0, false, None);
            let (_, same) = pixels(design, Size::Mini, Mood::Working, 0, false, Some(DARK));
            assert_eq!(hard, same, "{design:?}: the mini flame glows");
        }
    }

    #[test]
    fn a_glow_takes_the_colour_of_the_terminal_it_runs_out_into() {
        let at = |background| {
            let (_, grid) = pixels(
                Design::A,
                Size::Large,
                Mood::Working,
                0,
                false,
                Some(background),
            );
            grid
        };
        let (dark, light) = (at(DARK), at(LIGHT));
        // Where the glow is, the colour differs by the background; the flame's own pixels do not.
        let mut glow_differs = false;
        for (a, b) in dark.iter().flatten().zip(light.iter().flatten()) {
            if a != b {
                glow_differs = true;
            }
        }
        assert!(glow_differs);
        let (_, hard) = pixels(Design::A, Size::Large, Mood::Working, 0, false, None);
        for (flame, dark) in hard.iter().flatten().zip(dark.iter().flatten()) {
            if flame.is_some() {
                assert_eq!(flame, dark);
            }
        }
    }

    #[test]
    fn a_resting_glimm_is_drawn_in_quadrants_in_the_cells() {
        let first = draw(Design::A, Size::Large, Mood::Resting, 5, false, None);
        let glyphs: String = first
            .iter()
            .flat_map(|line| line.spans.iter().map(|s| s.content.to_string()))
            .collect();
        assert!(glyphs.chars().any(|c| "▘▝▖▗▚▞▛▙▜▟".contains(c)), "{glyphs}");
        // The terminal's own background shows around the flame.
        assert!(glyphs.contains(' '));
    }
}
