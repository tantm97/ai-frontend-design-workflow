#!/bin/sh
# Install dfw (no Node.js needed): download the native dfw binary for this machine from the GitHub
# release, verify its SHA-256 against the release's SHA256SUMS, put it in ~/.config/dfw/bin/<version>/,
# then let it register dfw and its skills with Claude Code and GitHub Copilot ("dfw install").
#   curl -fsSL https://raw.githubusercontent.com/tantm97/ai-frontend-design-workflow/main/install.sh | sh
#   ... | sh -s -- --scope project          (arguments go to "dfw install")
#   DFW_VERSION=0.6.3 sh install.sh          (a specific release; 0.5.x is the last Node-based one)
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
# Only a "dfw" of dfw's own is replaced: another "dfw" there (a file, or a link elsewhere) is the
# user's and stays.
LINK_DIR="$HOME/.local/bin"
LINK="$LINK_DIR/dfw"
mkdir -p "$LINK_DIR"
OURS=no
if [ "$OS" = win32 ]; then
  # Git Bash's "ln -s" copies the file unless Windows allows symbolic links, and a copy is not a
  # link, so a later install could never replace it. A small script that runs the installed
  # binary does the same job; its second line says it is dfw's.
  MARK="# dfw: written by install.sh, replaced by the next install"
  if [ ! -e "$LINK" ] && [ ! -L "$LINK" ]; then
    OURS=yes
  elif [ -f "$LINK" ] && [ "$(sed -n 2p "$LINK")" = "$MARK" ]; then
    OURS=yes
  fi
  if [ "$OURS" = yes ]; then
    QUOTED=$(printf '%s' "$DEST" | sed "s/'/'\\\\''/g")
    printf '#!/bin/sh\n%s\nexec '"'"'%s'"'"' "$@"\n' "$MARK" "$QUOTED" > "$LINK"
    chmod 755 "$LINK"
  else
    echo "  $LINK is not dfw's; left as it is. To use this dfw from a shell: add $DEST_DIR to PATH"
  fi
else
  if [ ! -e "$LINK" ] && [ ! -L "$LINK" ]; then
    OURS=yes
  elif [ -L "$LINK" ]; then
    case "$(readlink "$LINK")" in "$HOME_DIR/bin/"*) OURS=yes ;; esac
    [ -e "$LINK" ] || OURS=yes
  fi
  if [ "$OURS" = yes ]; then
    rm -f "$LINK"
    ln -s "$DEST" "$LINK"
  else
    echo "  $LINK is not a dfw link; left as it is. To use this dfw from a shell: ln -sf \"$DEST\" \"$LINK\""
  fi
fi
case ":$PATH:" in *":$LINK_DIR:"*) ;; *) echo "  add $LINK_DIR to PATH to run dfw from any shell" ;; esac

exec "$DEST" install "$@"
