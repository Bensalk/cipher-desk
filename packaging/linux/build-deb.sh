#!/usr/bin/env bash
# Builds the .deb package from a Flutter Linux release bundle.
#   bash packaging/linux/build-deb.sh <bundle dir> <version> <output dir>
# Installs to /opt/secure-rdp-viewer with a launcher, app-menu entry and icons.
# Uninstalling removes all of it. Saved passwords live in each user's keyring,
# which a package cannot reach: users remove them in Settings first (see
# docs/USER_GUIDE.md).
set -euo pipefail
BUNDLE=$(cd "$1" && pwd); VERSION=$2; OUT=$(mkdir -p "$3" && cd "$3" && pwd)
HERE=$(cd "$(dirname "$0")" && pwd)
case "$VERSION" in (*[!0-9.]*|"") echo "bad version: $VERSION" >&2; exit 1;; esac
[ -x "$BUNDLE/secure_rdp_viewer" ] || { echo "no secure_rdp_viewer in $BUNDLE" >&2; exit 1; }
ARCH=$(dpkg --print-architecture)
# The project's public commit identity (a GitHub noreply address, SA-9).
REPO_DIR=$(cd "$HERE/../.." && pwd)
MAINTAINER=${DEB_MAINTAINER:-$(git -C "$REPO_DIR" log -1 --format='%an <%ae>')}
HOMEPAGE=${REPO_URL:+Homepage: $REPO_URL}

ROOT=$(mktemp -d); trap 'rm -rf "$ROOT"' EXIT
install -d "$ROOT/DEBIAN" "$ROOT/opt/secure-rdp-viewer" "$ROOT/usr/bin" \
  "$ROOT/usr/share/applications" "$ROOT/usr/share/metainfo" "$ROOT/usr/share/doc/secure-rdp-viewer"
cp -a "$BUNDLE/." "$ROOT/opt/secure-rdp-viewer/"
ln -s /opt/secure-rdp-viewer/secure_rdp_viewer "$ROOT/usr/bin/secure-rdp-viewer"
install -m 644 "$HERE/secure-rdp-viewer.desktop" "$ROOT/usr/share/applications/"
install -m 644 "$HERE/io.github.securerdpviewer.app.metainfo.xml" "$ROOT/usr/share/metainfo/"
for size in 64x64 128x128 256x256 512x512; do
  install -D -m 644 "$HERE/icons/$size/secure-rdp-viewer.png" \
    "$ROOT/usr/share/icons/hicolor/$size/apps/secure-rdp-viewer.png"
done
install -D -m 644 "$HERE/icons/scalable/secure-rdp-viewer.svg" \
  "$ROOT/usr/share/icons/hicolor/scalable/apps/secure-rdp-viewer.svg"
{ echo "Secure RDP Viewer — Apache-2.0"; echo; cat "$REPO_DIR/NOTICE"; } > "$ROOT/usr/share/doc/secure-rdp-viewer/copyright"
# Files root-owned and not writable by others.
find "$ROOT" -type d -exec chmod 755 {} +
find "$ROOT/opt" -type f -exec chmod go-w {} +

SIZE=$(du -sk --exclude=DEBIAN "$ROOT" | cut -f1)
cat > "$ROOT/DEBIAN/control" <<CONTROL
Package: secure-rdp-viewer
Version: $VERSION
Architecture: $ARCH
Maintainer: $MAINTAINER
Installed-Size: $SIZE
Depends: libgtk-3-0t64 | libgtk-3-0, libstdc++6
Recommends: gnome-keyring | kwalletmanager | keepassxc
Section: net
Priority: optional
$HOMEPAGE
Description: Secure Remote Desktop (RDP) viewer
 Connects to Windows computers with Remote Desktop using NLA and TLS only,
 with certificate pinning. Passwords are kept only in the system keyring.
CONTROL
sed -i '/^$/d' "$ROOT/DEBIAN/control"
dpkg-deb --root-owner-group --build "$ROOT" "$OUT/secure-rdp-viewer_${VERSION}_${ARCH}.deb"
