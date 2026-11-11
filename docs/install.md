# Installing Glut

Glut is one program, `glut`, for macOS (Apple silicon and Intel) and Linux (x86_64 and arm64); on Windows it runs
in WSL2.

## The install script

```sh
curl -fsSL https://glut-ai.com/install.sh | sh
```

It picks the program for your system from the newest [release](https://github.com/axiomata-labs/glut/releases), checks it
against the release's SHA-256 checksums, and puts it into `~/.local/bin` — no password asked. If that folder is not
on your `PATH`, it says which line to add to your shell's profile. Read the script first if you like: it is short,
and it is the same file as `scripts/install.sh` in the repository. The same script lies beside every release on
GitHub too, should the site be away:

```sh
curl -fsSL https://github.com/axiomata-labs/glut/releases/latest/download/install.sh | sh
```

To pick a version or a folder:

```sh
curl -fsSL https://glut-ai.com/install.sh | sh -s -- --version 0.1.0 --to ~/bin
```

## Homebrew

```sh
brew install axiomata-labs/tap/glut
brew upgrade glut          # later
```

## Cargo

```sh
cargo install glut-tui     # installs the program `glut`
```

This builds Glut from source and needs a current stable [Rust](https://rustup.rs).

## From a checkout

```sh
git clone https://github.com/axiomata-labs/glut
cd glut
cargo install --path crates/glut-tui --locked
```

## The first start on macOS

The prebuilt program is not yet signed with an Apple Developer ID. When you download it yourself (rather than
through the install script or Homebrew, which do not mark it), macOS may refuse to open it the first time. Allow it
once, either in *System Settings → Privacy & Security → "glut" was blocked → Open Anyway*, or in a terminal:

```sh
xattr -d com.apple.quarantine "$(command -v glut)"
```

The sandbox Glut uses on macOS, Seatbelt (`/usr/bin/sandbox-exec`), is part of the system: nothing else to install.

## Linux

Glut's sandbox needs [bubblewrap](https://github.com/containers/bubblewrap) (`bwrap`) and unprivileged user
namespaces:

```sh
sudo apt install bubblewrap        # Debian, Ubuntu
sudo dnf install bubblewrap        # Fedora
sudo apk add bubblewrap            # Alpine
```

On **Ubuntu 23.10 and later**, AppArmor refuses unprivileged user namespaces by default, so bubblewrap cannot make a
sandbox there; Glut says so when it starts. Give `bwrap` alone the right with an AppArmor profile (tested on Ubuntu
24.04):

```sh
sudo tee /etc/apparmor.d/bwrap > /dev/null <<'EOF'
abi <abi/4.0>,
include <tunables/global>

profile bwrap /usr/bin/bwrap flags=(unconfined) {
  userns,

  include if exists <local/bwrap>
}
EOF
sudo apparmor_parser -r /etc/apparmor.d/bwrap
```

Every other program keeps Ubuntu's restriction. Setting `kernel.apparmor_restrict_unprivileged_userns=0` works too,
but lifts it for all of them.

Without a working sandbox Glut still starts, but every command it would run asks you first — and `/sandbox required
on` refuses them outright.

## Windows

Glut runs in **WSL2**, which is a Linux kernel: the Linux build and its sandbox run there as they are. In PowerShell,
`wsl --install` sets up WSL2 with Ubuntu; then, in the Ubuntu terminal, use the install script and bubblewrap as on
Linux above. WSL1 has no namespaces, so the sandbox needs WSL2 (`wsl --set-version <distribution> 2`).

- Keep projects in the WSL home (`~/…`), not on the Windows drive (`/mnt/c`): files there are slow and pass Linux's
  permissions by.
- Plans (`/schedule`) need systemd, which WSL starts only with `systemd=true` under `[boot]` in `/etc/wsl.conf`
  (then `wsl --shutdown`); Glut says so when you add one.
- VS Code reaches Glut in WSL through its WSL remote; a JetBrains IDE on Windows only with WSL's mirrored networking.
- A native Windows build, with a sandbox of Windows' own, is planned.

## Checking it

```sh
glut --version
glut sandbox show     # the sandbox on this system, and whether git folders are kept read-only
```

## Removing it

```sh
rm "$(command -v glut)"      # or: brew uninstall glut / cargo uninstall glut-tui
rm -rf ~/.glut               # your settings, sessions, snapshots — only if you want them gone
```

Provider keys live in the macOS Keychain under the service `glut` (on Linux in `~/.glut/credentials.json`, readable
only by you); `glut providers remove <id>` before you uninstall removes a key, *Keychain Access* does too.
