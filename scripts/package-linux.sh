#!/usr/bin/env bash
# Builds reframe for Linux and packs it as 0.x's Linux releases were: dist/reframe-<version>-x86_64.AppImage
# (it updates itself), dist/reframe_<version>_amd64.deb (updated by installing the next one), and
# dist/latest-linux.yml, which the AppImages' updaters read (reframe's own and electron-updater in 0.x).
# Needs: cargo, dpkg-deb, and appimagetool (APPIMAGETOOL, the PATH, or it is downloaded).
# Usage: scripts/package-linux.sh
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' Cargo.toml | head -1)"
target="${CARGO_TARGET_DIR:-$root/target}"
dist="$root/dist"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT
mkdir -p "$dist"

cargo build --release -p reframe
binary="$work/reframe"
cp "$target/release/reframe" "$binary"
strip "$binary"
icon="$root/crates/reframe/assets/reframe-icon-256.png"
comment="Keep your Claude Code sessions side by side in one window"

# The desktop entry 0.x had (its file name and StartupWMClass match the windows' app id).
desktop_entry() {
  cat <<EOF
[Desktop Entry]
Name=reframe
Exec=$1 %U
Terminal=false
Type=Application
Icon=reframe
StartupWMClass=reframe
Comment=$comment
Categories=Development;
EOF
}

# ---------- AppImage ----------
appdir="$work/reframe.AppDir"
mkdir -p "$appdir/usr/bin" "$appdir/usr/share/icons/hicolor/256x256/apps" "$appdir/usr/share/applications"
cp "$binary" "$appdir/usr/bin/reframe"
cp "$icon" "$appdir/usr/share/icons/hicolor/256x256/apps/reframe.png"
cp "$icon" "$appdir/reframe.png"
cp "$icon" "$appdir/.DirIcon"
{ desktop_entry reframe; echo "X-AppImage-Version=$version"; } > "$appdir/reframe.desktop"
cp "$appdir/reframe.desktop" "$appdir/usr/share/applications/reframe.desktop"
# Only starts reframe: nothing of the AppImage's (no library path) reaches the sessions. 0.x's
# updater runs a new AppImage with APPIMAGE_EXIT_AFTER_INSTALL when it installs it on quit: then
# it only ends, as 0.x's did.
cat > "$appdir/AppRun" <<'EOF'
#!/bin/sh
if [ -n "$APPIMAGE_EXIT_AFTER_INSTALL" ]; then
  exit 0
fi
here="$(dirname "$(readlink -f "$0")")"
exec "$here/usr/bin/reframe" "$@"
EOF
chmod 755 "$appdir/AppRun"

tool="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [ -z "$tool" ]; then
  tool="$work/appimagetool"
  curl -fsSL -o "$tool" \
    https://github.com/AppImage/appimagetool/releases/download/1.9.1/appimagetool-x86_64.AppImage
  chmod 755 "$tool"
fi
appimage="$dist/reframe-$version-x86_64.AppImage"
# Without FUSE (CI, containers) the tool runs from its unpacked self.
APPIMAGE_EXTRACT_AND_RUN=1 ARCH=x86_64 "$tool" --no-appstream "$appdir" "$appimage"

# ---------- .deb ----------
deb="$work/deb"
mkdir -p "$deb/DEBIAN" "$deb/opt/reframe" "$deb/usr/bin" "$deb/usr/share/applications" \
  "$deb/usr/share/icons/hicolor/256x256/apps"
cp "$binary" "$deb/opt/reframe/reframe"
cp "$icon" "$deb/usr/share/icons/hicolor/256x256/apps/reframe.png"
desktop_entry /opt/reframe/reframe > "$deb/usr/share/applications/reframe.desktop"
size="$(du -sk --exclude=DEBIAN "$deb" | cut -f1)"
cat > "$deb/DEBIAN/control" <<EOF
Package: reframe
Version: $version
Architecture: amd64
Maintainer: reframe Team
Installed-Size: $size
Depends: libc6 (>= 2.35), libgcc-s1, libxcb1, libxkbcommon0, libxkbcommon-x11-0, libwayland-client0, libvulkan1 | libegl1
Recommends: xdg-desktop-portal, mesa-vulkan-drivers
Section: devel
Priority: optional
Homepage: https://github.com/TheyCallMeJuzou/reframe
Description: Claude Code sessions side by side
 $comment: every session in a tile, notified when one needs you, resumed
 after a restart.
EOF
# /usr/bin/reframe goes through update-alternatives, as with 0.x's package (whose scripts remove
# it when that package goes).
cat > "$deb/DEBIAN/postinst" <<'EOF'
#!/bin/sh
set -e
if command -v update-alternatives >/dev/null 2>&1; then
  if [ -L /usr/bin/reframe ] && [ "$(readlink /usr/bin/reframe)" != /etc/alternatives/reframe ]; then
    rm -f /usr/bin/reframe
  fi
  update-alternatives --install /usr/bin/reframe reframe /opt/reframe/reframe 100 ||
    ln -sf /opt/reframe/reframe /usr/bin/reframe
else
  ln -sf /opt/reframe/reframe /usr/bin/reframe
fi
# 0.x's Chromium needed an AppArmor profile for its sandbox; reframe has no sandbox to allow.
if [ -f /etc/apparmor.d/reframe ] && grep -q chrome-sandbox /etc/apparmor.d/reframe 2>/dev/null; then
  if command -v apparmor_parser >/dev/null 2>&1; then apparmor_parser --remove /etc/apparmor.d/reframe 2>/dev/null || true; fi
  rm -f /etc/apparmor.d/reframe
fi
if command -v update-desktop-database >/dev/null 2>&1; then update-desktop-database /usr/share/applications || true; fi
EOF
cat > "$deb/DEBIAN/postrm" <<'EOF'
#!/bin/sh
set -e
if [ "$1" = remove ] || [ "$1" = purge ]; then
  if command -v update-alternatives >/dev/null 2>&1; then
    update-alternatives --remove reframe /opt/reframe/reframe || true
  else
    rm -f /usr/bin/reframe
  fi
fi
EOF
chmod 755 "$deb/DEBIAN/postinst" "$deb/DEBIAN/postrm"
rmdir "$deb/usr/bin"
dpkg-deb --root-owner-group --build "$deb" "$dist/reframe_${version}_amd64.deb"

# ---------- latest-linux.yml ----------
name="$(basename "$appimage")"
sha512="$(openssl dgst -sha512 -binary "$appimage" | base64 -w0)"
bytes="$(stat -c %s "$appimage")"
date="$(date -u +%Y-%m-%dT%H:%M:%S.000Z)"
printf "version: %s\nfiles:\n  - url: %s\n    sha512: %s\n    size: %s\npath: %s\nsha512: %s\nreleaseDate: '%s'\n" \
  "$version" "$name" "$sha512" "$bytes" "$name" "$sha512" "$date" > "$dist/latest-linux.yml"

ls -l "$dist"
