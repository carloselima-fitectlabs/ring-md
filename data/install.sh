#!/bin/sh
# Install RingMD under PREFIX (default /usr/local): the binary, its bundled
# Neovim runtime, the desktop file, AppStream metainfo and icons.
#
#   cargo build --release
#   ./data/install.sh ~/.local                # per user
#   sudo ./data/install.sh /usr/local         # system-wide
#   ./data/install.sh --uninstall ~/.local    # remove it again
#
# Neovim 0.10+ must be on PATH (the Flatpak bundles it).
set -eu

id=io.github.ringmd.RingMD

uninstall=false
if [ "${1:-}" = "--uninstall" ]; then
  uninstall=true
  shift
fi

prefix=${1:-/usr/local}
binary=${2:-target/release/ringmd}
here=$(dirname "$0")
root="$here/.."

desktop="$prefix/share/applications/$id.desktop"
metainfo="$prefix/share/metainfo/$id.metainfo.xml"
icon="$prefix/share/icons/hicolor/scalable/apps/$id.svg"
symbolic="$prefix/share/icons/hicolor/symbolic/apps/$id-symbolic.svg"

# Let the desktop pick up the new entry and icon without a re-login.
refresh_caches() {
  if command -v update-desktop-database >/dev/null 2>&1; then
    update-desktop-database -q "$prefix/share/applications" 2>/dev/null || true
  fi
  if command -v gtk-update-icon-cache >/dev/null 2>&1 &&
    [ -f "$prefix/share/icons/hicolor/index.theme" ]; then
    gtk-update-icon-cache -q -t "$prefix/share/icons/hicolor" 2>/dev/null || true
  fi
}

if $uninstall; then
  rm -f "$prefix/bin/ringmd" "$desktop" "$metainfo" "$icon" "$symbolic"
  rm -rf "$prefix/share/ringmd"
  refresh_caches
  echo "Removed RingMD from $prefix"
  echo "Your settings in ~/.config/ringmd were left in place."
  exit 0
fi

if [ ! -x "$binary" ]; then
  echo "error: $binary not found; run 'cargo build --release' first" >&2
  exit 1
fi

install -Dm755 "$binary" "$prefix/bin/ringmd"
mkdir -p "$prefix/share/ringmd"
rm -rf "$prefix/share/ringmd/runtime"
cp -r "$root/runtime" "$prefix/share/ringmd/runtime"
install -Dm644 "$here/$id.desktop" "$desktop"
install -Dm644 "$here/$id.metainfo.xml" "$metainfo"
install -Dm644 "$here/icons/hicolor/scalable/apps/$id.svg" "$icon"
install -Dm644 "$here/icons/hicolor/symbolic/apps/$id-symbolic.svg" "$symbolic"
refresh_caches

echo "Installed RingMD to $prefix"

# Warnings only: the install itself is done.
case ":$PATH:" in
  *":$prefix/bin:"*) ;;
  *) echo "note: $prefix/bin is not on PATH; add it to start 'ringmd' from a terminal" ;;
esac
if ! command -v nvim >/dev/null 2>&1; then
  echo "note: Neovim was not found on PATH; RingMD needs Neovim 0.10 or newer"
else
  version=$(nvim --version | sed -n '1s/^NVIM v\([0-9]*\)\.\([0-9]*\).*/\1 \2/p')
  set -- $version
  if [ "${1:-0}" -eq 0 ] && [ "${2:-0}" -lt 10 ]; then
    echo "note: $(nvim --version | head -1) is too old; RingMD needs Neovim 0.10 or newer"
  fi
fi
