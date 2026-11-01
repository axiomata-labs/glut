#!/usr/bin/env bash
# Glut on Linux in Docker, to try out (T59): builds the image from this checkout and opens a shell in it.
#
#   scripts/play/glut-play.sh          the shell (the image built first if it is missing)
#   scripts/play/glut-play.sh build    the image built anew from the checkout as it stands
#
# The home folder lives in the volume `glut-play-home`: providers, keys (the file store — Linux has no Keychain here),
# sessions and ~/work stay from one start to the next. No folder of the Mac is handed in: Docker Desktop hangs on
# that. Get a project in with git clone, or `docker cp`.
set -euo pipefail

cd "$(dirname "$0")/../.."
glut="$PWD"
foundation="$(cd ../Axiomata-Foundation && pwd)"
image=glut-play

build() {
  # The build context is a tar of both checkouts, without what a build left there or git's own folder.
  COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata --exclude target --exclude .git --exclude node_modules \
    -c -C "$(dirname "$glut")" "$(basename "$glut")" -C "$(dirname "$foundation")" "$(basename "$foundation")" |
    docker build -t "$image" -f "$(basename "$glut")/scripts/play/Dockerfile" -
}

if [[ "${1:-}" == "build" ]] || ! docker image inspect "$image" >/dev/null 2>&1; then
  build
fi
[[ "${1:-}" == "build" ]] && exit 0

# bubblewrap needs namespaces and a /proc of its own inside the container.
exec docker run --rm -it --hostname glut-play \
  --security-opt seccomp=unconfined --security-opt systempaths=unconfined \
  -v glut-play-home:/home/dev "$image"
