# NeonGauge (claude-limit) — Rust + egui desktop app
#
# Targets:
#   runtime         (default) Linux image that runs the app over X11/Wayland (Linux, WSLg)
#   export-linux    build the Linux binary only     -> docker build --target export-linux   --output dist .
#   export-windows  cross-compile claude-limit.exe  -> docker build --target export-windows --output dist .

ARG RUST_VERSION=1
ARG DEBIAN_VERSION=bookworm

# ---------- base: toolchain + cached dependencies ----------
FROM rust:${RUST_VERSION}-${DEBIAN_VERSION} AS base
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
# Download crates once; cached until Cargo.toml / Cargo.lock change
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo fetch --locked \
    && rm -rf src

# ---------- build-linux ----------
FROM base AS build-linux
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked \
    && rm -rf src
COPY src ./src
# touch so cargo rebuilds our crate instead of reusing the stub
RUN touch src/*.rs \
    && cargo build --release --locked \
    && cp target/release/claude-limit /claude-limit

# ---------- build-windows (x86_64-pc-windows-gnu via mingw) ----------
FROM base AS build-windows
ARG WIN_TARGET=x86_64-pc-windows-gnu
RUN apt-get update \
    && apt-get install -y --no-install-recommends gcc-mingw-w64-x86-64 \
    && rm -rf /var/lib/apt/lists/* \
    && rustup target add ${WIN_TARGET}
ENV CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
RUN mkdir src && echo "fn main() {}" > src/main.rs \
    && cargo build --release --locked --target ${WIN_TARGET} \
    && rm -rf src
COPY src ./src
RUN touch src/*.rs \
    && cargo build --release --locked --target ${WIN_TARGET} \
    && cp target/${WIN_TARGET}/release/claude-limit.exe /claude-limit.exe

# ---------- export stages (files only, use with --output) ----------
FROM scratch AS export-linux
COPY --from=build-linux /claude-limit /

FROM scratch AS export-windows
COPY --from=build-windows /claude-limit.exe /

# ---------- runtime (default) ----------
FROM debian:${DEBIAN_VERSION}-slim AS runtime
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        ca-certificates \
        libgl1 libegl1 libgl1-mesa-dri \
        libx11-6 libxcursor1 libxrandr2 libxi6 libx11-xcb1 \
        libxkbcommon0 libxkbcommon-x11-0 \
        libwayland-client0 libwayland-egl1 libwayland-cursor0 \
    && rm -rf /var/lib/apt/lists/*

ARG UID=1000
RUN useradd --create-home --uid ${UID} app
USER app
ENV HOME=/home/app

COPY --from=build-linux /claude-limit /usr/local/bin/claude-limit
ENTRYPOINT ["claude-limit"]
