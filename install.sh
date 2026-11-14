#!/bin/sh
# Install dfw without Node.js: download the single-file dfw binary for this machine from the
# GitHub release, verify its SHA-256, then let it register dfw and its skills with Claude Code.
#   curl -fsSL https://raw.githubusercontent.com/tantm97/ai-frontend-design-workflow/main/install.sh | sh
#   ... | sh -s -- --scope project          (arguments go to "dfw install")
#   DFW_VERSION=0.5.0 sh install.sh          (a specific release)
set -eu
REPO="tantm97/ai-frontend-design-workflow"

case "$(uname -s)" in
  Darwin) OS=darwin ;;
  Linux) OS=linux ;;
  MINGW* | MSYS* | CYGWIN*) OS=win32 ;;
  *) echo "dfw: unsupported OS $(uname -s)" >&2; exit 1 ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) ARCH=arm64 ;;
  x86_64 | amd64) ARCH=x64 ;;
  *) echo "dfw: unsupported CPU $(uname -m)" >&2; exit 1 ;;
esac

fetch() { # url -> stdout
  if command -v curl >/dev/null 2>&1; then curl -fsSL "$1"; else wget -qO- "$1"; fi
}

VERSION=${DFW_VERSION:-}
if [ -z "$VERSION" ]; then
  VERSION=$(fetch "https://api.github.com/repos/$REPO/releases/latest" | sed -n 's/.*"tag_name"[[:space:]]*:[[:space:]]*"v\{0,1\}\([^"]*\)".*/\1/p' | head -n 1)
fi
[ -n "$VERSION" ] || { echo "dfw: could not find the latest release of $REPO" >&2; exit 1; }

EXT=""
[ "$OS" = win32 ] && EXT=".exe"
NAME="dfw-$VERSION-$OS-$ARCH$EXT"
BASE="https://github.com/$REPO/releases/download/v$VERSION"
HOME_DIR=${DFW_HOME:-${XDG_CONFIG_HOME:-$HOME/.config}/dfw}
DEST_DIR="$HOME_DIR/bin/$VERSION"
DEST="$DEST_DIR/dfw$EXT"
TMP=$(mktemp -d 2>/dev/null || mktemp -d -t dfw)
trap 'rm -rf "$TMP"' EXIT INT TERM

echo "› dfw $VERSION for $OS-$ARCH"
fetch "$BASE/SHA256SUMS" > "$TMP/SHA256SUMS"
fetch "$BASE/$NAME" > "$TMP/$NAME"
WANT=$(grep " \*\{0,1\}$NAME\$" "$TMP/SHA256SUMS" | cut -d' ' -f1)
[ -n "$WANT" ] || { echo "dfw: release v$VERSION has no binary for $OS-$ARCH" >&2; exit 1; }
if command -v sha256sum >/dev/null 2>&1; then GOT=$(sha256sum "$TMP/$NAME" | cut -d' ' -f1); else GOT=$(shasum -a 256 "$TMP/$NAME" | cut -d' ' -f1); fi
[ "$WANT" = "$GOT" ] || { echo "dfw: checksum mismatch for $NAME; nothing installed" >&2; exit 1; }

mkdir -p "$DEST_DIR"
mv "$TMP/$NAME" "$DEST"
chmod 755 "$DEST"
echo "  installed $DEST"

# A "dfw" command on PATH for use outside Claude Code (Claude Code itself uses the plugin's bin/).
LINK_DIR="$HOME/.local/bin"
mkdir -p "$LINK_DIR"
ln -sf "$DEST" "$LINK_DIR/dfw$EXT"
case ":$PATH:" in *":$LINK_DIR:"*) ;; *) echo "  add $LINK_DIR to PATH to run dfw from any shell" ;; esac

exec "$DEST" install "$@"
