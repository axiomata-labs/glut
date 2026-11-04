//! Glimm for the site: the frames the terminal UI draws (`crates/glut-tui/src/glimm/a-large.txt`) and the colours it
//! gives their letters (`glimm.rs`, `fn color`), written as one script the page's canvas reads. Read from the
//! program's sources at every build, so the site's Glimm never drifts from the one in the terminal.

use std::path::Path;

use super::read;

/// The drawing in use and the file of its colours.
const FRAMES: &str = "crates/glut-tui/src/glimm/a-large.txt";
const COLOURS: &str = "crates/glut-tui/src/glimm.rs";

/// The script: `window.GLIMM = {w, h, colours: {letter: "#rrggbb"}, frames: {name: [rows]}}`.
///
/// # Errors
///
/// A source that cannot be read or holds no frames or colours.
pub fn script(repo: &Path) -> Result<String, String> {
    let (width, height, frames) = frames(&read(&repo.join(FRAMES))?)?;
    let colours = colours(&read(&repo.join(COLOURS))?);
    complete(&frames, &colours)?;
    let colours: Vec<String> = colours
        .iter()
        .map(|(letter, hex)| format!("\"{letter}\":\"{hex}\""))
        .collect();
    let frames: Vec<String> = frames
        .iter()
        .map(|(name, rows)| {
            let rows: Vec<String> = rows.iter().map(|row| format!("\"{row}\"")).collect();
            format!("\"{name}\":[{}]", rows.join(","))
        })
        .collect();
    Ok(format!(
        "// Made by `cargo run -p xtask -- site build` from {FRAMES} and {COLOURS}.\n\
         window.GLIMM={{w:{width},h:{height},colours:{{{}}},frames:{{{}}}}};\n",
        colours.join(","),
        frames.join(",")
    ))
}

/// Glimm at rest as a small SVG, for the browser's tab: one rectangle per lit pixel, two units tall as on the screen.
///
/// # Errors
///
/// A source that cannot be read or holds no frame `rest0`.
pub fn svg(repo: &Path) -> Result<String, String> {
    let (width, height, frames) = frames(&read(&repo.join(FRAMES))?)?;
    let colours: std::collections::HashMap<char, String> =
        colours(&read(&repo.join(COLOURS))?).into_iter().collect();
    let (_, rows) = frames
        .iter()
        .find(|(name, _)| name == "rest0")
        .ok_or_else(|| format!("{FRAMES}: no frame rest0"))?;
    let mut svg = format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" viewBox=\"0 0 {width} {}\" shape-rendering=\"crispEdges\">",
        height * 2
    );
    for (y, row) in rows.iter().enumerate() {
        for (x, letter) in row.chars().enumerate() {
            if let Some(colour) = colours.get(&letter) {
                svg.push_str(&format!(
                    "<rect x=\"{x}\" y=\"{}\" width=\"1\" height=\"2\" fill=\"{colour}\"/>",
                    y * 2
                ));
            }
        }
    }
    svg.push_str("</svg>\n");
    Ok(svg)
}

/// A drawing: its width and height in pixels, and its frames by name, each a row of letters per pixel row.
type Drawing = (usize, usize, Vec<(String, Vec<String>)>);

/// The frames the site's script draws (`site/static/site.js`): a drawing without one would leave Glimm blank there.
const NEEDED: [&str; 14] = [
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
    "done0",
    "done1",
    "wait0",
];

/// Whether every frame the site draws is there and every letter in them has a colour — a colour arm the reader
/// missed (a reformatted line) would otherwise draw as a hole, silently.
fn complete(frames: &[(String, Vec<String>)], colours: &[(char, String)]) -> Result<(), String> {
    for needed in NEEDED {
        if !frames.iter().any(|(name, _)| name == needed) {
            return Err(format!("{FRAMES}: no frame {needed}, which the site draws"));
        }
    }
    for (name, rows) in frames {
        if let Some(letter) = rows
            .iter()
            .flat_map(|row| row.chars())
            .find(|letter| *letter != '.' && !colours.iter().any(|(known, _)| known == letter))
        {
            return Err(format!(
                "{FRAMES}: frame {name} uses '{letter}', which has no colour read from {COLOURS}"
            ));
        }
    }
    Ok(())
}

/// The frames of a drawing file: `size W H`, `#` comments, and per frame `@ name` followed by its rows.
fn frames(text: &str) -> Result<Drawing, String> {
    let mut size = None;
    let mut frames: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines().map(str::trim_end) {
        if let Some(rest) = line.strip_prefix("size ") {
            let mut numbers = rest
                .split_whitespace()
                .filter_map(|n| n.parse::<usize>().ok());
            size = numbers.next().zip(numbers.next());
        } else if let Some(name) = line.strip_prefix("@ ") {
            let name = name.trim().to_owned();
            if frames.iter().any(|(known, _)| *known == name) {
                return Err(format!("{FRAMES}: frame {name} twice"));
            }
            frames.push((name, Vec::new()));
        } else if line.is_empty() || line.starts_with('#') {
            continue;
        } else if frames.is_empty() {
            return Err(format!("{FRAMES}: a row before the first `@ name`: {line}"));
        } else if let Some((_, rows)) = frames.last_mut() {
            // Only letters and dots reach the script, inside quotes.
            if !line.chars().all(|c| c.is_ascii_alphabetic() || c == '.') {
                return Err(format!("{FRAMES}: a row of other characters: {line}"));
            }
            rows.push(line.to_owned());
        }
    }
    let (width, height) = size.ok_or_else(|| format!("{FRAMES}: no `size` line"))?;
    for (name, rows) in &frames {
        if rows.len() != height || rows.iter().any(|row| row.len() != width) {
            return Err(format!("{FRAMES}: frame {name} is not {width} × {height}"));
        }
    }
    if frames.is_empty() {
        return Err(format!("{FRAMES}: no frames"));
    }
    Ok((width, height, frames))
}

/// The lit colours of `fn color`: each arm `('x', false) => (0x.., 0x.., 0x..)` or `('x', _) => …`, as `#rrggbb`. The
/// greys of a flame gone out (`true`) are the terminal's alone.
fn colours(source: &str) -> Vec<(char, String)> {
    let mut found = Vec::new();
    for line in source.lines().map(str::trim) {
        let Some(rest) = line.strip_prefix("('") else {
            continue;
        };
        let mut chars = rest.chars();
        let (Some(letter), Some('\'')) = (chars.next(), chars.next()) else {
            continue;
        };
        let rest: String = chars.collect();
        let Some((ash, value)) = rest.split_once(") => (") else {
            continue;
        };
        if ash.trim_start_matches(',').trim() == "true" {
            continue;
        }
        let bytes: Vec<u8> = value
            .trim_end_matches([')', ','])
            .split(',')
            .filter_map(|part| u8::from_str_radix(part.trim().trim_start_matches("0x"), 16).ok())
            .collect();
        if let [r, g, b] = bytes[..] {
            found.push((letter, format!("#{r:02x}{g:02x}{b:02x}")));
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_colours_and_frames_are_read_from_the_programs_sources() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        let script = script(&repo).unwrap();
        assert!(script.contains("\"r\":\"#b8360f\""), "{script}");
        assert!(
            script.contains("\"rest0\":[") && script.contains("\"work5\":["),
            "{script}"
        );
        assert!(
            !script.contains("#45454c"),
            "the greys of a flame gone out stay the terminal's"
        );
    }

    #[test]
    fn a_frame_of_the_wrong_size_is_refused() {
        assert!(frames("size 2 2\n@ a\n..\n.\n").is_err());
        assert!(frames("size 2 1\n@ a\n\"x\n").is_err());
        let (w, h, found) = frames("size 2 1\n# note\n@ a\nr.\n").unwrap();
        assert_eq!((w, h, found.len()), (2, 1, 1));
        assert!(
            frames("size 2 1\n@ a\nr.\n@ a\nr.\n").is_err(),
            "a frame twice"
        );
        assert!(
            frames("size 2 1\nr.\n@ a\nr.\n").is_err(),
            "a row before any frame"
        );
        assert!(
            complete(&found, &[('r', "#b8360f".to_owned())]).is_err(),
            "rest0 and the others are missing"
        );
        let all: Vec<(String, Vec<String>)> = NEEDED
            .iter()
            .map(|name| ((*name).to_owned(), vec!["rq".to_owned()]))
            .collect();
        assert!(
            complete(&all, &[('r', "#b8360f".to_owned())]).is_err(),
            "q has no colour"
        );
    }
}
