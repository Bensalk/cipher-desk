#!/usr/bin/env bash
# Builds the portable Linux AppImage from a Flutter Linux release bundle.
#   bash packaging/linux/build-appimage.sh <bundle dir> <version> <output dir> <appimagetool>
# Portable data (SA-10): create a folder named "<AppImage file name>.data" next
# to the AppImage to keep connections and settings there (never passwords).
set -euo pipefail
BUNDLE=$(cd "$1" && pwd); VERSION=$2; OUT=$(mkdir -p "$3" && cd "$3" && pwd); TOOL=$4
HERE=$(cd "$(dirname "$0")" && pwd)
[ -x "$BUNDLE/cipher_desk" ] || { echo "no cipher_desk in $BUNDLE" >&2; exit 1; }
APPDIR=$(mktemp -d)/CipherDesk.AppDir; trap 'rm -rf "$(dirname "$APPDIR")"' EXIT
install -d "$APPDIR/usr/bin" "$APPDIR/usr/share/metainfo" "$APPDIR/usr/share/applications"
cp -a "$BUNDLE/." "$APPDIR/usr/bin/"
install -m 644 "$HERE/cipher-desk.desktop" "$APPDIR/cipher-desk.desktop"
install -m 644 "$HERE/cipher-desk.desktop" "$APPDIR/usr/share/applications/"
install -m 644 "$HERE/io.github.bensalk.cipherdesk.metainfo.xml" "$APPDIR/usr/share/metainfo/"
install -m 644 "$HERE/icons/256x256/cipher-desk.png" "$APPDIR/cipher-desk.png"
ln -s cipher-desk.png "$APPDIR/.DirIcon"
cat > "$APPDIR/AppRun" <<'APPRUN'
#!/bin/sh
HERE=$(dirname "$(readlink -f "$0")")
exec "$HERE/usr/bin/cipher_desk" "$@"
APPRUN
chmod 755 "$APPDIR/AppRun"
ARCH=$(uname -m) "$TOOL" --no-appstream "$APPDIR" "$OUT/Cipher_Desk-${VERSION}-$(uname -m).AppImage"
