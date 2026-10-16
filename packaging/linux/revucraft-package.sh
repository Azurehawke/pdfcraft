#!/usr/bin/env bash
# Build and package RevuCraft for Linux x86_64. Writes, in $DIST (default: dist/revucraft):
#
#   RevuCraft-x86_64.AppImage        one file that runs on any distro with glibc >= the build
#                                    host's (CI: Ubuntu 22.04, glibc 2.35); chmod +x and run
#   revucraft_amd64.deb              Debian, Ubuntu, Mint, Pop!_OS...: menu entry, opens PDFs,
#                                    upgrades in place (version 1.0.<build>)
#   RevuCraft-linux-x86_64.tar.gz    the same files as a plain folder (bin/, share/)
#
# The binaries are PdfCraft's (`pdfcraft`, `pdfcraft-cli`) built from the revucraft branch and
# shipped under RevuCraft's names, as on Windows (packaging/windows/revucraft-package.ps1).
# REVUCRAFT_BUILD (set by CI) is the build number the update check compares.
#
# Usage: packaging/linux/revucraft-package.sh [--skip-build]
# Needs: cargo, curl and the network (OCR models, appimagetool), dpkg-deb.
set -euo pipefail
DIST="${DIST:-$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)/dist/revucraft}"
export DIST
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
APP_ID=com.azurehawke.revucraft

SKIP_BUILD=0
case "${1:-}" in
  --skip-build) SKIP_BUILD=1 ;;
  "") ;;
  *) echo "unknown argument: $1" >&2; exit 2 ;;
esac

BUILD="${REVUCRAFT_BUILD:-0}"
case "$BUILD" in
  '' | *[!0-9]*) echo "REVUCRAFT_BUILD must be a number, not '$BUILD'" >&2; exit 2 ;;
esac
[ "$(uname -m)" = x86_64 ] || { echo "this script packages x86_64 only" >&2; exit 2; }
DEB_VERSION="1.0.$BUILD"
echo "==> RevuCraft build $BUILD for Linux x86_64 (deb $DEB_VERSION)"

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release -p pdfcraft -p pdfcraft-cli)
fi
BIN="$CARGO_TARGET_DIR/release"
WORK="$CARGO_TARGET_DIR/revucraft-linux"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- stage an FHS tree (shared by every format) -------------------------------------------------
install -Dm755 "$BIN/pdfcraft" "$STAGE/usr/bin/revucraft"
install -Dm755 "$BIN/pdfcraft-cli" "$STAGE/usr/bin/revucraft-cli"
strip "$STAGE/usr/bin/revucraft" "$STAGE/usr/bin/revucraft-cli" 2>/dev/null || true
mkdir -p "$STAGE/usr/share/applications"
cat >"$STAGE/usr/share/applications/$APP_ID.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=RevuCraft
GenericName=Construction PDF Markup
Comment=Mark up, measure and draft on construction drawings
Exec=revucraft %U
TryExec=revucraft
Icon=$APP_ID
Terminal=false
StartupNotify=true
StartupWMClass=$APP_ID
Categories=Office;Graphics;Engineering;
Keywords=pdf;markup;measure;takeoff;drawing;plan;cad;construction;
MimeType=application/pdf;
EOF
install -Dm644 "$ROOT/assets/app-icon/revucraft.svg" "$STAGE/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg"
install -Dm644 "$ROOT/assets/app-icon/revucraft-256.png" "$STAGE/usr/share/icons/hicolor/256x256/apps/$APP_ID.png"
mkdir -p "$STAGE/usr/share/doc/revucraft"
for f in REVUCRAFT.md LICENSE-MIT LICENSE-APACHE; do
  if [ -f "$ROOT/$f" ]; then cp "$ROOT/$f" "$STAGE/usr/share/doc/revucraft/"; fi
done
# OCR models: the app looks in <bin>/../share/pdfcraft/models.
stage_models "$STAGE/usr/share/pdfcraft/models"
rm -f "$STAGE"/usr/share/pdfcraft/models/*.part

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi

# ---- .tar.gz ------------------------------------------------------------------------------------
mkdir -p "$WORK/tar"
cp -R "$STAGE/usr" "$WORK/tar/RevuCraft"
tar -C "$WORK/tar" -czf "$DIST/RevuCraft-linux-x86_64.tar.gz" RevuCraft
echo "wrote $DIST/RevuCraft-linux-x86_64.tar.gz"

# ---- .deb ---------------------------------------------------------------------------------------
DEB="$WORK/deb"
mkdir -p "$DEB/DEBIAN"
cp -R "$STAGE/usr" "$DEB/usr"
SIZE_KB="$(du -sk "$DEB/usr" | cut -f1)"
# The binaries link only glibc and libgcc_s; windowing and the GPU are loaded at run time, so
# they are dependencies here rather than linked libraries. File dialogs go through the desktop
# portal; printing shells out to CUPS.
cat >"$DEB/DEBIAN/control" <<EOF
Package: revucraft
Version: $DEB_VERSION
Architecture: amd64
Maintainer: Steve <noreply@github.com>
Installed-Size: $SIZE_KB
Depends: libc6 (>= 2.35), libgcc-s1, libxkbcommon0, libvulkan1 | libegl1
Recommends: xdg-desktop-portal, cups-client, libwayland-client0, libx11-6, libx11-xcb1, libxcursor1, libxi6, libxrandr2, mesa-vulkan-drivers
Section: graphics
Priority: optional
Homepage: https://github.com/Azurehawke/pdfcraft/tree/revucraft
Description: Construction PDF markup, measuring and drafting
 RevuCraft marks up, measures and drafts on construction drawings: feet-inch
 scales, layers, networking symbols with counts, photo pins, and CAD-style
 drawing by typed lengths and coordinates. Includes revucraft-cli.
EOF
install -m755 "$HERE/postinst.sh" "$DEB/DEBIAN/postinst"
install -m755 "$HERE/postinst.sh" "$DEB/DEBIAN/postrm"
dpkg-deb --root-owner-group --build "$DEB" "$DIST/revucraft_amd64.deb"
echo "wrote $DIST/revucraft_amd64.deb"

# ---- AppImage -----------------------------------------------------------------------------------
APPDIR="$WORK/RevuCraft.AppDir"
cp -R "$STAGE" "$APPDIR"
rm -rf "$APPDIR/usr/share/doc"
ln -s usr/bin/revucraft "$APPDIR/AppRun"
cp "$STAGE/usr/share/applications/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
cp "$ROOT/assets/app-icon/revucraft-256.png" "$APPDIR/$APP_ID.png"
ln -s "$APP_ID.png" "$APPDIR/.DirIcon"
TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
if [ -z "$TOOL" ]; then
  TOOL="$CARGO_TARGET_DIR/appimagetool-x86_64.AppImage"
  if [ ! -x "$TOOL" ]; then
    curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x "$TOOL"
  fi
fi
OUT="$(cd "$DIST" && pwd)/RevuCraft-x86_64.AppImage"
# Extract-and-run: works without FUSE (CI). The AppImage embeds the static runtime, so users
# don't need libfuse2 either.
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT"
echo "wrote $OUT"

"$STAGE/usr/bin/revucraft-cli" --version
echo "==> done"
ls -lh "$DIST"
