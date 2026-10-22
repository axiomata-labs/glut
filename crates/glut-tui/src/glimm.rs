//! Glimm, Glut's mascot (T27): a small flame with eyes, in true colour, drawn in half blocks — each terminal cell two
//! pixels, the upper one the `▀`'s colour, the lower one its background. The owner chose design A of three
//! (2026-10-22; B "in the hex" and C "round" are kept in `docs/glimm.md`). Five moods: resting, working (the tip
//! flickers, sparks rise), waiting for the owner (wide eyes and a "!"), done (pleased eyes and a twinkle) and failed (out
//! and grey, only the ember below still glowing, smoke rising).

use ratatui::style::{Color, Style};
use ratatui::text::{Line, Span};

/// What Glimm shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mood {
    Resting,
    Working,
    Waiting,
    Done,
    Failed,
}

/// Which Glimm: the large one of the welcome (12 × 14 pixels, 12 columns by 7 rows) or the small one beside a question
/// (8 × 8, 8 columns by 4 rows).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Size {
    Large,
    Small,
}

/// One drawing of Glimm: its pixels as letters of [`color`], its eyes' pixels, and where its extras go.
struct Design {
    rows: &'static [&'static str],
    eyes: &'static [(usize, usize)],
    /// How many of the top rows lean while the flame flickers.
    tip_rows: usize,
    /// Where sparks rise while it works, in turn.
    sparks: &'static [(usize, usize)],
    /// The "!" while it waits: its column, the rows of its stroke, and its dot.
    bang: (usize, &'static [usize], usize),
    /// The twinkle when it is done.
    twinkle: (usize, usize),
    /// Where smoke drifts when it failed, frame by frame.
    smoke: &'static [&'static [(usize, usize)]],
}

const LARGE: Design = Design {
    rows: &[
        ".....y......",
        ".....ya.....",
        "....roa.....",
        "....roar....",
        "...rooaor...",
        "...oaaaaor..",
        "..roayyaaor.",
        "..oaayyyaao.",
        "..oayyyyyao.",
        "..oaywwwyao.",
        "..raywwwyar.",
        "...raayaar..",
        "....rrrrr...",
        "............",
    ],
    eyes: &[(7, 4), (7, 8)],
    tip_rows: 3,
    sparks: &[(0, 2), (1, 9), (0, 8), (2, 1)],
    bang: (10, &[0, 1, 2], 4),
    twinkle: (1, 2),
    smoke: &[&[(2, 5), (1, 6)], &[(1, 5), (0, 6)], &[(2, 6), (0, 5)]],
};

const SMALL: Design = Design {
    rows: &[
        "...yy...", "...oa...", "..roar..", ".roaaor.", ".oayyao.", ".oaywao.", ".raywar.",
        "..rrrr..",
    ],
    eyes: &[(4, 2), (4, 5)],
    tip_rows: 2,
    sparks: &[(0, 1), (0, 6), (1, 7), (1, 0)],
    bang: (7, &[0, 1], 3),
    twinkle: (0, 1),
    smoke: &[&[(1, 3)], &[(0, 4)], &[(1, 4)]],
};

/// How the resting flame's tip leans, step by step (columns, negative to the left): mostly still, a sway each way.
const BREATH: [isize; 12] = [0, 0, 0, 1, 1, 0, 0, 0, 0, -1, -1, 0];
/// Ticks per step of the breath — at the screen's 120 ms tick a breath takes about four seconds.
const BREATH_STEP: usize = 3;
/// A blink every this many ticks (about five seconds), with the eyes shut this many.
const BLINK_EVERY: usize = 42;
const BLINK_TICKS: usize = 2;

/// A pixel's colour by its letter: the flame from deep red at its edge to white-hot in its heart, the eyes, the ember
/// and the smoke. `.` is no pixel.
fn color(pixel: char, ash: bool) -> Option<Color> {
    let rgb = |r, g, b| Some(Color::Rgb(r, g, b));
    match (pixel, ash) {
        ('.', _) => None,
        ('R', false) => rgb(0x6e, 0x1a, 0x0c),
        ('r', false) => rgb(0xb8, 0x36, 0x0f),
        ('o', false) => rgb(0xff, 0x7a, 0x1a),
        ('a', false) => rgb(0xff, 0xa0, 0x40),
        ('y', false) => rgb(0xff, 0xd3, 0x6b),
        ('w', false) => rgb(0xff, 0xf2, 0xcc),
        // Gone out: the flame in greys, darkest at the edge.
        ('R', true) => rgb(0x2e, 0x2e, 0x33),
        ('r', true) => rgb(0x45, 0x45, 0x4c),
        ('o', true) => rgb(0x5c, 0x5c, 0x64),
        ('a', true) => rgb(0x6e, 0x6e, 0x76),
        ('y', true) => rgb(0x83, 0x83, 0x8b),
        ('w', true) => rgb(0x9a, 0x9a, 0xa2),
        ('e', _) => rgb(0x1a, 0x0f, 0x0c),
        ('h', _) => rgb(0xff, 0xf7, 0xe6),
        ('m', _) => rgb(0x2a, 0x14, 0x10),
        ('E', _) => rgb(0xd9, 0x48, 0x0f),
        ('s', _) => rgb(0x80, 0x80, 0x8c),
        _ => None,
    }
}

/// Sets the pixel at `row`, `col` where there is one.
fn set(grid: &mut [Vec<char>], row: usize, col: usize, pixel: char) {
    if let Some(at) = grid.get_mut(row).and_then(|line| line.get_mut(col)) {
        *at = pixel;
    }
}

/// Sets the pixel at `row`, `col` only where the flame is not: sparks and marks around it.
fn set_free(grid: &mut [Vec<char>], row: usize, col: usize, pixel: char) {
    if let Some(at) = grid.get_mut(row).and_then(|line| line.get_mut(col))
        && *at == '.'
    {
        *at = pixel;
    }
}

/// The top `rows` lean `by` columns (negative: left) — the flame's tip moving, the body still.
fn lean(grid: &mut [Vec<char>], rows: usize, by: isize) {
    for line in grid.iter_mut().take(rows) {
        let before = line.clone();
        for (col, pixel) in line.iter_mut().enumerate() {
            *pixel = col
                .checked_add_signed(-by)
                .and_then(|from| before.get(from))
                .copied()
                .unwrap_or('.');
        }
    }
}

/// Glimm's pixels in `mood` at animation step `tick`: rows of letters for [`color`], and whether it has gone out.
fn pixels(size: Size, mood: Mood, tick: usize) -> (Vec<Vec<char>>, bool) {
    let design = match size {
        Size::Large => &LARGE,
        Size::Small => &SMALL,
    };
    let mut grid: Vec<Vec<char>> = design
        .rows
        .iter()
        .map(|row| row.chars().collect())
        .collect();
    let height = grid.len();
    let width = grid[0].len();
    let mut ash = false;
    match mood {
        Mood::Resting => {
            // A slow breath: the tip leans one way, rests, leans the other — and now and then Glimm blinks.
            lean(
                &mut grid,
                design.tip_rows,
                BREATH[tick / BREATH_STEP % BREATH.len()],
            );
            if tick % BLINK_EVERY >= BLINK_TICKS {
                for &(row, col) in design.eyes {
                    set(&mut grid, row, col, 'e');
                }
            }
        }
        Mood::Working => {
            lean(&mut grid, design.tip_rows, [-1, 0, 1, 0][tick % 4]);
            for &(row, col) in design.eyes {
                set(&mut grid, row, col, 'e');
            }
            let (row, col) = design.sparks[tick % design.sparks.len()];
            set_free(&mut grid, row, col, 'y');
            let (row, col) = design.sparks[(tick + 2) % design.sparks.len()];
            set_free(&mut grid, row, col, 'a');
        }
        Mood::Waiting => {
            // Wide eyes, looking up at the owner.
            for &(row, col) in design.eyes {
                set(&mut grid, row, col, 'e');
                set(&mut grid, row - 1, col, 'h');
            }
            let (col, stroke, dot) = design.bang;
            // The "!" blinks: three steps on, one off.
            if tick % 4 < 3 {
                for &row in stroke {
                    set_free(&mut grid, row, col, 'y');
                }
                set_free(&mut grid, dot, col, 'y');
            }
        }
        Mood::Done => {
            // Pleased eyes, a small "^" each.
            for &(row, col) in design.eyes {
                set(&mut grid, row, col - 1, 'm');
                set(&mut grid, row - 1, col, 'm');
                set(&mut grid, row, col + 1, 'm');
            }
            if tick % 6 < 3 {
                let (row, col) = design.twinkle;
                set_free(&mut grid, row, col, 'w');
            }
        }
        Mood::Failed => {
            ash = true;
            for line in grid.iter_mut().take(design.tip_rows) {
                line.fill('.');
            }
            // The ember at the bottom still glows, every other pixel of its middle.
            for (row, line) in grid.iter_mut().enumerate().skip(height.saturating_sub(4)) {
                for (col, pixel) in line.iter_mut().enumerate() {
                    let middle = col >= width / 3 && col < width - width / 3;
                    if middle && *pixel != '.' && (row + col) % 2 == 0 {
                        *pixel = 'E';
                    }
                }
            }
            // Drooping eyes, a short line each.
            for &(row, col) in design.eyes {
                set(&mut grid, row, col, 'm');
                set(&mut grid, row, col + 1, 'm');
            }
            for &(row, col) in design.smoke[tick % design.smoke.len()] {
                set_free(&mut grid, row, col, 's');
            }
        }
    }
    (grid, ash)
}

/// Glimm as lines of half blocks: two pixel rows to a terminal row, the upper one the `▀`'s colour, the lower one its
/// background — where only one of them is there, the other half stays the terminal's own.
pub fn lines(size: Size, mood: Mood, tick: usize) -> Vec<Line<'static>> {
    let (grid, ash) = pixels(size, mood, tick);
    grid.chunks(2)
        .map(|pair| {
            let spans: Vec<Span<'static>> = pair[0]
                .iter()
                .enumerate()
                .map(|(col, &top)| {
                    let bottom = pair.get(1).map_or('.', |lower| lower[col]);
                    match (color(top, ash), color(bottom, ash)) {
                        (Some(top), Some(bottom)) => {
                            Span::styled("▀", Style::default().fg(top).bg(bottom))
                        }
                        (Some(top), None) => Span::styled("▀", Style::default().fg(top)),
                        (None, Some(bottom)) => Span::styled("▄", Style::default().fg(bottom)),
                        (None, None) => Span::raw(" "),
                    }
                })
                .collect();
            Line::from(spans)
        })
        .collect()
}

/// How many terminal columns and rows a Glimm of `size` takes.
pub fn cells(size: Size) -> (u16, u16) {
    let design = match size {
        Size::Large => &LARGE,
        Size::Small => &SMALL,
    };
    (
        design.rows[0].len() as u16,
        design.rows.len().div_ceil(2) as u16,
    )
}

/// A one-cell flame for the header, flickering while Glut works.
pub fn spark(working: bool, frame: usize) -> &'static str {
    const SPARKS: [&str; 4] = ["▴", "▵", "▲", "▵"];
    if working {
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

    #[test]
    fn every_mood_and_frame_keeps_glimms_size() {
        for size in [Size::Large, Size::Small] {
            let (cols, rows) = cells(size);
            for mood in MOODS {
                for tick in 0..12 {
                    let drawn = lines(size, mood, tick);
                    assert_eq!(drawn.len(), usize::from(rows), "{size:?} {mood:?}");
                    for line in drawn {
                        assert_eq!(line.width(), usize::from(cols), "{size:?} {mood:?} {tick}");
                    }
                }
            }
        }
        assert_eq!(cells(Size::Large), (12, 7));
        assert_eq!(cells(Size::Small), (8, 4));
    }

    #[test]
    fn every_mood_looks_different_and_a_failed_glimm_has_gone_grey_but_its_ember() {
        for size in [Size::Large, Size::Small] {
            let looks: Vec<Vec<Vec<char>>> =
                MOODS.iter().map(|&mood| pixels(size, mood, 0).0).collect();
            for (at, look) in looks.iter().enumerate() {
                for other in &looks[at + 1..] {
                    assert_ne!(look, other, "{size:?}: two moods look alike");
                }
            }
            let (failed, ash) = pixels(size, Mood::Failed, 0);
            assert!(ash);
            assert!(
                failed.iter().flatten().any(|&pixel| pixel == 'E'),
                "{size:?}: the ember still glows"
            );
        }
    }

    #[test]
    fn a_resting_glimm_breathes_and_blinks_now_and_then() {
        let eyes_open = |tick| {
            let (grid, _) = pixels(Size::Large, Mood::Resting, tick);
            LARGE.eyes.iter().all(|&(row, col)| grid[row][col] == 'e')
        };
        assert!(
            !eyes_open(0),
            "the first ticks of a blink cycle are a blink"
        );
        assert!(eyes_open(BLINK_TICKS));
        assert!(eyes_open(BLINK_EVERY - 1));
        let tip = |tick| pixels(Size::Large, Mood::Resting, tick).0[0].clone();
        assert_ne!(
            tip(3 * BREATH_STEP),
            tip(9 * BREATH_STEP),
            "the tip sways both ways"
        );
    }

    #[test]
    fn half_blocks_carry_two_pixels_a_cell() {
        let first = &lines(Size::Large, Mood::Resting, 0)[0];
        // The tip's single pixel sits in the upper half of its cell, the rest is the terminal's own.
        assert!(first.spans.iter().any(|span| span.content == "▀"));
        assert!(first.spans.iter().any(|span| span.content == " "));
    }
}
