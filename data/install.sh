#!/bin/sh
# Install RingMD under PREFIX (default /usr/local): the binary, its bundled
# Neovim runtime, the desktop file, AppStream metainfo and icons.
#
#   cargo build --release
#   ./data/install.sh ~/.local            # per user
#   sudo ./data/install.sh /usr/local     # system-wide
#
# Neovim 0.10+ must be on PATH (the Flatpak bundles it).
set -eu

prefix=${1:-/usr/local}
binary=${2:-target/release/ringmd}
here=$(dirname "$0")
root="$here/.."
id=io.github.ringmd.RingMD

install -Dm755 "$binary" "$prefix/bin/ringmd"
mkdir -p "$prefix/share/ringmd"
rm -rf "$prefix/share/ringmd/runtime"
cp -r "$root/runtime" "$prefix/share/ringmd/runtime"
install -Dm644 "$here/$id.desktop" "$prefix/share/applications/$id.desktop"
install -Dm644 "$here/$id.metainfo.xml" "$prefix/share/metainfo/$id.metainfo.xml"
install -Dm644 "$here/icons/hicolor/scalable/apps/$id.svg" \
  "$prefix/share/icons/hicolor/scalable/apps/$id.svg"
install -Dm644 "$here/icons/hicolor/symbolic/apps/$id-symbolic.svg" \
  "$prefix/share/icons/hicolor/symbolic/apps/$id-symbolic.svg"

echo "Installed RingMD to $prefix"
