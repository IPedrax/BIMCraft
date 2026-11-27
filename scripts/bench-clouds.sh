#!/usr/bin/env bash
# Measures how long bringing in each scan of a corpus takes, and the most memory it takes: the numbers in the point cloud PRs.
#
#   scripts/bench-clouds.sh [DIR] [POINTS]
#
# Makes the synthetic corpus in DIR (target/cloud-corpus unless given; 10 million points unless given) with the `cloud_corpus`
# example, unless it's there, then runs `bimcraft-cli run --cmd 'point_cloud {...}'` on each file, release build. Linux and WSL
# need GNU time (/usr/bin/time); on Windows run scripts/bench-clouds.ps1. Any other scan files in DIR are measured too.
set -euo pipefail
cd "$(dirname "$0")/.."
dir="${1:-target/cloud-corpus}"
points="${2:-10000000}"
cargo build --release -p bimcraft-cli >&2
cargo build --release -p bimcraft-core --example cloud_corpus >&2
if [ ! -f "$dir/corpus.laz" ]; then
  target/release/examples/cloud_corpus "$dir" "$points" >&2
fi
printf '%-28s %12s %10s %12s %12s\n' file bytes seconds 'peak MB' points
for f in "$dir"/*; do
  case "${f,,}" in
    *.pts|*.xyz|*.ply|*.las|*.laz|*.e57) ;;
    *) continue ;;
  esac
  out=$(mktemp)
  /usr/bin/time -f '%e %M' -o "$out.time" target/release/bimcraft-cli run \
    --cmd "point_cloud {\"path\": \"$f\", \"units\": \"m\"}" > "$out" 2>&1 || {
    echo "$f: failed" >&2
    cat "$out" >&2
    continue
  }
  read -r secs kb < "$out.time"
  kept=$(grep -o '"points":[0-9]*' "$out" | head -1 | cut -d: -f2)
  printf '%-28s %12s %10s %12s %12s\n' "$(basename "$f")" "$(stat -c %s "$f")" "$secs" "$((kb / 1024))" "${kept:-?}"
  rm -f "$out" "$out.time"
done
