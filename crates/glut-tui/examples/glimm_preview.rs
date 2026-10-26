//! Preview for Glimm's finer drawing (T72, `docs/plans/glimm-fine.md`, "Preview (step 1)"): reads a design file of
//! pixel frames of any even size (its first line that is no comment says `size <W> <H>`; 32 × 20 is 16 × 10 cells)
//! and prints them in the terminal as Unicode quadrant blocks (each cell 2 × 2 pixels), in true colour, with the glow,
//! the heart's pulse and the ash greys of a failed Glimm. Self-contained on purpose (`glimm.rs` is a private module of
//! the binary): the colours are copied from its `color()`.
//!
//! ```text
//! cargo run -p glut-tui --example glimm_preview -- <design.txt|--demo> [--frames rest,work] [--bg RRGGBB|none]
//!     [--glow 0.18] [--compare] [--paint] [--animate] [--fps 8]
//! ```

use std::io::Write;
use std::time::{Duration, Instant};

/// The built-in demo's canvas in pixels.
const DEMO_W: usize = 32;
const DEMO_H: usize = 20;

/// The default of `--glow`.
const DEFAULT_GLOW: f32 = 0.18;

/// A colour, red green blue.
type Rgb = (u8, u8, u8);

/// The glow's colour (`r`, the flame's second edge colour).
const GLOW_COLOR: Rgb = (0xb8, 0x36, 0x0f);

/// The glyph of a foreground mask (bit 0 upper left, 1 upper right, 2 lower left, 3 lower right).
const GLYPHS: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

/// The letters a design may use.
const LETTERS: &str = ".Rroaywehm Es";

/// A pixel's colour by its letter, as `color()` of `glimm.rs`: the flame from deep red at its edge to white-hot in its
/// heart, the eyes, the ember and the smoke; `ash` is a failed Glimm (the flame in greys, the ember stays orange).
fn color(pixel: char, ash: bool) -> Option<Rgb> {
    let rgb = |r, g, b| Some((r, g, b));
    match (pixel, ash) {
        ('.', _) => None,
        ('R', false) => rgb(0x6e, 0x1a, 0x0c),
        ('r', false) => rgb(0xb8, 0x36, 0x0f),
        ('o', false) => rgb(0xff, 0x7a, 0x1a),
        ('a', false) => rgb(0xff, 0xa0, 0x40),
        ('y', false) => rgb(0xff, 0xd3, 0x6b),
        ('w', false) => rgb(0xff, 0xf2, 0xcc),
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

// ---------------------------------------------------------------------------------------------------------------
// The design file
// ---------------------------------------------------------------------------------------------------------------

/// One frame of a design file.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Frame {
    name: String,
    /// The line of its `@` header.
    line: usize,
    /// Canvas width and height in pixels (from the file's `size` line; even).
    w: usize,
    h: usize,
    rows: Vec<Vec<char>>,
}

impl Frame {
    /// Cells across and down (a cell is 2 × 2 pixels).
    fn cells_w(&self) -> usize {
        self.w / 2
    }

    fn cells_h(&self) -> usize {
        self.h / 2
    }
}

/// The `size <W> <H>` line: both numbers even and at least 2.
fn parse_size(line: &str, file: &str, n: usize) -> Result<(usize, usize), String> {
    let usage = || format!("{file}:{n}: expected `size <W> <H>` (both even, at least 2)");
    let mut words = line.split_whitespace();
    if words.next() != Some("size") {
        return Err(usage());
    }
    let (Some(w), Some(h), None) = (words.next(), words.next(), words.next()) else {
        return Err(usage());
    };
    let (Ok(w), Ok(h)) = (w.parse::<usize>(), h.parse::<usize>()) else {
        return Err(usage());
    };
    if w < 2 || h < 2 || w % 2 != 0 || h % 2 != 0 {
        return Err(format!(
            "{file}:{n}: size {w} {h} must be even and at least 2"
        ));
    }
    Ok((w, h))
}

/// Parses a design file strictly; every problem is reported as `file:line: message`. Its first line that is no comment
/// and not blank is `size <W> <H>`; every frame then has H rows of W letters.
fn parse(text: &str, file: &str) -> Result<Vec<Frame>, Vec<String>> {
    let mut size: Option<(usize, usize)> = None;
    let mut frames: Vec<Frame> = Vec::new();
    let mut errors = Vec::new();
    let mut cur: Option<Frame> = None;
    // A frame left open too early is a wrong-height frame.
    let short = |frame: &Frame, errors: &mut Vec<String>| {
        errors.push(format!(
            "{file}:{}: frame '{}' has {} rows, expected {}",
            frame.line,
            frame.name,
            frame.rows.len(),
            frame.h
        ));
    };
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim_end_matches('\r');
        if line.starts_with('#') {
            continue;
        }
        let Some((w, h)) = size else {
            if line.trim().is_empty() {
                continue;
            }
            if !line.starts_with("size") {
                return Err(vec![format!(
                    "{file}:{n}: the file has no `size <W> <H>` line before its first frame"
                )]);
            }
            size = Some(parse_size(line, file, n).map_err(|e| vec![e])?);
            continue;
        };
        if let Some(rest) = line.strip_prefix('@') {
            if let Some(frame) = cur.take() {
                short(&frame, &mut errors);
            }
            let name = rest.trim();
            if name.is_empty() || name.contains(char::is_whitespace) {
                errors.push(format!(
                    "{file}:{n}: a frame needs a one-word name (`@ name`)"
                ));
            } else if frames.iter().any(|f| f.name == name) {
                errors.push(format!("{file}:{n}: frame '{name}' is there twice"));
            }
            cur = Some(Frame {
                name: name.to_string(),
                line: n,
                w,
                h,
                rows: Vec::new(),
            });
        } else if line.trim().is_empty() {
            if let Some(frame) = cur.take() {
                short(&frame, &mut errors);
            }
        } else if let Some(frame) = cur.as_mut() {
            let chars: Vec<char> = line.chars().collect();
            if chars.len() != w {
                errors.push(format!(
                    "{file}:{n}: row {} of frame '{}' has {} characters, expected {w}",
                    frame.rows.len() + 1,
                    frame.name,
                    chars.len()
                ));
            }
            for (col, c) in chars.iter().enumerate() {
                if *c == ' ' || !LETTERS.contains(*c) {
                    errors.push(format!("{file}:{n}:{}: unknown letter '{c}'", col + 1));
                }
            }
            frame.rows.push(chars);
            if frame.rows.len() == h {
                frames.extend(cur.take());
            }
        } else {
            errors.push(format!(
                "{file}:{n}: a line outside a frame (a frame has exactly {h} rows)"
            ));
        }
    }
    if let Some(frame) = cur.take() {
        short(&frame, &mut errors);
    }
    let Some((w, h)) = size else {
        return Err(vec![format!(
            "{file}: the file has no `size <W> <H>` line (it comes first, after comments)"
        )]);
    };
    if frames.is_empty() && errors.is_empty() {
        errors.push(format!(
            "{file}: no frames (`@ name` and {h} rows of {w} letters)"
        ));
    }
    if errors.is_empty() {
        Ok(frames)
    } else {
        Err(errors)
    }
}

// ---------------------------------------------------------------------------------------------------------------
// The encoder
// ---------------------------------------------------------------------------------------------------------------

/// One terminal cell: a glyph, its foreground and its background (`None` is the terminal's own).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Cell {
    glyph: char,
    fg: Option<Rgb>,
    bg: Option<Rgb>,
}

/// The mean of some colours.
fn mean(colors: &[Rgb]) -> Rgb {
    let n = colors.len().max(1) as f32;
    let sum =
        |f: fn(&Rgb) -> u8| (colors.iter().map(|c| f(c) as f32).sum::<f32>() / n).round() as u8;
    (sum(|c| c.0), sum(|c| c.1), sum(|c| c.2))
}

/// The squared error of a group of colours against its mean.
fn error(colors: &[Rgb]) -> f32 {
    let m = mean(colors);
    let d = |a: u8, b: u8| (a as f32 - b as f32).powi(2);
    colors
        .iter()
        .map(|c| d(c.0, m.0) + d(c.1, m.1) + d(c.2, m.2))
        .sum()
}

/// A 2 × 2 block (upper left, upper right, lower left, lower right; `None` is transparent) as one cell. Opaque pixels
/// are split into two groups of least colour error (all 8 partitions tried), the foreground's mask picks the glyph. A
/// block with transparent pixels has them as its background — the terminal's own, never mixed with a colour — and the
/// opaque ones as one group.
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
    // Four colours: group A is the pixels of `mask` (the pixel at bit 3 stays in B), so 8 partitions.
    let mut best = (f32::MAX, 0usize);
    for mask in 0..8usize {
        let e = error(&pick(mask)) + error(&pick(15 & !mask));
        if e < best.0 {
            best = (e, mask);
        }
    }
    let mask = best.1;
    if mask == 0 {
        // One colour: a space on a background of it (no font gap).
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

/// A cell as escape sequences and its glyph, ending without a reset (the line adds it).
fn push_cell(out: &mut String, c: Cell) {
    if c.glyph == ' ' && c.fg.is_none() && c.bg.is_none() {
        out.push_str("\x1b[0m ");
        return;
    }
    if let Some((r, g, b)) = c.fg {
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m"));
    }
    match c.bg {
        Some((r, g, b)) => out.push_str(&format!("\x1b[48;2;{r};{g};{b}m")),
        None => out.push_str("\x1b[49m"),
    }
    out.push(c.glyph);
}

// ---------------------------------------------------------------------------------------------------------------
// Glow, pulse
// ---------------------------------------------------------------------------------------------------------------

/// The glow's strength per frame name (first prefix that fits): all the per-mood choices live here.
const GLOW_SCALE: [(&str, f32); 7] = [
    ("fail", 0.0),
    ("work", 1.4),
    ("wait0", 1.2),
    ("wait1", 0.8),
    ("wait", 1.0),
    ("rest", 0.6),
    ("done", 1.0),
];

/// The pulse's period in seconds by frame name; `None` is no pulse.
fn pulse_period(name: &str) -> Option<f32> {
    if name.starts_with("fail") {
        None
    } else if name.starts_with("work") {
        Some(1.5)
    } else {
        Some(3.0)
    }
}

/// The heart's mix between `y` (0) and `w` (1): static (`time` is `None`) it is 1.
fn pulse(name: &str, time: Option<f32>) -> f32 {
    match (time, pulse_period(name)) {
        (Some(t), Some(period)) => 0.5 + 0.5 * (std::f32::consts::TAU * t / period).sin(),
        _ => 1.0,
    }
}

/// The glow's alpha at a real distance (in pixels measured sideways; a pixel is 1 wide, 2 tall): the rings 0.5 / 0.28 /
/// 0.12 at 2 / 4 / 6, joined by straight lines and gone at 7.5.
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

/// Whether a letter is the flame's body (the glow's source), not an eye or a spark.
fn is_flame(c: char) -> bool {
    "Rroayw".contains(c)
}

/// The glow's alpha for every pixel, from the flame's mask: the distance field in real size (dy counts twice).
fn glow_field(rows: &[Vec<char>]) -> Vec<Vec<f32>> {
    let h = rows.len();
    let w = rows.first().map_or(0, Vec::len);
    let flame: Vec<(f32, f32)> = (0..h)
        .flat_map(|y| (0..w).map(move |x| (x, y)))
        .filter(|&(x, y)| is_flame(rows[y][x]))
        .map(|(x, y)| (x as f32, y as f32 * 2.0))
        .collect();
    (0..h)
        .map(|y| {
            (0..w)
                .map(|x| {
                    if flame.is_empty() {
                        return 0.0;
                    }
                    let (px, py) = (x as f32, y as f32 * 2.0);
                    let d2 = flame
                        .iter()
                        .map(|&(fx, fy)| (fx - px).powi(2) + (fy - py).powi(2))
                        .fold(f32::MAX, f32::min);
                    alpha_at(d2.sqrt())
                })
                .collect()
        })
        .collect()
}

/// `over` blended on `under` by `alpha`.
fn blend(over: Rgb, under: Rgb, alpha: f32) -> Rgb {
    let m = |o: u8, u: u8| {
        (u as f32 + (o as f32 - u as f32) * alpha)
            .round()
            .clamp(0.0, 255.0) as u8
    };
    (m(over.0, under.0), m(over.1, under.1), m(over.2, under.2))
}

/// How a frame is drawn.
#[derive(Debug, Clone, Copy)]
struct Look {
    /// The terminal's background; `None` is unknown: no glow.
    bg: Option<Rgb>,
    /// The `--glow` option, 0 to 2.
    glow: f32,
    /// Paint the background into the transparent cells (to show a light terminal on a dark one).
    paint: bool,
}

/// The pixels of a frame: letters to colours with the pulse, then the glow into the transparent ones.
fn pixels(frame: &Frame, look: Look, time: Option<f32>) -> Vec<Vec<Option<Rgb>>> {
    let ash = frame.name.starts_with("fail");
    let t = pulse(&frame.name, time);
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
    let scale = GLOW_SCALE
        .iter()
        .find(|(p, _)| frame.name.starts_with(p))
        .map_or(1.0, |&(_, s)| s);
    // The glow breathes a little with the heart.
    let strength = look.glow.clamp(0.0, 2.0) * scale * (0.85 + 0.15 * t);
    if let (Some(bg), true) = (look.bg, strength > 0.0) {
        let field = glow_field(&frame.rows);
        for (row, alphas) in grid.iter_mut().zip(&field) {
            for (pixel, alpha) in row.iter_mut().zip(alphas) {
                let alpha = (alpha * strength).min(0.9);
                if pixel.is_none() && alpha >= 0.02 {
                    *pixel = Some(blend(GLOW_COLOR, bg, alpha));
                }
            }
        }
    }
    grid
}

/// A frame as its lines of cells, each ended with a reset.
fn render(frame: &Frame, look: Look, time: Option<f32>) -> Vec<String> {
    let grid = pixels(frame, look, time);
    (0..frame.cells_h())
        .map(|cy| {
            let mut line = String::new();
            for cx in 0..frame.cells_w() {
                let (x, y) = (cx * 2, cy * 2);
                let mut c = cell([
                    grid[y][x],
                    grid[y][x + 1],
                    grid[y + 1][x],
                    grid[y + 1][x + 1],
                ]);
                if let (true, Some(bg)) = (look.paint, look.bg) {
                    c.bg = c.bg.or(Some(bg));
                }
                push_cell(&mut line, c);
            }
            line.push_str("\x1b[0m");
            line
        })
        .collect()
}

// ---------------------------------------------------------------------------------------------------------------
// Demo design
// ---------------------------------------------------------------------------------------------------------------

/// A flame drawn by code: a drop, 24 px wide and 16 tall, with 2 × 2 eyes; `shift` moves it sideways, `extras` adds
/// pixels (row, column, letter).
fn demo_frame(name: &str, shift: isize, extras: &[(usize, usize, char)]) -> String {
    let mut g = vec![vec!['.'; DEMO_W]; DEMO_H];
    let at = |col: usize| (col as isize + shift) as usize;
    for y in 0..16usize {
        let t = (y as f32 + 0.5) / 16.0;
        let half = if t < 0.6 {
            12.0 * (t / 0.6).powf(0.8)
        } else {
            12.0 * (1.0 - ((t - 0.6) / 0.4).powi(2)).max(0.0).sqrt()
        };
        for x in 0..DEMO_W {
            let dx = (x as f32 - 15.5).abs();
            if dx > half {
                continue;
            }
            let edge = (half - dx).min(y as f32 + 0.5).min(15.5 - y as f32);
            g[y + 2][at(x)] = match edge {
                e if e < 0.6 => 'R',
                e if e < 1.6 => 'r',
                e if e < 3.5 => 'o',
                e if e < 5.5 => 'a',
                e if e < 8.0 => 'y',
                _ => 'w',
            };
        }
    }
    // Eyes of 2 × 2 with a highlight at the upper left, mirrored.
    for (row, col, letter) in [(11, 11, 'h'), (11, 12, 'e'), (12, 11, 'e'), (12, 12, 'e')] {
        g[row][at(col)] = letter;
        g[row][at(31 - col)] = letter;
    }
    for &(row, col, letter) in extras {
        g[row][col] = letter;
    }
    let rows: Vec<String> = g.iter().map(|r| r.iter().collect()).collect();
    format!("@ {name}\n{}\n\n", rows.join("\n"))
}

/// The built-in design: four frames to see the encoder, the glow per name and the ash without the other files.
fn demo_text() -> String {
    [
        format!("# built-in demo: a drop with eyes\nsize {DEMO_W} {DEMO_H}\n\n"),
        demo_frame("rest0", 0, &[]),
        demo_frame("rest1", 1, &[]),
        demo_frame("work0", 0, &[(4, 26, 'E'), (7, 4, 'E')]),
        demo_frame("fail0", 0, &[(18, 15, 'E'), (18, 16, 'E'), (0, 15, 's')]),
    ]
    .concat()
}

// ---------------------------------------------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------------------------------------------

/// The frames side by side as wide as `columns` allows, each with its name above.
fn grid_lines(frames: &[&Frame], look: Look, columns: usize) -> Vec<String> {
    const GAP: usize = 2;
    let gap = " ".repeat(GAP);
    let cells_w = frames.iter().map(|f| f.cells_w()).max().unwrap_or(1);
    let per_row = ((columns + GAP) / (cells_w + GAP)).max(1);
    let mut out = Vec::new();
    for chunk in frames.chunks(per_row) {
        let names: Vec<String> = chunk
            .iter()
            .map(|f| format!("{:<width$}", f.name, width = f.cells_w()))
            .collect();
        out.push(names.join(&gap));
        let rendered: Vec<Vec<String>> = chunk.iter().map(|f| render(f, look, None)).collect();
        for row in 0..chunk.iter().map(|f| f.cells_h()).max().unwrap_or(0) {
            out.push(
                rendered
                    .iter()
                    .zip(chunk)
                    .map(|(r, f)| r.get(row).map_or(" ".repeat(f.cells_w()), |l| l.clone()))
                    .collect::<Vec<_>>()
                    .join(&gap),
            );
        }
        out.push(String::new());
    }
    out
}

/// The command line.
#[derive(Debug)]
struct Args {
    path: Option<String>,
    demo: bool,
    frames: Vec<String>,
    bg: Option<Rgb>,
    glow: f32,
    compare: bool,
    paint: bool,
    animate: bool,
    fps: f32,
}

/// `RRGGBB` (with or without `#`) as a colour.
fn parse_hex(s: &str) -> Option<Rgb> {
    let s = s.trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let p = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some((p(0)?, p(2)?, p(4)?))
}

const USAGE: &str = "usage: glimm_preview <design.txt | --demo> [--frames rest,work] [--bg RRGGBB|none] [--glow 0..2, default 0.18] \
[--compare] [--paint] [--animate] [--fps 8]";

/// Parses the arguments (without the program's name).
fn parse_args(args: &[String]) -> Result<Args, String> {
    let mut a = Args {
        path: None,
        demo: false,
        frames: Vec::new(),
        bg: Some((0x1e, 0x1e, 0x2e)),
        glow: DEFAULT_GLOW,
        compare: false,
        paint: false,
        animate: false,
        fps: 8.0,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut value = |name: &str| it.next().cloned().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--demo" => a.demo = true,
            "--compare" => a.compare = true,
            "--paint" => a.paint = true,
            "--animate" => a.animate = true,
            "--frames" => {
                a.frames = value("--frames")?
                    .split(',')
                    .filter(|s| !s.is_empty())
                    .map(String::from)
                    .collect()
            }
            "--bg" => {
                let v = value("--bg")?;
                a.bg = if v == "none" {
                    None
                } else {
                    Some(parse_hex(&v).ok_or(format!("--bg: '{v}' is not RRGGBB or none"))?)
                };
            }
            "--glow" => {
                let v = value("--glow")?;
                a.glow = v
                    .parse::<f32>()
                    .map_err(|_| format!("--glow: '{v}' is not a number"))?
                    .clamp(0.0, 2.0);
            }
            "--fps" => {
                let v = value("--fps")?;
                a.fps = v
                    .parse::<f32>()
                    .ok()
                    .filter(|f| *f > 0.0 && *f <= 60.0)
                    .ok_or(format!("--fps: '{v}' is not 0..60"))?;
            }
            "-h" | "--help" => return Err(USAGE.to_string()),
            s if s.starts_with("--") => return Err(format!("unknown option {s}\n{USAGE}")),
            s => a.path = Some(s.to_string()),
        }
    }
    if a.path.is_none() && !a.demo {
        return Err(USAGE.to_string());
    }
    Ok(a)
}

/// Restores the terminal when the animation ends, by return, error or panic.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let mut out = std::io::stdout();
        let _ = out.write_all(b"\x1b[0m\x1b[?25h");
        let _ = out.flush();
    }
}

/// Plays the frames in a loop, redrawn in place, until Ctrl-C (or `q`, Esc).
fn animate(frames: &[&Frame], look: Look, fps: f32) -> std::io::Result<()> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, poll, read};
    crossterm::terminal::enable_raw_mode()?;
    let _restore = Restore;
    let mut out = std::io::stdout();
    out.write_all(b"\x1b[?25l")?;
    let start = Instant::now();
    let step = Duration::from_secs_f32(1.0 / fps);
    let mut tick = 0usize;
    loop {
        let frame = frames[tick % frames.len()];
        let time = start.elapsed().as_secs_f32();
        let mut buf = String::new();
        if tick > 0 {
            buf.push_str(&format!("\x1b[{}A\r", frame.cells_h() + 1));
        }
        buf.push_str(&format!("\x1b[2K{}  (Ctrl-C ends)\r\n", frame.name));
        for line in render(frame, look, Some(time)) {
            buf.push_str(&format!("\x1b[2K{line}\r\n"));
        }
        out.write_all(buf.as_bytes())?;
        out.flush()?;
        tick += 1;
        let next = start + step.mul_f32(tick as f32);
        while let Some(wait) = next.checked_duration_since(Instant::now()) {
            if poll(wait)?
                && let Event::Key(k) = read()?
                && k.kind != KeyEventKind::Release
                && (matches!(k.code, KeyCode::Esc | KeyCode::Char('q'))
                    || (k.code == KeyCode::Char('c')
                        && k.modifiers.contains(KeyModifiers::CONTROL)))
            {
                return Ok(());
            }
        }
    }
}

fn run() -> Result<(), String> {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let args = parse_args(&argv)?;
    let (text, file) = if args.demo {
        (demo_text(), "<demo>".to_string())
    } else {
        let path = args.path.clone().unwrap_or_default();
        (
            std::fs::read_to_string(&path).map_err(|e| format!("{path}: {e}"))?,
            path,
        )
    };
    let all = parse(&text, &file).map_err(|errors| errors.join("\n"))?;
    let wanted = |f: &Frame| {
        args.frames.is_empty() || args.frames.iter().any(|p| f.name.starts_with(p.as_str()))
    };
    let chosen: Vec<&Frame> = all.iter().filter(|f| wanted(f)).collect();
    if chosen.is_empty() {
        return Err(format!("{file}: no frame matches {:?}", args.frames));
    }
    let look = Look {
        bg: args.bg,
        glow: args.glow,
        paint: args.paint,
    };
    if args.animate {
        // The numbered frames in file order (`rest0..rest3`, not `restblink`), else whatever matched.
        let numbered: Vec<&Frame> = chosen
            .iter()
            .copied()
            .filter(|f| f.name.ends_with(|c: char| c.is_ascii_digit()))
            .collect();
        let seq = if numbered.is_empty() {
            chosen
        } else {
            numbered
        };
        return animate(&seq, look, args.fps).map_err(|e| format!("animation: {e}"));
    }
    let columns = std::env::var("COLUMNS")
        .ok()
        .and_then(|c| c.parse::<usize>().ok())
        .filter(|c| *c > 0)
        .unwrap_or(100);
    let shown = args.bg.map_or("none (no glow)".to_string(), |(r, g, b)| {
        format!("{r:02x}{g:02x}{b:02x}")
    });
    let mut lines = vec![format!("== bg {shown} ==")];
    lines.extend(grid_lines(&chosen, look, columns));
    if args.compare {
        lines.push("== on a light terminal f5f5f5 (painted) ==".to_string());
        lines.extend(grid_lines(
            &chosen,
            Look {
                bg: Some((0xf5, 0xf5, 0xf5)),
                paint: true,
                ..look
            },
            columns,
        ));
        lines.push("== bg none (no glow, hard edge) ==".to_string());
        lines.extend(grid_lines(
            &chosen,
            Look {
                bg: None,
                paint: false,
                ..look
            },
            columns,
        ));
    }
    println!("{}", lines.join("\n"));
    Ok(())
}

fn main() {
    if let Err(message) = run() {
        eprintln!("{message}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RED: Rgb = (255, 0, 0);
    const BLUE: Rgb = (0, 0, 255);
    const GREEN: Rgb = (0, 255, 0);
    const DARK: Rgb = (0x1e, 0x1e, 0x2e);
    const W: usize = 32;
    const H: usize = 20;

    /// A frame of transparent pixels but where `set` says.
    fn frame(name: &str, set: &[(usize, usize, char)]) -> Frame {
        let mut rows = vec![vec!['.'; W]; H];
        for &(y, x, c) in set {
            rows[y][x] = c;
        }
        Frame {
            name: name.to_string(),
            line: 1,
            w: W,
            h: H,
            rows,
        }
    }

    fn painted(px: &[Vec<Option<Rgb>>]) -> usize {
        px.iter().flatten().filter(|p| p.is_some()).count()
    }

    #[test]
    fn every_mask_of_one_colour_has_its_glyph() {
        for mask in 1..15usize {
            let block = std::array::from_fn(|i| (mask >> i & 1 == 1).then_some(RED));
            let c = cell(block);
            assert_eq!(c.glyph, GLYPHS[mask], "mask {mask}");
            assert_eq!((c.fg, c.bg), (Some(RED), None), "mask {mask}");
        }
    }

    #[test]
    fn empty_and_full_blocks() {
        assert_eq!(
            cell([None; 4]),
            Cell {
                glyph: ' ',
                fg: None,
                bg: None
            }
        );
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
    fn two_colours_split_where_they_are() {
        let top = cell([Some(RED), Some(RED), Some(BLUE), Some(BLUE)]);
        assert_eq!(top.glyph, '▀');
        assert_eq!((top.fg, top.bg), (Some(RED), Some(BLUE)));
        let diagonal = cell([Some(RED), Some(BLUE), Some(BLUE), Some(RED)]);
        assert!(matches!(diagonal.glyph, '▚' | '▞'));
        assert!(diagonal.fg != diagonal.bg && diagonal.fg.is_some() && diagonal.bg.is_some());
        let corner = cell([Some(RED), Some(BLUE), Some(BLUE), Some(BLUE)]);
        assert_eq!(
            (corner.glyph, corner.fg, corner.bg),
            ('▘', Some(RED), Some(BLUE))
        );
    }

    #[test]
    fn three_and_four_colours_merge_the_nearest() {
        // Two near reds and two blues: the reds merge into their mean.
        let near = (250, 10, 0);
        let c = cell([Some(RED), Some(near), Some(BLUE), Some(BLUE)]);
        assert_eq!((c.glyph, c.fg, c.bg), ('▀', Some((253, 5, 0)), Some(BLUE)));
        // Red, green and two blues: never more than two colours in a cell.
        let c = cell([Some(RED), Some(GREEN), Some(BLUE), Some(BLUE)]);
        assert!(c.fg.is_some() && c.bg.is_some() && c.fg != c.bg);
        // Three colours and a transparent pixel: one group over the terminal's background.
        let c = cell([Some(RED), Some(GREEN), Some(BLUE), None]);
        assert_eq!((c.glyph, c.bg), ('▛', None));
    }

    #[test]
    fn transparent_is_the_terminals_background() {
        // A transparent pixel is never mixed into a colour: the colours' mean stays the colours' mean.
        let c = cell([Some(RED), Some(BLUE), None, None]);
        assert_eq!((c.glyph, c.fg, c.bg), ('▀', Some((128, 0, 128)), None));
        let c = cell([None, None, None, Some(GREEN)]);
        assert_eq!((c.glyph, c.fg, c.bg), ('▗', Some(GREEN), None));
    }

    #[test]
    fn cells_as_escapes() {
        let mut s = String::new();
        push_cell(
            &mut s,
            Cell {
                glyph: ' ',
                fg: None,
                bg: None,
            },
        );
        assert_eq!(s, "\x1b[0m ");
        let mut s = String::new();
        push_cell(
            &mut s,
            Cell {
                glyph: '▀',
                fg: Some(RED),
                bg: None,
            },
        );
        assert_eq!(s, "\x1b[38;2;255;0;0m\x1b[49m▀");
        let mut s = String::new();
        push_cell(
            &mut s,
            Cell {
                glyph: '▌',
                fg: Some(RED),
                bg: Some(BLUE),
            },
        );
        assert_eq!(s, "\x1b[38;2;255;0;0m\x1b[48;2;0;0;255m▌");
    }

    #[test]
    fn the_parser_takes_a_good_file() {
        let row = ".".repeat(W);
        let one = vec![row.as_str(); H].join("\n");
        let text = format!("# c\n\nsize {W} {H}\n@ rest0\n{one}\n\n@ rest1\n{one}\n");
        let frames = parse(&text, "f.txt").unwrap();
        assert_eq!(
            frames.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
            ["rest0", "rest1"]
        );
        assert!(frames.iter().all(|f| (f.w, f.h) == (W, H)));
    }

    #[test]
    fn the_parser_names_file_and_line() {
        let row = ".".repeat(W);
        let mut rows = vec![row.clone(); H];
        rows[2] = ".".repeat(W - 1);
        rows[4] = format!("{}x", ".".repeat(W - 1));
        let text = format!(
            "size 32 20\n@ a\n{}\n\n@ b\n{}\n",
            rows.join("\n"),
            vec![row.as_str(); H - 1].join("\n")
        );
        let errors = parse(&text, "d.txt").unwrap_err();
        assert!(
            errors.contains(
                &"d.txt:5: row 3 of frame 'a' has 31 characters, expected 32".to_string()
            ),
            "{errors:?}"
        );
        assert!(
            errors.contains(&"d.txt:7:32: unknown letter 'x'".to_string()),
            "{errors:?}"
        );
        assert!(
            errors.contains(&"d.txt:24: frame 'b' has 19 rows, expected 20".to_string()),
            "{errors:?}"
        );
    }

    #[test]
    fn the_parser_refuses_strays_doubles_and_spaces() {
        let errors = parse("size 32 20\n....\n", "x").unwrap_err();
        assert!(
            errors[0].starts_with("x:2: a line outside a frame"),
            "{errors:?}"
        );
        assert!(parse("# nothing\nsize 32 20\n", "x").is_err());
        let row = ".".repeat(W);
        let one = vec![row.as_str(); H].join("\n");
        let errors = parse(&format!("size 32 20\n@ a\n{one}\n@ a\n{one}\n"), "x").unwrap_err();
        assert!(errors[0].contains("there twice"), "{errors:?}");
        let errors = parse(&format!("size 32 20\n@ a\n{one}\n{row}\n"), "x").unwrap_err();
        assert!(errors[0].starts_with("x:23:"), "{errors:?}");
        let spaced = one.replacen('.', " ", 1);
        let errors = parse(&format!("size 32 20\n@ a\n{spaced}\n"), "x").unwrap_err();
        assert_eq!(errors, ["x:3:1: unknown letter ' '"]);
    }

    #[test]
    fn the_size_line_is_required_even_and_rules_the_frames() {
        let one = |w: usize, h: usize| vec![".".repeat(w); h].join("\n");
        // Missing: the error names the file.
        let errors = parse(&format!("@ a\n{}\n", one(32, 20)), "m.txt").unwrap_err();
        assert!(errors[0].starts_with("m.txt:"), "{errors:?}");
        assert!(errors[0].contains("size"), "{errors:?}");
        let errors = parse("# only a comment\n", "m.txt").unwrap_err();
        assert!(
            errors[0].starts_with("m.txt:") && errors[0].contains("no `size"),
            "{errors:?}"
        );
        // Odd, too small, or not numbers.
        for bad in ["size 31 20", "size 32 5", "size 0 4", "size 32", "size a b"] {
            let errors = parse(&format!("{bad}\n@ a\n{}\n", one(32, 20)), "o.txt").unwrap_err();
            assert!(errors[0].starts_with("o.txt:1:"), "{bad}: {errors:?}");
        }
        // A 20 × 12 file parses, with its size in its frames, and renders 10 × 6 cells.
        let text = format!(
            "# c\nsize 20 12\n@ a\n{}\n\n@ b\n{}\n",
            one(20, 12),
            one(20, 12)
        );
        let frames = parse(&text, "s.txt").unwrap();
        assert_eq!(frames.len(), 2);
        assert!(frames.iter().all(|f| (f.w, f.h) == (20, 12)));
        let look = Look {
            bg: Some(DARK),
            glow: DEFAULT_GLOW,
            paint: false,
        };
        assert_eq!(render(&frames[0], look, None).len(), 6);
        assert_eq!(grid_lines(&[&frames[0], &frames[1]], look, 100).len(), 8);
        // A 32 × 20 body in a 20 × 12 file is wrong in height and width.
        let errors = parse(&format!("size 20 12\n@ a\n{}\n", one(32, 20)), "w.txt").unwrap_err();
        assert!(
            errors[0].contains("has 32 characters, expected 20"),
            "{errors:?}"
        );
        let errors = parse(&format!("size 20 12\n@ a\n{}\n", one(20, 11)), "h.txt").unwrap_err();
        assert!(errors[0].contains("has 11 rows, expected 12"), "{errors:?}");
        // A 8 × 4 file with a glow: the field has the frame's size.
        let f = parse(
            &format!("size 8 4\n@ a\n.o......\n........\n........\n........\n"),
            "t",
        )
        .unwrap();
        let field = glow_field(&f[0].rows);
        assert_eq!((field.len(), field[0].len()), (4, 8));
    }

    #[test]
    fn the_glow_defaults_to_018() {
        let a = parse_args(&["--demo".to_string()]).unwrap();
        assert_eq!(a.glow, 0.18);
        assert!(USAGE.contains("0.18"));
    }

    #[test]
    fn the_demo_is_a_good_design() {
        let frames = parse(&demo_text(), "demo").unwrap();
        assert_eq!(frames.len(), 4);
        assert!(frames.iter().all(|f| (f.w, f.h) == (32, 20)));
        for f in &frames {
            let lines = render(
                f,
                Look {
                    bg: Some(DARK),
                    glow: 1.0,
                    paint: false,
                },
                None,
            );
            assert_eq!(lines.len(), 10);
        }
    }

    #[test]
    fn glow_surrounds_the_flame_and_runs_out() {
        let f = frame("rest0", &[(10, 16, 'o'), (10, 17, 'y')]);
        let field = glow_field(&f.rows);
        assert!(field[10][15] > 0.4, "next to the flame");
        assert!(field[10][12] > field[10][8], "falls off sideways");
        assert_eq!(field[10][2], 0.0, "far away");
        // Real size: a pixel is two tall, so one row up is two sideways, two rows up four.
        assert!((field[9][16] - alpha_at(2.0)).abs() < 1e-6);
        assert!((field[8][16] - alpha_at(4.0)).abs() < 1e-6);
    }

    #[test]
    fn eyes_and_sparks_give_no_glow() {
        let f = frame(
            "rest0",
            &[(10, 16, 'e'), (4, 4, 'E'), (6, 6, 's'), (8, 8, 'h')],
        );
        assert!(glow_field(&f.rows).iter().flatten().all(|a| *a == 0.0));
    }

    #[test]
    fn glow_paints_only_transparent_pixels_in_the_background_blend() {
        let f = frame("rest0", &[(10, 16, 'o')]);
        let px = pixels(
            &f,
            Look {
                bg: Some(DARK),
                glow: 1.0,
                paint: false,
            },
            None,
        );
        assert_eq!(px[10][16], color('o', false), "the flame itself stays");
        let glow = px[10][15].expect("a glowing pixel");
        assert!(
            glow.0 > DARK.0 && glow.0 < GLOW_COLOR.0,
            "between the background and the glow colour: {glow:?}"
        );
        assert_eq!(px[0][0], None, "far away stays transparent");
    }

    #[test]
    fn no_background_no_glow_and_failed_no_glow() {
        let f = frame("rest0", &[(10, 16, 'o')]);
        assert_eq!(
            painted(&pixels(
                &f,
                Look {
                    bg: None,
                    glow: 1.0,
                    paint: false
                },
                None
            )),
            1
        );
        let px = pixels(
            &frame("fail0", &[(10, 16, 'o')]),
            Look {
                bg: Some(DARK),
                glow: 1.0,
                paint: false,
            },
            None,
        );
        assert_eq!(painted(&px), 1);
        assert_eq!(px[10][16], color('o', true), "ash greys");
        assert_eq!(
            painted(&pixels(
                &f,
                Look {
                    bg: Some(DARK),
                    glow: 0.0,
                    paint: false
                },
                None
            )),
            1
        );
    }

    #[test]
    fn work_glows_wider_than_rest_and_the_ember_stays_orange() {
        let look = Look {
            bg: Some(DARK),
            glow: 1.0,
            paint: false,
        };
        let count = |name: &str| painted(&pixels(&frame(name, &[(10, 16, 'o')]), look, None));
        assert!(count("work0") > count("rest0"));
        let px = pixels(&frame("fail1", &[(10, 16, 'E')]), look, None);
        assert_eq!(px[10][16], color('E', false));
    }

    #[test]
    fn the_heart_pulses_between_y_and_w() {
        let f = frame("rest0", &[(10, 16, 'w')]);
        let look = Look {
            bg: None,
            glow: 0.0,
            paint: false,
        };
        let (y, w) = (color('y', false), color('w', false));
        assert_eq!(pixels(&f, look, None)[10][16], w, "static: w");
        // sin is -1 at three quarters of the period: y; at a quarter: w.
        assert_eq!(pixels(&f, look, Some(2.25))[10][16], y);
        assert_eq!(pixels(&f, look, Some(0.75))[10][16], w);
        assert!(
            (pulse("work0", Some(0.375)) - 1.0).abs() < 1e-5,
            "work's period is 1.5 s"
        );
        assert_eq!(pulse("fail0", Some(0.1)), 1.0);
    }

    #[test]
    fn painting_fills_the_background_in() {
        let f = frame("rest0", &[(10, 16, 'o')]);
        let lines = render(
            &f,
            Look {
                bg: Some((0xf5, 0xf5, 0xf5)),
                glow: 0.0,
                paint: true,
            },
            None,
        );
        assert!(lines[0].contains("\x1b[48;2;245;245;245m"));
        let bare = render(
            &f,
            Look {
                bg: None,
                glow: 0.0,
                paint: false,
            },
            None,
        );
        assert!(!bare[0].contains("48;2"));
        assert!(bare[0].starts_with("\x1b[0m "));
    }

    #[test]
    fn the_arguments() {
        let a = |v: &[&str]| parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        let ok = a(&[
            "d.txt",
            "--frames",
            "rest,work",
            "--bg",
            "none",
            "--glow",
            "3",
            "--fps",
            "4",
            "--compare",
        ])
        .unwrap();
        assert_eq!(
            (ok.frames.len(), ok.bg, ok.glow, ok.fps, ok.compare),
            (2, None, 2.0, 4.0, true)
        );
        assert_eq!(a(&["--demo"]).unwrap().bg, Some(DARK));
        assert!(a(&[]).is_err());
        assert!(a(&["--demo", "--bg", "12"]).is_err());
        assert!(a(&["--demo", "--wat"]).is_err());
    }
}
