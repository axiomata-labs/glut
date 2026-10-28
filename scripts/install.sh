#!/usr/bin/env bash
# Builds glut and installs it where every terminal finds it, as Axiomata's `build-app.sh --install` does for the app.
#
#   scripts/install.sh
#
# The program goes to cargo's own folder (`~/.cargo/bin`, no password), by `cargo install`: a copy, never a link into
# `target/`, which agents write to — a plan (`glut schedule`) refuses a program there. A terminal finds it through
# `/usr/local/bin`, which macOS puts on every PATH (`/etc/paths`): the first run asks once to link `glut` there (`sudo`);
# every later install updates the file the link points at, without a password.
set -euo pipefail

cd "$(dirname "$0")/.."
bin="${CARGO_INSTALL_ROOT:-${CARGO_HOME:-$HOME/.cargo}}/bin"
installed="$bin/glut"
link=/usr/local/bin/glut

# The release build first: the owner also starts target/release/glut, and `cargo install` then only copies it.
cargo build --release --locked -p glut-tui
cargo install --path crates/glut-tui --locked --quiet
echo "installed: $installed ($("$installed" --version))"

found="$(command -v glut || true)"
if [[ -n "$found" && "$(realpath "$found")" == "$(realpath "$installed")" ]]; then
  echo "found as: $found"
  exit 0
fi

if [[ -e "$link" || -L "$link" ]]; then
  # Somebody else's glut, or an old link: never replaced without the owner's look.
  echo "$link exists and is not this glut ($(realpath "$link" 2>/dev/null || echo "a broken link"))." >&2
  echo "Remove it, or put $bin first on your PATH." >&2
  exit 1
fi

read -r -p "Link $link to $installed, so every terminal finds glut? This asks for your password once. [y/N] " answer
if [[ "$answer" =~ ^[yYjJ]$ ]]; then
  sudo ln -s "$installed" "$link"
  echo "linked: $link → $installed"
  echo "A terminal that was open before finds it after 'hash -r' or in a new window."
else
  echo "Not linked. Start it as $installed, or put $bin on your PATH."
fi
