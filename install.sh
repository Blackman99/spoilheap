#!/bin/sh
# spoilheap installer: downloads the release binary for this machine, checks its
# SHA-256 against checksums.txt, and puts it in ~/.local/bin (or $SPOILHEAP_BIN).
#   curl -fsSL https://blackman99.github.io/spoilheap/install.sh | sh
set -eu
repo=Blackman99/spoilheap
dest=${SPOILHEAP_BIN:-"$HOME/.local/bin"}
version=${SPOILHEAP_VERSION:-}

say() { printf 'spoilheap-install: %s\n' "$*"; }
die() { say "$*" >&2; exit 1; }

case "$(uname -s)" in
  Linux) os=unknown-linux-musl ;;
  Darwin) os=apple-darwin ;;
  *) die "no prebuilt binary for $(uname -s); on Windows download the .zip from https://github.com/$repo/releases, or run: cargo install --git https://github.com/$repo" ;;
esac
case "$(uname -m)" in
  x86_64|amd64) arch=x86_64 ;;
  arm64|aarch64) arch=aarch64 ;;
  *) die "no prebuilt binary for $(uname -m); try: cargo install --git https://github.com/$repo" ;;
esac
target="$arch-$os"

if [ -z "$version" ]; then
  version=$(curl -fsSL "https://api.github.com/repos/$repo/releases/latest" | sed -n 's/.*"tag_name": *"\([^"]*\)".*/\1/p' | head -n1)
  [ -n "$version" ] || die "could not find the latest release (GitHub API rate limit?); set SPOILHEAP_VERSION=v0.1.0"
fi
name="spoilheap-$version-$target"
base="https://github.com/$repo/releases/download/$version"
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT

say "downloading $name.tar.gz"
curl -fsSL "$base/$name.tar.gz" -o "$tmp/$name.tar.gz" || die "download failed: $base/$name.tar.gz"
curl -fsSL "$base/checksums.txt" -o "$tmp/checksums.txt" || die "could not fetch checksums.txt"
want=$(grep " $name.tar.gz\$" "$tmp/checksums.txt" | cut -d' ' -f1)
if command -v sha256sum >/dev/null 2>&1; then got=$(sha256sum "$tmp/$name.tar.gz" | cut -d' ' -f1)
else got=$(shasum -a 256 "$tmp/$name.tar.gz" | cut -d' ' -f1); fi
[ -n "$want" ] && [ "$want" = "$got" ] || die "checksum mismatch for $name.tar.gz (want $want, got $got); nothing installed"

tar xzf "$tmp/$name.tar.gz" -C "$tmp"
mkdir -p "$dest"
mv "$tmp/$name/spoilheap" "$dest/spoilheap"
chmod +x "$dest/spoilheap"
say "installed $("$dest/spoilheap" version) to $dest/spoilheap (checksum ok)"
case ":$PATH:" in *":$dest:"*) ;; *) say "add $dest to your PATH to run it as 'spoilheap'" ;; esac
say "next: run 'spoilheap' to see what your coding agents left behind (it deletes nothing)"
