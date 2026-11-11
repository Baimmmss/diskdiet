#!/bin/sh
# Installs the latest diskdiet release binary.
#   curl -fsSL https://raw.githubusercontent.com/Baimmmss/diskdiet/main/install.sh | sh
# Set DISKDIET_INSTALL_DIR to choose where it goes (default: ~/.local/bin).
set -eu

REPO="Baimmmss/diskdiet"
DIR="${DISKDIET_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-musl" ;;
  *) echo "diskdiet: unsupported OS $(uname -s); try: cargo install diskdiet" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *) echo "diskdiet: unsupported CPU $(uname -m); try: cargo install diskdiet" >&2; exit 1 ;;
esac

target="$arch-$os"
url="https://github.com/$REPO/releases/latest/download/diskdiet-$target.tar.gz"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading diskdiet for $target…"
curl -fsSL "$url" -o "$tmp/diskdiet.tar.gz"
curl -fsSL "https://github.com/$REPO/releases/latest/download/diskdiet-$target.sha256" -o "$tmp/diskdiet.tar.gz.sha256" 2>/dev/null && {
  expected="$(cut -d' ' -f1 < "$tmp/diskdiet.tar.gz.sha256")"
  if command -v shasum >/dev/null; then actual="$(shasum -a 256 "$tmp/diskdiet.tar.gz" | cut -d' ' -f1)"
  else actual="$(sha256sum "$tmp/diskdiet.tar.gz" | cut -d' ' -f1)"; fi
  [ "$expected" = "$actual" ] || { echo "diskdiet: checksum mismatch, aborting" >&2; exit 1; }
}
tar -xzf "$tmp/diskdiet.tar.gz" -C "$tmp"
mkdir -p "$DIR"
mv "$tmp/diskdiet" "$DIR/diskdiet"
chmod +x "$DIR/diskdiet"

echo "Installed diskdiet to $DIR/diskdiet"
case ":$PATH:" in
  *":$DIR:"*) echo "Run: diskdiet" ;;
  *) echo "Add $DIR to your PATH, then run: diskdiet" ;;
esac
