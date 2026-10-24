#!/usr/bin/env sh
#
# Download a static ffmpeg build for this machine into src-tauri/binaries/,
# named the way Tauri expects for a sidecar (ffmpeg-<target-triple>).
# Skips the download if it's already there. Builds: https://ffmpeg.martin-riedl.de
#
set -eu

cd "$(dirname "$0")/.."

TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
DEST="src-tauri/binaries/ffmpeg-$TRIPLE"

if [ -x "$DEST" ]; then
  exit 0
fi

# Pinned ffmpeg 9.0.2 release builds + their SHA-256 checksums.
BASE="https://ffmpeg.martin-riedl.de/download"
case "$TRIPLE" in
  aarch64-apple-darwin)
    URL="$BASE/macos/arm64/1789931890_9.0.2/ffmpeg.zip"
    SHA="c8ed4c4e6978a03c485edbfe4e0a5dc2380f8a30bba5150531b31b094492d924" ;;
  x86_64-apple-darwin)
    URL="$BASE/macos/amd64/1789931006_9.0.2/ffmpeg.zip"
    SHA="7c6b4125b191cbf773832dc51f424cf2b6bb7da43007d1e066f95909e47cacd4" ;;
  x86_64-unknown-linux-gnu)
    URL="$BASE/linux/amd64/1789931100_9.0.2/ffmpeg.zip"
    SHA="fa8ecf4abbd290d98f7d188b8649cc6b391ae209a98452be955a15aab1909d7f" ;;
  aarch64-unknown-linux-gnu)
    URL="$BASE/linux/arm64/1789931697_9.0.2/ffmpeg.zip"
    SHA="93a76ae90db5474eecdf951a729857c64f3de23567228d6a7d5e6e8e3cd1021b" ;;
  *)
    echo "No bundled ffmpeg for $TRIPLE. Put an ffmpeg binary at $DEST yourself." >&2
    exit 1 ;;
esac

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading ffmpeg for $TRIPLE..."
curl -fsSL "$URL" -o "$TMP/ffmpeg.zip"

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL="$(sha256sum "$TMP/ffmpeg.zip" | cut -d' ' -f1)"
else
  ACTUAL="$(shasum -a 256 "$TMP/ffmpeg.zip" | cut -d' ' -f1)"
fi
if [ "$ACTUAL" != "$SHA" ]; then
  echo "ffmpeg checksum mismatch (got $ACTUAL)" >&2
  exit 1
fi

unzip -q "$TMP/ffmpeg.zip" -d "$TMP/out"
mkdir -p "$(dirname "$DEST")"
mv "$TMP/out/ffmpeg" "$DEST"
chmod +x "$DEST"
echo "Saved $DEST"
