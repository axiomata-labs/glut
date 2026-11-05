# Glimm — the designs

Glut's mascot (T27 in `plans/glut-tui.md`). The owner chose **design A** on 2026-10-22 ("Größe können wir ja immer
noch anpassen") and asked to keep the other two in case he changes his mind. A lives in
`crates/glut-tui/src/glimm.rs`; B and C are kept here, in the same letters, so either can take its place there.

The drafts page with all three in their five moods, animated, and a mock of Glut's screen:
https://claude.ai/artifact/8j3h3Y5SJX8AG2xmVyP35V (private to the owner).

## How a design is written (since T72)

A drawing is a file per size in `crates/glut-tui/src/glimm/` (`a-large.txt`, `a-mini.txt`, and the same for `c-`),
read at compile time (`include_str!`) and parsed once. Its first line (after `#` comments) is `size <width> <height>`
in pixels, both even; then frames — `@ <name>`, exactly `height` lines of exactly `width` letters, a blank line between
frames. A terminal cell holds 2 × 2 pixels (Unicode quadrant blocks), and a cell is about
one wide and two tall, so **a pixel is 1 wide : 2 tall** — a round shape is drawn about twice as wide as tall.

| size | pixels | cells | shown |
|---|---|---|---|
| large | 32 × 20 | 16 × 10 | the welcome |
| mini | 8 × 4 | 4 × 2 | before a thought while the model thinks (working frames only, no glow) |

Frames (the large one; the mini one has `work0`–`work3`): `rest0`–`rest3` (the tip leaning left, a breath, right, the
plain one), `restblink`, `work0`–`work5` (a flicker, run 0→5→0, sparks drawn in), `wait0` (the "!") and `wait1`
(without), `done0` and `done1` (with the twinkle), `fail0`–`fail2` (the flame out, the ember, smoke). Sparks, the "!",
the twinkle and the smoke are pixels of the frames; the eyes and the highlight too.

| letter | colour | what |
|---|---|---|
| `.` | — | no pixel, the terminal's own background |
| `R` | `#6e1a0c` | the deepest edge |
| `r` | `#b8360f` | the flame's edge |
| `o` | `#ff7a1a` | orange |
| `a` | `#ffa040` | amber |
| `y` | `#ffd36b` | yellow |
| `w` | `#fff2cc` | white-hot heart (drawn pulsing between `y` and `w`) |
| `e` `h` `m` | dark, white, dark | eye, its highlight, a pleased `^` / a shut or drooping eye |
| `E` | `#d9480f` | the ember |
| `s` | `#80808c` | smoke |

A failed Glimm (the mood, not the name of a frame) turns every flame colour grey and keeps the ember. The **glow** is
not drawn: it is computed from the flame's pixels (`R r o a y w`) in rings, blended over the terminal's own background
colour (`background.rs`, asked once with OSC 11) into the transparent pixels, strength 0.18 and per mood (working
warmer, resting weaker, waiting with the "!", failed none); where the background is unknown there is none. Leave room
for it in a drawing: at least 4 pixels at the sides and 2 above and below.

`crates/glut-tui/examples/glimm_preview.rs` shows a file as it is drawn (`sh crates/glut-tui/examples/glimm_fine/show.sh`
for both designs, `--animate` to play one). **A is in use**; C is kept and changes in as `ACTIVE` in `glimm.rs`.

The designs below are the first half-block drawings (12 × 14 and 8 × 8 pixels, one letter a square pixel), kept for
their shapes.

## B — Glimm in the hex

The flame stands in a hexagonal bowl of embers: the system's shape.

```
large, eyes at (6,4) (6,7)      small, eyes at (3,2) (3,5)
.....y......                    ...y....
.....ya.....                    ...oa...
....roa.....                    ..oaar..
....roar....                    ..ayya..
...rooaor...                    ..aywa..
...oaaaao...                    .RroooR.
...oayyao...                    RroooorR
...oaywao...                    .RrrrrR.
...raywar...
..RRrrrrRR..
.RrooooooorR
.RroayyaorR.
..RRrrrrRR..
............
```

## C — Glimm round

Rounder, with large eyes of 2 × 2 pixels and a highlight in each.

```
large, eyes (2×2) at (7,3) (7,8)    small, eyes at (4,2) (4,5)
......y.....                        ...ya...
.....ya.....                        ..roar..
.....oar....                        .roaaor.
....roaar...                        roayyaor
...roaaaor..                        oayyyyao
..roaayaaor.                        oaywwyao
.roaayyyaaor                        .raywar.
.oaaayyyaaao                        ..rrrr..
.oaaayyyaaao
.oaayywwyyao
.raayywwyyar
..raaywwyar.
...rraaarr..
....rrrrr...
```
