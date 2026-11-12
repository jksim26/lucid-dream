#!/bin/sh
# Install lucid-dream for the current user. No root needed.
#   curl -fsSL https://github.com/jksim26/lucid-dream/releases/latest/download/install.sh | sh
# Downloads the binary for this system, checks it against the release's
# SHA-256 checksums, and puts it in ~/.local/bin (or $LUCID_INSTALL_DIR).
set -eu

REPO="jksim26/lucid-dream"
BASE="https://github.com/$REPO/releases/latest/download"
DIR="${LUCID_INSTALL_DIR:-$HOME/.local/bin}"

case "$(uname -s)-$(uname -m)" in
  Darwin-arm64) ASSET="lucid-dream-macos-arm64" ;;
  Linux-x86_64) ASSET="lucid-dream-linux-x64" ;;
  *)
    echo "No prebuilt binary for $(uname -s) $(uname -m)." >&2
    echo "Build it instead: cargo install --git https://github.com/$REPO" >&2
    exit 1
    ;;
esac

TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

echo "Downloading $ASSET"
curl -fsSL "$BASE/$ASSET" -o "$TMP/$ASSET"
curl -fsSL "$BASE/SHA256SUMS" -o "$TMP/SHA256SUMS"

WANT="$(grep " $ASSET\$" "$TMP/SHA256SUMS" | cut -d' ' -f1)"
if command -v sha256sum >/dev/null 2>&1; then
  GOT="$(sha256sum "$TMP/$ASSET" | cut -d' ' -f1)"
else
  GOT="$(shasum -a 256 "$TMP/$ASSET" | cut -d' ' -f1)"
fi
if [ -z "$WANT" ] || [ "$WANT" != "$GOT" ]; then
  echo "Checksum does not match; nothing was installed." >&2
  exit 1
fi

mkdir -p "$DIR"
chmod +x "$TMP/$ASSET"
mv "$TMP/$ASSET" "$DIR/lucid-dream"
echo "Installed $("$DIR/lucid-dream" --version) to $DIR/lucid-dream"
case ":$PATH:" in
  *":$DIR:"*) ;;
  *) echo "Add $DIR to your PATH to run it as lucid-dream." ;;
esac
