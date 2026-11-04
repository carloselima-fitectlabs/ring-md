# syntax=docker/dockerfile:1
#
# RingMD container images. Stages:
#   base     Ubuntu 24.04 + Neovim (shared by every other stage)
#   dev      base + Rust toolchain + GTK 4 dev files + Xvfb: build, lint, test
#   build    dev + the source, compiled in release mode
#   runtime  base + GTK 4 runtime libraries + the ringmd binary (default)
#
# See docker-compose.yml for how to run each one.

ARG UBUNTU_VERSION=24.04

FROM ubuntu:${UBUNTU_VERSION} AS base
# Ubuntu 24.04 ships Neovim 0.9; RingMD needs 0.10+ (F2.R3). Keep in sync
# with .github/workflows/ci.yml.
ARG NVIM_VERSION=v0.10.4
ARG TARGETARCH
ENV DEBIAN_FRONTEND=noninteractive
RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl \
 && case "${TARGETARCH:-amd64}" in \
      amd64) nvim_arch=x86_64 ;; \
      arm64) nvim_arch=arm64 ;; \
      *) echo "unsupported architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
 && curl -fsSL "https://github.com/neovim/neovim/releases/download/${NVIM_VERSION}/nvim-linux-${nvim_arch}.tar.gz" \
    | tar xz -C /opt \
 && ln -s "/opt/nvim-linux-${nvim_arch}/bin/nvim" /usr/local/bin/nvim \
 && nvim --version | head -1 \
 && rm -rf /var/lib/apt/lists/*

FROM base AS dev
# Minimum is rust-version in Cargo.toml; CI uses the latest stable too.
ARG RUST_TOOLCHAIN=stable
ARG TARGETARCH
ENV RUSTUP_HOME=/usr/local/rustup \
    CARGO_HOME=/usr/local/cargo \
    CARGO_TARGET_DIR=/target \
    PATH=/usr/local/cargo/bin:${PATH}
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      build-essential pkg-config libgtk-4-dev libwebkitgtk-6.0-dev \
      xvfb xauth fonts-dejavu-core git \
 && rm -rf /var/lib/apt/lists/*
# rustup-init straight from static.rust-lang.org, checked against its
# published SHA-256 (as the official rust images do).
RUN case "${TARGETARCH:-amd64}" in \
      amd64) rust_arch=x86_64-unknown-linux-gnu ;; \
      arm64) rust_arch=aarch64-unknown-linux-gnu ;; \
      *) echo "unsupported architecture: ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
 && url="https://static.rust-lang.org/rustup/dist/${rust_arch}/rustup-init" \
 && curl -fsSLo /tmp/rustup-init "${url}" \
 && echo "$(curl -fsSL "${url}.sha256" | cut -d' ' -f1)  /tmp/rustup-init" | sha256sum -c - \
 && chmod +x /tmp/rustup-init \
 && /tmp/rustup-init -y --no-modify-path --profile minimal \
      --default-toolchain "${RUST_TOOLCHAIN}" --component rustfmt,clippy \
 && rm /tmp/rustup-init \
 && rustc --version \
 # The dev container runs as the host user (see docker-compose.yml), so the
 # cargo registry and target directory must be writable by any UID.
 && mkdir -p "${CARGO_HOME}/registry" "${CARGO_HOME}/git" "${CARGO_TARGET_DIR}" \
 && chmod -R a+rwX "${CARGO_HOME}" "${CARGO_TARGET_DIR}"
WORKDIR /src

FROM dev AS build
COPY . .
RUN cargo build --release --locked \
 && install -Dm755 "${CARGO_TARGET_DIR}/release/ringmd" /out/bin/ringmd \
 && mkdir -p /out/share/ringmd \
 && cp -r runtime /out/share/ringmd/runtime

FROM base AS runtime
RUN apt-get update \
 && apt-get install -y --no-install-recommends \
      libgtk-4-1 libwebkitgtk-6.0-4 libgl1-mesa-dri fonts-dejavu-core \
 && rm -rf /var/lib/apt/lists/*
# ringmd finds its Neovim config at <prefix>/share/ringmd/runtime.
COPY --from=build /out/ /usr/local/
# No GPU inside the container by default; the Cairo renderer always works.
ENV GSK_RENDERER=cairo
# uid 1000, matching the usual first desktop user, so X11 access and files
# written to mounted folders line up with the host.
USER ubuntu
WORKDIR /work
ENTRYPOINT ["ringmd"]
