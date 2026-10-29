//! Glut's colours (T5, "Glut + Hex"): orange only where something lives, works or needs the owner — the running
//! step, an open question, the input's cursor — quiet greys for what is settled. The terminal's own background and
//! text colour stay, so Glut sits in any theme. Besides Glut's own palette, the app's themes (TUI3c), their values
//! taken from Axiomata's `themes/*.css` on 2026-10-22 — Glut reads nothing of Axiomata's at run time; `/theme` picks
//! one and `prefs.json` keeps it. A theme's glow is its accent, as it is in the app (a Catppuccin theme glows
//! mauve); Glut colours only the text, so a light theme wants a light terminal.

use std::sync::RwLock;

use ratatui::style::{Color, Modifier, Style};

/// The colours of one theme.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Palette {
    pub name: &'static str,
    /// What lives, works or needs the owner (the accent).
    pub glow: Color,
    /// The glow's darker edge, for the mascot's outline and a waiting mark.
    pub ember: Color,
    /// Settled things: a finished tool line, a hint.
    pub quiet: Color,
    /// The owner's own words.
    pub owner: Color,
    /// Something failed.
    pub fault: Color,
    /// Something went right.
    pub fine: Color,
    /// Dark text on the glow (the search's match shown): the theme's background.
    pub ink: Color,
    /// A search's other matches: the glow, faint.
    pub found: Color,
    /// Inline code in an answer.
    pub code: Color,
    /// A link's text.
    pub link: Color,
    pub syntax: Syntax,
}

/// The colours of code (tree-sitter's captures, `highlight.rs`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Syntax {
    pub keyword: Color,
    pub string: Color,
    pub number: Color,
    pub comment: Color,
    pub function: Color,
    pub kind: Color,
    pub constant: Color,
    pub property: Color,
    pub operator: Color,
    pub punctuation: Color,
    pub tag: Color,
    pub attribute: Color,
}

/// Glut's own palette, the default: its orange (`--ax-harness-glut` in Axiomata's tokens) and calm code colours.
pub const GLUT: Palette = Palette {
    name: "glut",
    glow: Color::Rgb(0xe8, 0xb0, 0x4a),
    ember: Color::Rgb(0xa8, 0x5a, 0x1e),
    quiet: Color::Rgb(0x8a, 0x8f, 0x98),
    owner: Color::Rgb(0x9c, 0xc8, 0xe8),
    fault: Color::Rgb(0xe0, 0x6c, 0x6c),
    fine: Color::Rgb(0x8c, 0xc8, 0x8c),
    ink: Color::Rgb(0x1a, 0x1a, 0x1a),
    found: Color::Rgb(0x5a, 0x44, 0x20),
    code: Color::Rgb(0x8b, 0xd5, 0xca),
    link: Color::Rgb(0x8a, 0xad, 0xf4),
    syntax: Syntax {
        keyword: Color::Rgb(0xc6, 0xa0, 0xf6),
        string: Color::Rgb(0xa6, 0xda, 0x95),
        number: Color::Rgb(0xf5, 0xbd, 0xe6),
        comment: Color::Rgb(0x8a, 0x8f, 0x98),
        function: Color::Rgb(0x8a, 0xad, 0xf4),
        kind: Color::Rgb(0xee, 0xd4, 0x9f),
        constant: Color::Rgb(0xf5, 0xbd, 0xe6),
        property: Color::Rgb(0x8b, 0xd5, 0xca),
        operator: Color::Rgb(0x93, 0x9a, 0xb7),
        punctuation: Color::Rgb(0x93, 0x9a, 0xb7),
        tag: Color::Rgb(0xed, 0x87, 0x96),
        attribute: Color::Rgb(0xee, 0xd4, 0x9f),
    },
};

/// The app's themes, by the names Axiomata gives them.
pub const THEMES: [Palette; 13] = [
    Palette {
        name: "catppuccin-espresso",
        glow: Color::Rgb(0xc6, 0xa0, 0xf6),
        ember: Color::Rgb(0x77, 0x60, 0x94),
        quiet: Color::Rgb(0xb8, 0xc0, 0xe0),
        owner: Color::Rgb(0xb7, 0xbd, 0xf8),
        fault: Color::Rgb(0xed, 0x87, 0x96),
        fine: Color::Rgb(0xa6, 0xda, 0x95),
        ink: Color::Rgb(0x00, 0x00, 0x00),
        found: Color::Rgb(0x33, 0x2a, 0x40),
        code: Color::Rgb(0xa6, 0xda, 0x95),
        link: Color::Rgb(0xb7, 0xbd, 0xf8),
        syntax: Syntax {
            keyword: Color::Rgb(0xc6, 0xa0, 0xf6),
            string: Color::Rgb(0xa6, 0xda, 0x95),
            number: Color::Rgb(0xf5, 0xa9, 0x7f),
            comment: Color::Rgb(0x93, 0x9a, 0xb7),
            function: Color::Rgb(0x8a, 0xad, 0xf4),
            kind: Color::Rgb(0xee, 0xd4, 0x9f),
            constant: Color::Rgb(0xf5, 0xa9, 0x7f),
            property: Color::Rgb(0xb7, 0xbd, 0xf8),
            operator: Color::Rgb(0x91, 0xd7, 0xe3),
            punctuation: Color::Rgb(0x93, 0x9a, 0xb7),
            tag: Color::Rgb(0x8a, 0xad, 0xf4),
            attribute: Color::Rgb(0xee, 0xd4, 0x9f),
        },
    },
    Palette {
        name: "catppuccin-frappe",
        glow: Color::Rgb(0xca, 0x9e, 0xe6),
        ember: Color::Rgb(0x79, 0x5f, 0x8a),
        quiet: Color::Rgb(0xa5, 0xad, 0xce),
        owner: Color::Rgb(0x8c, 0xaa, 0xee),
        fault: Color::Rgb(0xe7, 0x82, 0x84),
        fine: Color::Rgb(0xa6, 0xd1, 0x89),
        ink: Color::Rgb(0x23, 0x26, 0x34),
        found: Color::Rgb(0x4e, 0x45, 0x62),
        code: Color::Rgb(0xa6, 0xd1, 0x89),
        link: Color::Rgb(0x8c, 0xaa, 0xee),
        syntax: Syntax {
            keyword: Color::Rgb(0xca, 0x9e, 0xe6),
            string: Color::Rgb(0xa6, 0xd1, 0x89),
            number: Color::Rgb(0xef, 0x9f, 0x76),
            comment: Color::Rgb(0x73, 0x79, 0x94),
            function: Color::Rgb(0x8c, 0xaa, 0xee),
            kind: Color::Rgb(0xe5, 0xc8, 0x90),
            constant: Color::Rgb(0xef, 0x9f, 0x76),
            property: Color::Rgb(0xba, 0xbb, 0xf1),
            operator: Color::Rgb(0x99, 0xd1, 0xdb),
            punctuation: Color::Rgb(0x94, 0x9c, 0xbb),
            tag: Color::Rgb(0xe7, 0x82, 0x84),
            attribute: Color::Rgb(0xe5, 0xc8, 0x90),
        },
    },
    Palette {
        name: "catppuccin-latte",
        glow: Color::Rgb(0x88, 0x39, 0xef),
        ember: Color::Rgb(0x52, 0x22, 0x8f),
        quiet: Color::Rgb(0x5c, 0x5f, 0x77),
        owner: Color::Rgb(0x1e, 0x66, 0xf5),
        fault: Color::Rgb(0xd2, 0x0f, 0x39),
        fine: Color::Rgb(0x40, 0xa0, 0x2b),
        ink: Color::Rgb(0xe6, 0xe9, 0xef),
        found: Color::Rgb(0xce, 0xbb, 0xef),
        code: Color::Rgb(0x40, 0xa0, 0x2b),
        link: Color::Rgb(0x1e, 0x66, 0xf5),
        syntax: Syntax {
            keyword: Color::Rgb(0x88, 0x39, 0xef),
            string: Color::Rgb(0x40, 0xa0, 0x2b),
            number: Color::Rgb(0xfe, 0x64, 0x0b),
            comment: Color::Rgb(0x9c, 0xa0, 0xb0),
            function: Color::Rgb(0x1e, 0x66, 0xf5),
            kind: Color::Rgb(0xdf, 0x8e, 0x1d),
            constant: Color::Rgb(0xfe, 0x64, 0x0b),
            property: Color::Rgb(0x72, 0x87, 0xfd),
            operator: Color::Rgb(0x04, 0xa5, 0xe5),
            punctuation: Color::Rgb(0x7c, 0x7f, 0x93),
            tag: Color::Rgb(0xd2, 0x0f, 0x39),
            attribute: Color::Rgb(0xdf, 0x8e, 0x1d),
        },
    },
    Palette {
        name: "catppuccin-macchiato",
        glow: Color::Rgb(0xc6, 0xa0, 0xf6),
        ember: Color::Rgb(0x77, 0x60, 0x94),
        quiet: Color::Rgb(0xa5, 0xad, 0xcb),
        owner: Color::Rgb(0x8a, 0xad, 0xf4),
        fault: Color::Rgb(0xed, 0x87, 0x96),
        fine: Color::Rgb(0xa6, 0xda, 0x95),
        ink: Color::Rgb(0x18, 0x19, 0x26),
        found: Color::Rgb(0x45, 0x3c, 0x5c),
        code: Color::Rgb(0xa6, 0xda, 0x95),
        link: Color::Rgb(0x8a, 0xad, 0xf4),
        syntax: Syntax {
            keyword: Color::Rgb(0xc6, 0xa0, 0xf6),
            string: Color::Rgb(0xa6, 0xda, 0x95),
            number: Color::Rgb(0xf5, 0xa9, 0x7f),
            comment: Color::Rgb(0x6e, 0x73, 0x8d),
            function: Color::Rgb(0x8a, 0xad, 0xf4),
            kind: Color::Rgb(0xee, 0xd4, 0x9f),
            constant: Color::Rgb(0xf5, 0xa9, 0x7f),
            property: Color::Rgb(0xb7, 0xbd, 0xf8),
            operator: Color::Rgb(0x91, 0xd7, 0xe3),
            punctuation: Color::Rgb(0x93, 0x9a, 0xb7),
            tag: Color::Rgb(0xed, 0x87, 0x96),
            attribute: Color::Rgb(0xee, 0xd4, 0x9f),
        },
    },
    Palette {
        name: "catppuccin-mocha",
        glow: Color::Rgb(0xcb, 0xa6, 0xf7),
        ember: Color::Rgb(0x7a, 0x64, 0x94),
        quiet: Color::Rgb(0xa6, 0xad, 0xc8),
        owner: Color::Rgb(0x89, 0xb4, 0xfa),
        fault: Color::Rgb(0xf3, 0x8b, 0xa8),
        fine: Color::Rgb(0xa6, 0xe3, 0xa1),
        ink: Color::Rgb(0x11, 0x11, 0x1b),
        found: Color::Rgb(0x41, 0x38, 0x54),
        code: Color::Rgb(0xa6, 0xe3, 0xa1),
        link: Color::Rgb(0x89, 0xb4, 0xfa),
        syntax: Syntax {
            keyword: Color::Rgb(0xcb, 0xa6, 0xf7),
            string: Color::Rgb(0xa6, 0xe3, 0xa1),
            number: Color::Rgb(0xfa, 0xb3, 0x87),
            comment: Color::Rgb(0x6c, 0x70, 0x86),
            function: Color::Rgb(0x89, 0xb4, 0xfa),
            kind: Color::Rgb(0xf9, 0xe2, 0xaf),
            constant: Color::Rgb(0xfa, 0xb3, 0x87),
            property: Color::Rgb(0xb4, 0xbe, 0xfe),
            operator: Color::Rgb(0x89, 0xdc, 0xeb),
            punctuation: Color::Rgb(0x93, 0x99, 0xb2),
            tag: Color::Rgb(0xf3, 0x8b, 0xa8),
            attribute: Color::Rgb(0xf9, 0xe2, 0xaf),
        },
    },
    Palette {
        name: "forest",
        glow: Color::Rgb(0xe8, 0x84, 0x2c),
        ember: Color::Rgb(0x8b, 0x4f, 0x1a),
        quiet: Color::Rgb(0x8f, 0xa8, 0x92),
        owner: Color::Rgb(0x8f, 0xd0, 0xc8),
        fault: Color::Rgb(0xee, 0x6f, 0x5c),
        fine: Color::Rgb(0x7f, 0xd0, 0x7a),
        ink: Color::Rgb(0x08, 0x10, 0x0c),
        found: Color::Rgb(0x42, 0x2e, 0x14),
        code: Color::Rgb(0xa6, 0xd9, 0x8a),
        link: Color::Rgb(0x8f, 0xd0, 0xc8),
        syntax: Syntax {
            keyword: Color::Rgb(0xe8, 0x94, 0x3f),
            string: Color::Rgb(0xa6, 0xd9, 0x8a),
            number: Color::Rgb(0xd7, 0xb0, 0xf0),
            comment: Color::Rgb(0x62, 0x80, 0x6a),
            function: Color::Rgb(0x8f, 0xd0, 0xc8),
            kind: Color::Rgb(0x6f, 0xc2, 0xa8),
            constant: Color::Rgb(0xe3, 0xc0, 0x5a),
            property: Color::Rgb(0xb9, 0xd6, 0xa8),
            operator: Color::Rgb(0xc7, 0xd6, 0xc2),
            punctuation: Color::Rgb(0x7f, 0x9a, 0x86),
            tag: Color::Rgb(0xee, 0x8a, 0x6c),
            attribute: Color::Rgb(0xe3, 0xc0, 0x5a),
        },
    },
    Palette {
        name: "github-dark",
        glow: Color::Rgb(0x44, 0x93, 0xf8),
        ember: Color::Rgb(0x29, 0x58, 0x95),
        quiet: Color::Rgb(0x91, 0x98, 0xa1),
        owner: Color::Rgb(0x44, 0x93, 0xf8),
        fault: Color::Rgb(0xf8, 0x51, 0x49),
        fine: Color::Rgb(0x3f, 0xb9, 0x50),
        ink: Color::Rgb(0x01, 0x04, 0x09),
        found: Color::Rgb(0x12, 0x29, 0x47),
        code: Color::Rgb(0xa5, 0xd6, 0xff),
        link: Color::Rgb(0x44, 0x93, 0xf8),
        syntax: Syntax {
            keyword: Color::Rgb(0xff, 0x7b, 0x72),
            string: Color::Rgb(0xa5, 0xd6, 0xff),
            number: Color::Rgb(0x79, 0xc0, 0xff),
            comment: Color::Rgb(0x91, 0x98, 0xa1),
            function: Color::Rgb(0xd2, 0xa8, 0xff),
            kind: Color::Rgb(0xff, 0xa6, 0x57),
            constant: Color::Rgb(0x79, 0xc0, 0xff),
            property: Color::Rgb(0x79, 0xc0, 0xff),
            operator: Color::Rgb(0xff, 0x7b, 0x72),
            punctuation: Color::Rgb(0x91, 0x98, 0xa1),
            tag: Color::Rgb(0x7e, 0xe7, 0x87),
            attribute: Color::Rgb(0x79, 0xc0, 0xff),
        },
    },
    Palette {
        name: "github-light",
        glow: Color::Rgb(0x09, 0x69, 0xda),
        ember: Color::Rgb(0x05, 0x3f, 0x83),
        quiet: Color::Rgb(0x59, 0x63, 0x6e),
        owner: Color::Rgb(0x09, 0x69, 0xda),
        fault: Color::Rgb(0xd1, 0x24, 0x2f),
        fine: Color::Rgb(0x1a, 0x7f, 0x37),
        ink: Color::Rgb(0xf6, 0xf8, 0xfa),
        found: Color::Rgb(0xb8, 0xd3, 0xf2),
        code: Color::Rgb(0x0a, 0x30, 0x69),
        link: Color::Rgb(0x09, 0x69, 0xda),
        syntax: Syntax {
            keyword: Color::Rgb(0xcf, 0x22, 0x2e),
            string: Color::Rgb(0x0a, 0x30, 0x69),
            number: Color::Rgb(0x05, 0x50, 0xae),
            comment: Color::Rgb(0x59, 0x63, 0x6e),
            function: Color::Rgb(0x66, 0x39, 0xba),
            kind: Color::Rgb(0x95, 0x38, 0x00),
            constant: Color::Rgb(0x05, 0x50, 0xae),
            property: Color::Rgb(0x05, 0x50, 0xae),
            operator: Color::Rgb(0xcf, 0x22, 0x2e),
            punctuation: Color::Rgb(0x59, 0x63, 0x6e),
            tag: Color::Rgb(0x05, 0x50, 0xae),
            attribute: Color::Rgb(0x05, 0x50, 0xae),
        },
    },
    Palette {
        name: "graphite",
        glow: Color::Rgb(0xff, 0x7a, 0x1a),
        ember: Color::Rgb(0x99, 0x49, 0x10),
        quiet: Color::Rgb(0x94, 0x94, 0x9c),
        owner: Color::Rgb(0x7c, 0xb8, 0xff),
        fault: Color::Rgb(0xf2, 0x6d, 0x6d),
        fine: Color::Rgb(0x4f, 0xd6, 0x7f),
        ink: Color::Rgb(0x00, 0x00, 0x00),
        found: Color::Rgb(0x42, 0x20, 0x07),
        code: Color::Rgb(0x9a, 0xd4, 0x8a),
        link: Color::Rgb(0x7c, 0xb8, 0xff),
        syntax: Syntax {
            keyword: Color::Rgb(0xff, 0x9f, 0x5a),
            string: Color::Rgb(0x9a, 0xd4, 0x8a),
            number: Color::Rgb(0xd4, 0xa8, 0xff),
            comment: Color::Rgb(0x6e, 0x6e, 0x78),
            function: Color::Rgb(0x7c, 0xb8, 0xff),
            kind: Color::Rgb(0x5f, 0xd4, 0xc4),
            constant: Color::Rgb(0xf0, 0xb8, 0x60),
            property: Color::Rgb(0xb6, 0xc4, 0xff),
            operator: Color::Rgb(0xc8, 0xc8, 0xd0),
            punctuation: Color::Rgb(0x8a, 0x8a, 0x94),
            tag: Color::Rgb(0xff, 0x8a, 0x70),
            attribute: Color::Rgb(0xe8, 0xc2, 0x7c),
        },
    },
    Palette {
        name: "ocean",
        glow: Color::Rgb(0xff, 0x8a, 0x5b),
        ember: Color::Rgb(0x99, 0x53, 0x37),
        quiet: Color::Rgb(0x85, 0xa3, 0xb3),
        owner: Color::Rgb(0x6e, 0xc6, 0xff),
        fault: Color::Rgb(0xf2, 0x71, 0x6f),
        fine: Color::Rgb(0x4f, 0xd6, 0xb8),
        ink: Color::Rgb(0x04, 0x0c, 0x12),
        found: Color::Rgb(0x45, 0x2d, 0x25),
        code: Color::Rgb(0x8f, 0xdc, 0xb5),
        link: Color::Rgb(0x6e, 0xc6, 0xff),
        syntax: Syntax {
            keyword: Color::Rgb(0xff, 0x9a, 0x6e),
            string: Color::Rgb(0x8f, 0xdc, 0xb5),
            number: Color::Rgb(0xc9, 0xa6, 0xff),
            comment: Color::Rgb(0x5a, 0x7c, 0x8e),
            function: Color::Rgb(0x6e, 0xc6, 0xff),
            kind: Color::Rgb(0x4f, 0xd6, 0xc8),
            constant: Color::Rgb(0xe8, 0xc4, 0x6a),
            property: Color::Rgb(0xa9, 0xcd, 0xe0),
            operator: Color::Rgb(0xbc, 0xd2, 0xdd),
            punctuation: Color::Rgb(0x78, 0x96, 0xa6),
            tag: Color::Rgb(0xff, 0x8a, 0x70),
            attribute: Color::Rgb(0xe8, 0xc4, 0x6a),
        },
    },
    Palette {
        name: "paper",
        glow: Color::Rgb(0xd0, 0x64, 0x2c),
        ember: Color::Rgb(0x7d, 0x3c, 0x1a),
        quiet: Color::Rgb(0x63, 0x59, 0x4e),
        owner: Color::Rgb(0x2b, 0x5f, 0x9e),
        fault: Color::Rgb(0xb2, 0x3a, 0x3a),
        fine: Color::Rgb(0x3f, 0x7d, 0x4e),
        ink: Color::Rgb(0xf3, 0xef, 0xe6),
        found: Color::Rgb(0xea, 0xcb, 0xb6),
        code: Color::Rgb(0x3f, 0x7d, 0x4e),
        link: Color::Rgb(0x2b, 0x5f, 0x9e),
        syntax: Syntax {
            keyword: Color::Rgb(0xb0, 0x52, 0x1c),
            string: Color::Rgb(0x3f, 0x7d, 0x4e),
            number: Color::Rgb(0x7a, 0x4f, 0xa0),
            comment: Color::Rgb(0x8f, 0x85, 0x78),
            function: Color::Rgb(0x2b, 0x5f, 0x9e),
            kind: Color::Rgb(0x1f, 0x7a, 0x70),
            constant: Color::Rgb(0x9a, 0x6a, 0x12),
            property: Color::Rgb(0x45, 0x60, 0x9a),
            operator: Color::Rgb(0x4a, 0x42, 0x38),
            punctuation: Color::Rgb(0x7a, 0x70, 0x64),
            tag: Color::Rgb(0xa8, 0x43, 0x2a),
            attribute: Color::Rgb(0x8a, 0x5a, 0x12),
        },
    },
    Palette {
        name: "steampunk",
        glow: Color::Rgb(0xd9, 0x93, 0x2e),
        ember: Color::Rgb(0x82, 0x58, 0x1c),
        quiet: Color::Rgb(0xa8, 0x92, 0x7a),
        owner: Color::Rgb(0x7f, 0xb3, 0xa6),
        fault: Color::Rgb(0xd4, 0x57, 0x3f),
        fine: Color::Rgb(0x8f, 0xb8, 0x5a),
        ink: Color::Rgb(0x11, 0x0d, 0x0a),
        found: Color::Rgb(0x45, 0x30, 0x13),
        code: Color::Rgb(0xa9, 0xc4, 0x6c),
        link: Color::Rgb(0x7f, 0xb3, 0xa6),
        syntax: Syntax {
            keyword: Color::Rgb(0xe0, 0xa1, 0x4a),
            string: Color::Rgb(0xa9, 0xc4, 0x6c),
            number: Color::Rgb(0xd9, 0x8f, 0x6c),
            comment: Color::Rgb(0x80, 0x6b, 0x56),
            function: Color::Rgb(0xe8, 0xc4, 0x7e),
            kind: Color::Rgb(0x7f, 0xb3, 0xa6),
            constant: Color::Rgb(0xd9, 0x93, 0x2e),
            property: Color::Rgb(0xcb, 0xb4, 0x91),
            operator: Color::Rgb(0xcd, 0xb8, 0x99),
            punctuation: Color::Rgb(0x9c, 0x86, 0x70),
            tag: Color::Rgb(0xd4, 0x73, 0x5a),
            attribute: Color::Rgb(0xe0, 0xa8, 0x3c),
        },
    },
    Palette {
        name: "tokyo-night",
        glow: Color::Rgb(0x7a, 0xa2, 0xf7),
        ember: Color::Rgb(0x49, 0x61, 0x94),
        quiet: Color::Rgb(0xa9, 0xb1, 0xd6),
        owner: Color::Rgb(0x7d, 0xcf, 0xff),
        fault: Color::Rgb(0xf7, 0x76, 0x8e),
        fine: Color::Rgb(0x9e, 0xce, 0x6a),
        ink: Color::Rgb(0x16, 0x16, 0x1e),
        found: Color::Rgb(0x30, 0x3a, 0x56),
        code: Color::Rgb(0x9e, 0xce, 0x6a),
        link: Color::Rgb(0x7d, 0xcf, 0xff),
        syntax: Syntax {
            keyword: Color::Rgb(0xbb, 0x9a, 0xf7),
            string: Color::Rgb(0x9e, 0xce, 0x6a),
            number: Color::Rgb(0xff, 0x9e, 0x64),
            comment: Color::Rgb(0x56, 0x5f, 0x89),
            function: Color::Rgb(0x7a, 0xa2, 0xf7),
            kind: Color::Rgb(0x2a, 0xc3, 0xde),
            constant: Color::Rgb(0xff, 0x9e, 0x64),
            property: Color::Rgb(0x7d, 0xcf, 0xff),
            operator: Color::Rgb(0x89, 0xdd, 0xff),
            punctuation: Color::Rgb(0x73, 0x7a, 0xa2),
            tag: Color::Rgb(0xf7, 0x76, 0x8e),
            attribute: Color::Rgb(0xe0, 0xaf, 0x68),
        },
    },
];

/// The palette in use.
static CURRENT: RwLock<Palette> = RwLock::new(GLUT);

/// The palette in use now.
pub fn c() -> Palette {
    *CURRENT
        .read()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `a` moved toward `b` by `share` (0 is `a`, 1 is `b`): the footer's calm backgrounds. A colour that is no RGB
/// (a terminal's own) stays `a`.
pub fn mix(a: Color, b: Color, share: f32) -> Color {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return a;
    };
    let part = |from: u8, to: u8| {
        (f32::from(from) + (f32::from(to) - f32::from(from)) * share).round() as u8
    };
    Color::Rgb(part(ar, br), part(ag, bg), part(ab, bb))
}

/// The palette named `name` (Glut's own or an app theme's), if there is one.
pub fn named(name: &str) -> Option<Palette> {
    std::iter::once(GLUT)
        .chain(THEMES)
        .find(|palette| palette.name == name)
}

/// Every palette, Glut's own first.
pub fn all() -> impl Iterator<Item = Palette> {
    std::iter::once(GLUT).chain(THEMES)
}

/// Uses the palette named `name` from now on; `false` when there is none of that name.
pub fn set(name: &str) -> bool {
    let Some(palette) = named(name) else {
        return false;
    };
    *CURRENT
        .write()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = palette;
    true
}

pub fn quiet() -> Style {
    Style::default().fg(c().quiet)
}

pub fn glow() -> Style {
    Style::default().fg(c().glow)
}

/// What the owner runs themselves: a `!`/`!!` command line in the input, in the theme's warning colour — it runs
/// straight in the owner's terminal, whatever it does, so it must never pass for a message (T78).
pub fn shell() -> Style {
    Style::default().fg(c().fault)
}

pub fn fault() -> Style {
    Style::default().fg(c().fault)
}

/// How much of a hue a muted background holds, in hundredths (T84): elegant, not garish — the palette's colours mixed
/// well into the theme's own background. The preview draws more than one.
static TINT: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(30);

/// Sets how strong the muted hues are, in hundredths.
#[cfg(test)]
pub fn set_tint(percent: u8) {
    TINT.store(percent.min(100), std::sync::atomic::Ordering::Relaxed);
}

fn tint_share() -> f32 {
    f32::from(TINT.load(std::sync::atomic::Ordering::Relaxed)) / 100.0
}

/// Whether the screen keeps to the colours it had before T84: in the reader mode, and where `NO_COLOR` asks for none
/// (read once, the standard's way: set and not empty).
pub fn plain() -> bool {
    static NO_COLOR: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    crate::motion::reader()
        || *NO_COLOR
            .get_or_init(|| std::env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()))
}

/// `hue` as a muted background: mixed into the theme's background (T84).
pub fn tint(hue: Color) -> Color {
    mix(c().ink, hue, tint_share())
}

/// `hue` as a muted foreground for a name in the stream: between the quiet grey and the hue itself (T84).
pub fn soft(hue: Color) -> Color {
    if plain() {
        return c().quiet;
    }
    mix(c().quiet, hue, (0.4 + tint_share()).min(1.0))
}

/// How far apart, in RGB, two hues must be to read as two (T84); a theme's colours that lie closer count as one.
const DISTINCT: f32 = 40.0;
/// How far a hue must lie from the accent and the fault colour, whose meanings it must never borrow.
const MEANING_APART: f32 = 60.0;

fn distance(a: Color, b: Color) -> f32 {
    let (Color::Rgb(ar, ag, ab), Color::Rgb(br, bg, bb)) = (a, b) else {
        return if a == b { 0.0 } else { f32::MAX };
    };
    let d = |x: u8, y: u8| f32::from(x) - f32::from(y);
    (d(ar, br).powi(2) + d(ag, bg).powi(2) + d(ab, bb).powi(2)).sqrt()
}

/// The theme's hues for kinds and panes (T84), in order: its palette's colours that stand apart from one another and
/// from what already has a meaning — the accent (what works or asks), the fault colour, the quiet grey and the
/// background. A theme whose link is its accent (GitHub's) or whose keyword is (Catppuccin's) loses that one here.
fn hues() -> Vec<Color> {
    hues_of(&c())
}

/// [`hues`] of `palette`.
fn hues_of(palette: &Palette) -> Vec<Color> {
    let meanings = [palette.glow, palette.fault];
    let taken = [palette.quiet, palette.ink];
    let candidates = [
        palette.link,
        palette.code,
        palette.syntax.keyword,
        palette.fine,
        palette.syntax.number,
        palette.owner,
        palette.syntax.string,
        palette.syntax.kind,
        palette.syntax.tag,
        palette.syntax.function,
        palette.syntax.property,
        palette.syntax.constant,
        palette.syntax.attribute,
    ];
    let mut hues: Vec<Color> = Vec::new();
    for hue in candidates {
        let apart = |other: &Color| distance(hue, *other) >= DISTINCT;
        let free = meanings
            .iter()
            .all(|meaning| distance(hue, *meaning) >= MEANING_APART);
        if free && taken.iter().all(apart) && hues.iter().all(apart) {
            hues.push(hue);
        }
    }
    if hues.is_empty() {
        hues.push(palette.quiet);
    }
    hues
}

/// The hue of a tool's kind (T84), one meaning each: writing in the accent, reading, commands, what reaches out of the
/// machine and the agents each a hue of the theme's own; `None` for one of no kind of these.
pub fn tool_hue(name: &str) -> Option<Color> {
    if plain() {
        return None;
    }
    let hues = hues();
    let at = |index: usize| hues[index % hues.len()];
    Some(match name {
        "write_file" | "edit_file" | "glut_file" | "memory" => c().glow,
        "read_file" | "glob" | "grep" | "list_dir" | "outline" | "read_full_result" | "lsp" => {
            at(0)
        }
        "bash" | "bash_output" | "kill_shell" | "codemode" => at(1),
        "web_fetch" | "web_search" => at(2),
        name if name.starts_with("mcp__") => at(2),
        "task" | "tell" => at(3),
        _ => return None,
    })
}

/// The hue of pane or background agent `number` (T84): its own among the others, never the accent, which is left to
/// what works or asks.
pub fn pane_hue(number: u8) -> Color {
    let hues = hues();
    hues[usize::from(number.saturating_sub(1)) % hues.len()]
}

/// A background beside `before` that would melt into it is set apart a little (T84): two segments of one hue side by
/// side still show where one ends.
pub fn apart_from(bg: Color, before: Color) -> Color {
    if distance(bg, before) >= DISTINCT / 4.0 {
        return bg;
    }
    mix(bg, c().quiet, 0.25)
}

pub fn strong() -> Style {
    Style::default().add_modifier(Modifier::BOLD)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_theme_has_three_hues_apart_and_none_near_the_accent_or_the_fault_colour() {
        // Each palette read as it is: the screen's theme is the tests' to share, never switched here.
        for palette in all() {
            let hues = hues_of(&palette);
            assert!(
                hues.len() >= 3,
                "{}: reading, commands and what reaches out need three",
                palette.name
            );
            for (at, one) in hues.iter().enumerate() {
                assert!(
                    distance(*one, palette.glow) >= MEANING_APART,
                    "{}",
                    palette.name
                );
                assert!(
                    distance(*one, palette.fault) >= MEANING_APART,
                    "{}",
                    palette.name
                );
                for other in &hues[at + 1..] {
                    assert!(
                        distance(*one, *other) >= DISTINCT,
                        "{}: {one:?} ~ {other:?}",
                        palette.name
                    );
                }
            }
        }
    }

    #[test]
    fn a_tool_kind_and_a_pane_have_a_hue_of_their_own() {
        // The reader mode or `NO_COLOR` in the shell keeps the colours of before: nothing to compare.
        if plain() {
            return;
        }
        assert_eq!(
            tool_hue("read_file"),
            tool_hue("grep"),
            "reading is one kind"
        );
        assert_ne!(tool_hue("read_file"), tool_hue("edit_file"));
        assert_ne!(tool_hue("bash"), tool_hue("mcp__mail__send"));
        assert_eq!(
            tool_hue("mcp__mail__send"),
            tool_hue("web_fetch"),
            "what reaches out is one kind"
        );
        assert_eq!(tool_hue("todo"), None);
        assert_ne!(pane_hue(1), pane_hue(2));
    }

    #[test]
    fn every_theme_has_a_name_of_its_own_and_glut_is_the_default() {
        let names: Vec<&str> = all().map(|palette| palette.name).collect();
        for (at, name) in names.iter().enumerate() {
            assert!(!names[at + 1..].contains(name), "{name} twice");
        }
        assert_eq!(names[0], "glut");
        assert!(names.contains(&"tokyo-night") && names.contains(&"graphite"));
        assert!(named("nonesuch").is_none());
    }
}
