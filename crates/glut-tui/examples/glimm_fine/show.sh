#!/bin/sh
# Renders every Glimm draft (A and C) in every frame, at a weak, a weaker and no glow, on the terminal's own
# background. Run from anywhere: sh crates/glut-tui/examples/glimm_fine/show.sh (large drawings only; the small and mini files are ./src/glimm/*-small.txt, *-mini.txt) [bg RRGGBB]
# The bg is the terminal's background colour (default 1e1e2e, Ghostty's); `--bg none` shows the hard edge.
set -e
here=$(cd "$(dirname "$0")" && pwd)
root=$(cd "$here/../../../.." && pwd)
cd "$root"
bg=${1:-1e1e2e}
COLUMNS=${COLUMNS:-$(tput cols 2>/dev/null || echo 100)}
export COLUMNS
cargo build -q -p glut-tui --example glimm_preview
run() { cargo run -q -p glut-tui --example glimm_preview -- "$@"; }
for design in a c; do
    for glow in 0.35 0.18; do
        printf '\n\n######## Design %s — glow %s (bg %s) ########\n\n' "$design" "$glow" "$bg"
        run "$root/crates/glut-tui/src/glimm/$design-large.txt" --bg "$bg" --glow "$glow"
    done
    printf '\n\n######## Design %s — no glow ########\n\n' "$design"
    run "$root/crates/glut-tui/src/glimm/$design-large.txt" --bg none
done
