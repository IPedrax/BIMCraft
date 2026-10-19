#!/usr/bin/env bash
# Build and package BIMCraft for Linux (<arch> is x86_64 or aarch64, the host's):
#
#   $DIST/bimcraft-<version>-linux-<arch>.AppImage        any distro with glibc >= the build host's
#   $DIST/bimcraft-<version>-linux-<arch>.AppImage.zsync  its delta updates (needs zsyncmake)
#   $DIST/bimcraft-<version>-linux-<arch>.deb             Debian, Ubuntu, Mint, Pop!_OS, ...
#   $DIST/bimcraft-<version>-linux-<arch>.rpm             Fedora, openSUSE, RHEL, ...
#   $DIST/bimcraft-<version>-linux-<arch>.tar.gz          a plain tree (bin/, share/)
#
# Usage: packaging/linux/package.sh [--skip-build] [--formats "appimage deb rpm tar"]
#
# Needs: cargo; nfpm for the deb and the rpm (https://nfpm.goreleaser.com); appimagetool for the
# AppImage (downloaded into $CARGO_TARGET_DIR if missing). Build on an old distro (CI: Ubuntu
# 22.04, glibc 2.35) so the binaries run on newer ones. Optional: desktop-file-validate and
# appstreamcli check the metadata; zsyncmake (the zsync package) writes the AppImage's .zsync.
set -euo pipefail
# shellcheck source=../env.sh
. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"
HERE="$ROOT/packaging/linux"
APP_ID=io.github.ipedrax.bimcraft

SKIP_BUILD=0
FORMATS="appimage deb rpm tar"
while [ $# -gt 0 ]; do
  case "$1" in
    --skip-build) SKIP_BUILD=1; shift ;;
    --formats) FORMATS="$2"; shift 2 ;;
    -h | --help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done

ARCH="$(uname -m)"
case "$ARCH" in
  x86_64) DEB_ARCH=amd64 ;;
  aarch64 | arm64) ARCH=aarch64; DEB_ARCH=arm64 ;;
  *) echo "unsupported architecture $ARCH" >&2; exit 2 ;;
esac
export BIMCRAFT_MAINTAINER="${BIMCRAFT_MAINTAINER:-IPedrax <https://github.com/IPedrax/BIMCraft>}"
BASENAME="bimcraft-$VERSION-linux-$ARCH"

echo "==> BIMCraft $VERSION for Linux $ARCH ($FORMATS)"

if [ "$SKIP_BUILD" = 0 ]; then
  (cd "$ROOT" && cargo build --release --locked -p bimcraft -p bimcraft-cli)
fi
BIN="$CARGO_TARGET_DIR/release"
WORK="$CARGO_TARGET_DIR/linux-package"
STAGE="$WORK/root"
rm -rf "$WORK"

# ---- a tree every format is made from ----------------------------------------------------------
install -Dm755 "$BIN/bimcraft" "$STAGE/usr/bin/bimcraft"
install -Dm755 "$BIN/bimcraft-cli" "$STAGE/usr/bin/bimcraft-cli"
strip "$STAGE/usr/bin/bimcraft" "$STAGE/usr/bin/bimcraft-cli"
install -Dm644 "$HERE/$APP_ID.desktop" "$STAGE/usr/share/applications/$APP_ID.desktop"
install -Dm644 "$HERE/$APP_ID.mime.xml" "$STAGE/usr/share/mime/packages/$APP_ID.xml"
mkdir -p "$STAGE/usr/share/metainfo"
sed -e "s/@VERSION@/$VERSION/g" -e "s/@DATE@/$BUILD_DATE/g" \
  "$HERE/$APP_ID.metainfo.xml.in" >"$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
mkdir -p "$STAGE/usr/share/icons"
cp -R "$ROOT/assets/app-icon/hicolor" "$STAGE/usr/share/icons/"
mkdir -p "$STAGE/usr/share/doc/bimcraft"
copy_docs "$STAGE/usr/share/doc/bimcraft"

if command -v desktop-file-validate >/dev/null; then
  desktop-file-validate "$STAGE/usr/share/applications/$APP_ID.desktop"
fi
if command -v appstreamcli >/dev/null; then
  appstreamcli validate --no-net --explain "$STAGE/usr/share/metainfo/$APP_ID.metainfo.xml"
fi

has() { case " $FORMATS " in *" $1 "*) return 0 ;; *) return 1 ;; esac; }

# ---- .tar.gz -----------------------------------------------------------------------------------
if has tar; then
  mkdir -p "$WORK/tar"
  cp -R "$STAGE/usr" "$WORK/tar/$BASENAME"
  tar -C "$WORK/tar" -czf "$DIST/$BASENAME.tar.gz" "$BASENAME"
  echo "wrote $DIST/$BASENAME.tar.gz"
fi

# ---- .deb and .rpm -----------------------------------------------------------------------------
if has deb || has rpm; then
  command -v nfpm >/dev/null || { echo "error: nfpm not found (https://nfpm.goreleaser.com/install/)" >&2; exit 1; }
  export NFPM_ARCH="$DEB_ARCH"
  # nfpm expands environment variables in fields like `version` and `arch`, but not in `src`.
  sed "s|\${STAGE}|$STAGE|g" "$HERE/nfpm.yaml" >"$WORK/nfpm.yaml"
  for fmt in deb rpm; do
    if has "$fmt"; then (cd "$ROOT" && nfpm package -f "$WORK/nfpm.yaml" -p "$fmt" -t "$DIST/$BASENAME.$fmt"); fi
  done
fi

# ---- AppImage ----------------------------------------------------------------------------------
if has appimage; then
  APPDIR="$WORK/BIMCraft.AppDir"
  cp -R "$STAGE" "$APPDIR"
  rm -rf "$APPDIR/usr/share/doc"
  ln -s usr/bin/bimcraft "$APPDIR/AppRun"
  cp "$HERE/$APP_ID.desktop" "$APPDIR/$APP_ID.desktop"
  cp "$ROOT/assets/app-icon/hicolor/256x256/apps/$APP_ID.png" "$APPDIR/$APP_ID.png"
  ln -s "$APP_ID.png" "$APPDIR/.DirIcon"

  TOOL="${APPIMAGETOOL:-$(command -v appimagetool || true)}"
  if [ -z "$TOOL" ]; then
    TOOL="$CARGO_TARGET_DIR/appimagetool-$ARCH.AppImage"
    if [ ! -x "$TOOL" ]; then
      curl -fsSL -o "$TOOL" "https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-$ARCH.AppImage"
      chmod +x "$TOOL"
    fi
  fi
  TOOL="$(cd "$(dirname "$TOOL")" && pwd)/$(basename "$TOOL")"
  OUT="$DIST/$BASENAME.AppImage"
  # A .zsync left by an earlier run would hide a missing zsyncmake and describe another file.
  rm -f "$OUT.zsync"
  # Update information: AppImageUpdate, AppImageLauncher and the like read it from the file and
  # fetch only the blocks that changed in a newer release, through the .zsync published beside
  # each AppImage. `latest` is the newest release that isn't a pre-release; a fork's builds
  # point at its own releases through GITHUB_REPOSITORY.
  REPO="${GITHUB_REPOSITORY:-IPedrax/BIMCraft}"
  UPDATE_INFO="gh-releases-zsync|${REPO%%/*}|${REPO#*/}|latest|bimcraft-*-linux-$ARCH.AppImage.zsync"
  # Extract-and-run works without FUSE (containers, CI); the AppImage it writes embeds the static
  # runtime, so users don't need libfuse2 either. appimagetool writes the .zsync, when zsyncmake
  # is there, into the directory it runs in, hence the cd.
  (cd "$DIST" && ARCH="$ARCH" APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream -u "$UPDATE_INFO" "$APPDIR" "$OUT")
  echo "wrote $OUT"
  if [ -s "$OUT.zsync" ]; then
    echo "wrote $OUT.zsync"
  else
    warn "zsyncmake not found, so $OUT.zsync was not written; AppImage delta updates need it"
  fi
fi

"$STAGE/usr/bin/bimcraft-cli" --version
"$STAGE/usr/bin/bimcraft" --version
echo "==> done"
ls -lh "$DIST"
