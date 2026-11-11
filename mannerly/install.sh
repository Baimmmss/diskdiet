#!/bin/sh
# Installs a mannerly release binary.
#   curl -fsSL https://raw.githubusercontent.com/Baimmmss/mannerly/main/install.sh | sh
# MANNERLY_INSTALL_DIR chooses where it goes (default: ~/.local/bin).
# MANNERLY_VERSION picks a release tag like v0.1.0 (default: latest).
set -eu

REPO="Baimmmss/mannerly"
DIR="${MANNERLY_INSTALL_DIR:-$HOME/.local/bin}"
VERSION="${MANNERLY_VERSION:-latest}"

case "$(uname -s)" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-musl" ;;
  *) echo "mannerly: unsupported OS $(uname -s); try: cargo install mannerly" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *) echo "mannerly: unsupported CPU $(uname -m); try: cargo install mannerly" >&2; exit 1 ;;
esac

if [ "$VERSION" = "latest" ]; then
  base="https://github.com/$REPO/releases/latest/download"
else
  base="https://github.com/$REPO/releases/download/$VERSION"
fi
target="$arch-$os"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

echo "Downloading mannerly ($VERSION) for $target…"
curl -fsSL "$base/mannerly-$target.tar.gz" -o "$tmp/mannerly.tar.gz"
if curl -fsSL "$base/mannerly-$target.sha256" -o "$tmp/sum" 2>/dev/null; then
  expected="$(cut -d' ' -f1 < "$tmp/sum")"
  if command -v shasum >/dev/null; then actual="$(shasum -a 256 "$tmp/mannerly.tar.gz" | cut -d' ' -f1)"
  else actual="$(sha256sum "$tmp/mannerly.tar.gz" | cut -d' ' -f1)"; fi
  [ "$expected" = "$actual" ] || { echo "mannerly: checksum mismatch, aborting" >&2; exit 1; }
fi
tar -xzf "$tmp/mannerly.tar.gz" -C "$tmp"
mkdir -p "$DIR"
mv "$tmp/mannerly" "$DIR/mannerly"
chmod +x "$DIR/mannerly"

echo "Installed mannerly to $DIR/mannerly"
case ":$PATH:" in
  *":$DIR:"*) echo "Try: mannerly git" ;;
  *) echo "Add $DIR to your PATH, then try: mannerly git" ;;
esac
