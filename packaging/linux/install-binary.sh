#!/usr/bin/env bash
# Install a locally built BIMCraft over the packaged /usr/bin/bimcraft.
#
# Run as root: sudo bash packaging/linux/install-binary.sh
#
# Copies target/release/bimcraft (override: BIMCRAFT_BINARY) into /usr/bin/bimcraft:
# the installed file is backed up beside it first, then replaced atomically with
# a temporary file in the same directory and `mv -f`, so a running BIMCraft keeps
# its old inode instead of hitting ETXTBSY. Finishes with a self-check of the
# release, its permissions and its size, printed under VERIFICATION.
#
# Needs Bash 4 or later. Close BIMCraft before running this: an open process keeps
# running the old binary, and unsaved edits are lost if it is killed instead.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
TARGET=/usr/bin/bimcraft
SOURCE="${BIMCRAFT_BINARY:-$ROOT/target/release/bimcraft}"

# Keep the terminal open after a double click, on success and on any failure.
pause_on_exit() {
  local code
  code=$?
  printf '\nPress Enter to close this window...'
  read -r _ || true
  exit "$code"
}
trap pause_on_exit EXIT

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

if [ "$(id -u)" -ne 0 ]; then
  fail "run this script as root: sudo bash $0"
fi
[ -f "$SOURCE" ] || fail "no binary at $SOURCE: build it first with cargo build --release -p bimcraft, or set BIMCRAFT_BINARY"

EXPECTED_VERSION="$("$SOURCE" --version 2>/dev/null)" || fail "$SOURCE --version failed: the binary is broken"
[ -n "$EXPECTED_VERSION" ] || fail "$SOURCE --version printed nothing"

BACKUP="$TARGET.bak-$(date +%Y%m%d-%H%M%S)"
if [ -f "$TARGET" ]; then
  cp --preserve=mode,timestamps "$TARGET" "$BACKUP" || fail "could not back up $TARGET"
  printf 'Backed up %s to %s\n' "$TARGET" "$BACKUP"
else
  printf 'No installed binary at %s: installing fresh\n' "$TARGET"
fi

TMP="$(mktemp "$TARGET.XXXXXX")" || fail "could not make a temporary file beside $TARGET"
# install(1) copies the source; the temporary file lives on the target's file system,
# so the rename below is atomic.
if ! install -m 0755 "$SOURCE" "$TMP"; then
  rm -f "$TMP"
  fail "could not write $TMP"
fi
if ! mv -f "$TMP" "$TARGET"; then
  rm -f "$TMP"
  fail "could not move $TMP over $TARGET"
fi

ACTUAL_VERSION="$("$TARGET" --version 2>/dev/null)" || fail "$TARGET does not run after the install"
MODE="$(stat -c '%a' "$TARGET")"
SOURCE_SIZE="$(stat -c '%s' "$SOURCE")"
TARGET_SIZE="$(stat -c '%s' "$TARGET")"

STATUS=0
printf '\nVERIFICATION\n'
check() {
  local ok=$1 label=$2 detail=$3
  if [ "$ok" -eq 0 ]; then
    printf '  ok    %s: %s\n' "$label" "$detail"
  else
    printf '  FAIL  %s: %s\n' "$label" "$detail"
    STATUS=1
  fi
}
if [ "$ACTUAL_VERSION" = "$EXPECTED_VERSION" ]; then v=0; else v=1; fi
check "$v" "version" "$ACTUAL_VERSION (expected $EXPECTED_VERSION)"
if [ "$MODE" = "755" ]; then p=0; else p=1; fi
check "$p" "permissions" "$MODE (expected 755)"
if [ "$TARGET_SIZE" = "$SOURCE_SIZE" ]; then s=0; else s=1; fi
check "$s" "size" "$TARGET_SIZE bytes (source $SOURCE_SIZE)"
if [ -f "$BACKUP" ]; then
  printf '  note  backup: %s\n' "$BACKUP"
fi

if [ "$STATUS" -ne 0 ]; then
  fail "the installed binary differs from the built one"
fi
printf '\nInstalled %s at %s\n' "$EXPECTED_VERSION" "$TARGET"
