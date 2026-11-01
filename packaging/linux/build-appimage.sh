#!/bin/sh
# Builds Winklet-<version>-x86_64.AppImage into dist/. Run it on Linux, or from any OS with
# Docker:
#   docker build -t winklet-linux packaging/linux
#   docker run --rm -v "$PWD:/src" -v winklet-target:/target -e CARGO_TARGET_DIR=/target \
#       winklet-linux sh packaging/linux/build-appimage.sh
set -eu

cd "$(dirname "$0")/../.."
VERSION=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n 1)
TARGET_DIR=${CARGO_TARGET_DIR:-target}

cargo build --release --locked
BIN="$TARGET_DIR/release/winklet"

APPDIR=$(mktemp -d)/Winklet.AppDir
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/applications" "$APPDIR/usr/share/icons/hicolor/256x256/apps"
cp "$BIN" "$APPDIR/usr/bin/winklet"
cp packaging/linux/winklet.desktop "$APPDIR/usr/share/applications/winklet.desktop"
cp packaging/linux/winklet.desktop "$APPDIR/winklet.desktop"
cp assets/icon.png "$APPDIR/usr/share/icons/hicolor/256x256/apps/winklet.png"
cp assets/icon.png "$APPDIR/winklet.png"
cp assets/icon.png "$APPDIR/.DirIcon"
cp packaging/linux/AppRun "$APPDIR/AppRun"
chmod +x "$APPDIR/AppRun" "$APPDIR/usr/bin/winklet"
desktop-file-validate "$APPDIR/winklet.desktop"

mkdir -p dist
OUT="dist/Winklet-$VERSION-x86_64.AppImage"
ARCH=x86_64 appimagetool --no-appstream "$APPDIR" "$OUT"
echo "Built $OUT"
