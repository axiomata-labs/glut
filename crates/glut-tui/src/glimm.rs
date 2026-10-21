//! Glimm, Glut's mascot (T27): a small flame with eyes, drawn in half blocks. Still while Glut waits, flickering
//! while it works — the flame's tip moves, nothing else.

/// Glimm at rest, row by row (four rows of half blocks: eight pixels high).
const AT_REST: [&str; 4] = ["   ▄█   ", " ▄████▄ ", "██▄██▄██", "▀██████▀"];

/// The frames of the flicker: only the tip changes.
const FLICKER: [&str; 4] = ["   ▄█   ", "   █▄   ", "  ▄█    ", "   ▀█▄  "];

/// Glimm's rows for `frame` of the animation; `working` false is the resting flame.
pub fn rows(working: bool, frame: usize) -> [&'static str; 4] {
    let mut rows = AT_REST;
    if working {
        rows[0] = FLICKER[frame % FLICKER.len()];
    }
    rows
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
    use unicode_width::UnicodeWidthStr;

    #[test]
    fn every_frame_keeps_glimms_width() {
        for frame in 0..8 {
            for row in rows(true, frame) {
                assert_eq!(row.width(), 8, "{row:?}");
            }
        }
    }
}
