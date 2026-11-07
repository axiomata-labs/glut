#!/bin/sh
# Installs glut from a release: the program for this system, checked against the release's SHA-256 checksums, into
# ~/.local/bin (no password asked). It is the release's `install.sh`, the same file at that release's tag:
#
#   curl -fsSL https://github.com/axiomata-labs/glut/releases/latest/download/install.sh | sh
#   curl -fsSL https://github.com/axiomata-labs/glut/releases/latest/download/install.sh | sh -s -- --version 0.1.0 --to ~/bin
#
# POSIX sh on purpose: it is piped into whatever `sh` the system has.
set -eu

REPO="axiomata-labs/glut"

usage() {
  cat <<'EOF'
install.sh — installs glut from a GitHub release

  --version X.Y.Z   that release instead of the newest
  --to DIR          the folder for the program (default: ~/.local/bin)
  -h, --help        this text
EOF
}

fail() {
  echo "install.sh: $*" >&2
  exit 1
}

version=""
to="${HOME:?HOME is not set}/.local/bin"
while [ $# -gt 0 ]; do
  case "$1" in
    --version)
      [ $# -ge 2 ] || fail "--version needs a version, such as 0.1.0"
      version="${2#v}"
      shift 2
      ;;
    --to)
      [ $# -ge 2 ] || fail "--to needs a folder"
      to="$2"
      shift 2
      ;;
    -h | --help)
      usage
      exit 0
      ;;
    *) fail "unknown option $1 (see --help)" ;;
  esac
done

# A version is digits and dots (and a pre-release tail): it goes into a URL.
case "$version" in
  "" | [0-9]*) ;;
  *) fail "not a version: $version" ;;
esac
case "$version" in
  *[!0-9A-Za-z.+-]*) fail "not a version: $version" ;;
esac

case "$(uname -s)/$(uname -m)" in
  Darwin/arm64) target="aarch64-apple-darwin" ;;
  Darwin/x86_64) target="x86_64-apple-darwin" ;;
  Linux/x86_64 | Linux/amd64) target="x86_64-unknown-linux-musl" ;;
  Linux/aarch64 | Linux/arm64) target="aarch64-unknown-linux-musl" ;;
  MINGW* | MSYS* | CYGWIN*) fail "on Windows, Glut runs in WSL2: wsl --install in PowerShell, then this line in its Ubuntu terminal" ;;
  *) fail "no prebuilt glut for $(uname -s) $(uname -m); build it with: cargo install glut-tui" ;;
esac

if [ -n "$version" ]; then
  base="https://github.com/$REPO/releases/download/v$version"
else
  base="https://github.com/$REPO/releases/latest/download"
fi

command -v curl >/dev/null 2>&1 || fail "curl is needed"
command -v tar >/dev/null 2>&1 || fail "tar is needed"
if command -v sha256sum >/dev/null 2>&1; then
  sum() { sha256sum "$1" | cut -d ' ' -f 1; }
elif command -v shasum >/dev/null 2>&1; then
  sum() { shasum -a 256 "$1" | cut -d ' ' -f 1; }
else
  fail "sha256sum or shasum is needed to check the download"
fi

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT INT TERM

archive="glut-$target.tar.gz"
fetch() {
  # HTTPS only, also after a redirect.
  curl -fsSL --proto '=https' --tlsv1.2 -o "$work/$1" "$base/$1" || fail "could not download $base/$1"
}
echo "glut: downloading $archive${version:+ (v$version)}"
fetch "$archive"
fetch SHA256SUMS

expected="$(awk -v name="$archive" '$2 == name || $2 == "*" name { print $1 }' "$work/SHA256SUMS")"
[ -n "$expected" ] || fail "SHA256SUMS names no $archive"
actual="$(sum "$work/$archive")"
[ "$expected" = "$actual" ] || fail "checksum mismatch for $archive: expected $expected, got $actual"

tar -xzf "$work/$archive" -C "$work"
[ -f "$work/glut-$target/glut" ] || fail "the archive holds no glut-$target/glut"

mkdir -p "$to"
# Written beside the old program and renamed over it: a running glut keeps its file.
cp "$work/glut-$target/glut" "$to/.glut.new"
chmod 0755 "$to/.glut.new"
mv -f "$to/.glut.new" "$to/glut"
echo "glut: installed $("$to/glut" --version 2>/dev/null || echo glut) to $to/glut"

case ":${PATH:-}:" in
  *":$to:"*) ;;
  *)
    echo
    echo "$to is not on your PATH. Add this line to your shell's profile (~/.zshrc, ~/.bashrc …):"
    echo "  export PATH=\"$to:\$PATH\""
    ;;
esac

# WSL1 has no Linux kernel of its own, so no namespaces: the sandbox needs WSL2 ("microsoft-standard-WSL2").
if grep -q "Microsoft" /proc/sys/kernel/osrelease 2>/dev/null; then
  echo
  echo "This is WSL1: Glut's sandbox needs WSL2 (wsl --set-version <distribution> 2 in PowerShell)."
fi

if [ "$(uname -s)" = "Linux" ] && ! command -v bwrap >/dev/null 2>&1; then
  echo
  echo "Glut's sandbox on Linux needs bubblewrap (bwrap): sudo apt install bubblewrap, dnf install bubblewrap …"
fi
