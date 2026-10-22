# Glimm — the designs

Glut's mascot (T27 in `plans/glut-tui.md`). The owner chose **design A** on 2026-10-22 ("Größe können wir ja immer
noch anpassen") and asked to keep the other two in case he changes his mind. A lives in
`crates/glut-tui/src/glimm.rs`; B and C are kept here, in the same letters, so either can take its place there.

The drafts page with all three in their five moods, animated, and a mock of Glut's screen:
https://claude.ai/artifact/8j3h3Y5SJX8AG2xmVyP35V (private to the owner).

## How a design is written

Each letter is one pixel; two pixel rows make one terminal row (`▀` with the upper pixel as its colour and the lower as
its background). A large Glimm is 12 × 14 pixels (12 columns × 7 rows), a small one 8 × 8 (8 columns × 4 rows).

| letter | colour | what |
|---|---|---|
| `.` | — | no pixel, the terminal's own background |
| `R` | `#6e1a0c` | the deepest edge (B's hex bowl) |
| `r` | `#b8360f` | the flame's edge |
| `o` | `#ff7a1a` | orange |
| `a` | `#ffa040` | amber |
| `y` | `#ffd36b` | yellow |
| `w` | `#fff2cc` | white-hot heart |

The eyes are not part of the rows: each design names its eye pixels, and the mood draws them (open, wide with a
highlight, pleased `^`, drooping). A failed Glimm turns every flame colour grey and keeps an ember (`#d9480f`) below.

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
