#!/bin/sh
# Installs diskdiet.
#   curl -fsSL https://raw.githubusercontent.com/Baimmmss/diskdiet/master/install.sh | sh
# Downloads the prebuilt binary from the latest GitHub release. If there is no
# release binary for this machine yet, builds it from source with cargo instead.
# Set DISKDIET_INSTALL_DIR to choose where it goes (default: ~/.local/bin).
set -eu

REPO="Baimmmss/diskdiet"
DIR="${DISKDIET_INSTALL_DIR:-${HOME}/.local/bin}"

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

build_from_source() {
  if ! command -v cargo >/dev/null 2>&1; then
    echo "diskdiet: $1, and cargo is not installed to build it from source." >&2
    echo "Install Rust from https://rustup.rs, then run this again." >&2
    exit 1
  fi
  echo "$1; building from source with cargo (takes a minute)..."
  cargo install diskdiet --locked --root "${tmp}/root"
  mv "${tmp}/root/bin/diskdiet" "${tmp}/diskdiet"
}

target=""
case "$(uname -s)" in
  Darwin) os="apple-darwin" ;;
  Linux) os="unknown-linux-musl" ;;
  *) os="" ;;
esac
case "$(uname -m)" in
  x86_64 | amd64) arch="x86_64" ;;
  arm64 | aarch64) arch="aarch64" ;;
  *) arch="" ;;
esac
[ -n "${os}" ] && [ -n "${arch}" ] && target="${arch}-${os}"

if [ -z "${target}" ]; then
  build_from_source "no prebuilt binary for $(uname -s) $(uname -m)"
else
  base="https://github.com/${REPO}/releases/latest/download/diskdiet-${target}"
  echo "Downloading diskdiet for ${target}..."
  if curl -fsSL "${base}.tar.gz" -o "${tmp}/diskdiet.tar.gz" 2>/dev/null; then
    if curl -fsSL "${base}.sha256" -o "${tmp}/diskdiet.sha256" 2>/dev/null; then
      expected="$(cut -d' ' -f1 < "${tmp}/diskdiet.sha256")"
      if command -v shasum >/dev/null 2>&1; then actual="$(shasum -a 256 "${tmp}/diskdiet.tar.gz" | cut -d' ' -f1)"
      else actual="$(sha256sum "${tmp}/diskdiet.tar.gz" | cut -d' ' -f1)"; fi
      [ "${expected}" = "${actual}" ] || { echo "diskdiet: checksum mismatch, aborting" >&2; exit 1; }
    fi
    tar -xzf "${tmp}/diskdiet.tar.gz" -C "${tmp}"
  else
    build_from_source "no release binary for ${target} yet"
  fi
fi

mkdir -p "${DIR}"
mv "${tmp}/diskdiet" "${DIR}/diskdiet"
chmod +x "${DIR}/diskdiet"

echo "Installed diskdiet to ${DIR}/diskdiet"
case ":${PATH}:" in
  *":${DIR}:"*) echo "Run: diskdiet" ;;
  *) echo "Add ${DIR} to your PATH, then run: diskdiet" ;;
esac
