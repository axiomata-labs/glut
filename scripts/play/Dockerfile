# Glut on Linux to try out: Glut built from this checkout, in a Debian with the tools a session needs.
#
# Built by scripts/play/glut-play.sh, which sends the sources as the build context (Glut and Axiomata-Foundation beside
# it): Docker Desktop hangs on folders of the Mac handed in.

FROM rust:1-bookworm AS build
COPY Axiomata-Foundation /src/Axiomata-Foundation
COPY Axiomata-Glut /src/Axiomata-Glut
WORKDIR /src/Axiomata-Glut
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/src/Axiomata-Glut/target \
    cargo build --release --locked -p glut-tui && cp target/release/glut /glut

FROM rust:1-bookworm
# The sandbox (bubblewrap), what Glut's tools and a model's commands reach for, and two language servers Glut asks.
RUN apt-get update && apt-get install -y --no-install-recommends \
        bubblewrap git ripgrep fd-find curl ca-certificates less zsh python3 python3-pip python3-venv nodejs npm \
        neovim jq unzip locales \
    && rm -rf /var/lib/apt/lists/* \
    && sed -i 's/^# *de_DE.UTF-8/de_DE.UTF-8/; s/^# *en_US.UTF-8/en_US.UTF-8/' /etc/locale.gen && locale-gen \
    && ln -s /usr/bin/fdfind /usr/local/bin/fd \
    && rustup component add rust-analyzer clippy rustfmt \
    && useradd -m -s /bin/zsh dev \
    && chmod -R a+rwX /usr/local/cargo /usr/local/rustup
COPY --from=build /glut /usr/local/bin/glut
ENV LANG=de_DE.UTF-8 TERM=xterm-256color COLORTERM=truecolor
USER dev
WORKDIR /home/dev
RUN mkdir -p /home/dev/work && git config --global init.defaultBranch main
CMD ["zsh", "-l"]
