# Build environment for the Linux AppImage. Ubuntu 22.04 (glibc 2.35) so the binary also
# runs on older distros: an AppImage works on systems as new as or newer than its build box.
FROM ubuntu:22.04

ARG DEBIAN_FRONTEND=noninteractive
RUN apt-get update && apt-get install -y --no-install-recommends \
        build-essential ca-certificates clang curl file git pkg-config python3 \
        libfontconfig1-dev libfreetype6-dev libxkbcommon-dev libxkbcommon-x11-dev libwayland-dev \
        libx11-dev libx11-xcb-dev libxcursor-dev libxrandr-dev libxi-dev libgl1-mesa-dev \
        desktop-file-utils squashfs-tools zsync \
    && rm -rf /var/lib/apt/lists/*

ENV RUSTUP_HOME=/usr/local/rustup CARGO_HOME=/usr/local/cargo PATH=/usr/local/cargo/bin:$PATH
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable \
    && chmod -R a+w $RUSTUP_HOME $CARGO_HOME

RUN curl -sSfL -o /usr/local/bin/appimagetool \
        https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage \
    && chmod +x /usr/local/bin/appimagetool
# no FUSE inside a container: AppImages unpack themselves and run
ENV APPIMAGE_EXTRACT_AND_RUN=1

WORKDIR /src
