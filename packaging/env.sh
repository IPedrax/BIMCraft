# shellcheck shell=bash
# Shared setup for the packaging scripts. Source it: `. "$(dirname "${BASH_SOURCE[0]}")/../env.sh"`.
#
# Exports:
#   ROOT              workspace root
#   VERSION           [workspace.package] version from Cargo.toml (override: BIMCRAFT_VERSION)
#   DIST              where the packages go (default: $ROOT/dist/release)
#   BUILD_DATE        UTC build date, YYYY-MM-DD (AppStream's release date)
#   BUILD_SHA         the git commit, for the macOS bundle's Info.plist
#   CARGO_TARGET_DIR  cargo's target dir (default: $ROOT/target)

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
export ROOT

# The version lives in one place: `[workspace.package] version` in the root Cargo.toml.
workspace_version() {
  awk '
    /^\[/ { in_pkg = ($0 == "[workspace.package]"); next }
    in_pkg && $1 == "version" { gsub(/[" ]/, "", $3); print $3; exit }
  ' "$ROOT/Cargo.toml"
}

VERSION="${BIMCRAFT_VERSION:-$(workspace_version)}"
if [ -z "$VERSION" ]; then
  echo "error: could not read [workspace.package] version from $ROOT/Cargo.toml" >&2
  exit 1
fi
export VERSION

DIST="${DIST:-$ROOT/dist/release}"
mkdir -p "$DIST"
DIST="$(cd "$DIST" && pwd)"
export DIST

BUILD_SHA="${BUILD_SHA:-$(git -C "$ROOT" rev-parse HEAD 2>/dev/null || echo unknown)}"
export BUILD_SHA
export BUILD_DATE="${BUILD_DATE:-$(date -u +%Y-%m-%d)}"
CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target}"
mkdir -p "$CARGO_TARGET_DIR"
CARGO_TARGET_DIR="$(cd "$CARGO_TARGET_DIR" && pwd)"
export CARGO_TARGET_DIR

# A GitHub Actions warning (plain stderr outside Actions).
warn() {
  if [ -n "${GITHUB_ACTIONS:-}" ]; then echo "::warning::$*"; else echo "warning: $*" >&2; fi
}

# The readme and the licences, into a package's directory.
copy_docs() {
  local dest="$1" f
  for f in README.md LICENSE-MIT LICENSE-APACHE; do
    cp "$ROOT/$f" "$dest/"
  done
}
