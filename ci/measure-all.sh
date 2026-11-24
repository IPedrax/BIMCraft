#!/usr/bin/env bash
# Every UI of docs/areas/rune-ui.md's measurement, twice over, one after another on the same machine (a shared runner varies, so a
# single run proves little), and egui's own frame timings (`--bench`). usage: ci/measure-all.sh BINS RESULTS   (DISPLAY: an X server)
set -euo pipefail

bins=$1
out=$2
rune="$bins/bimcraft-rune/bimcraft-rune"
egui_main="$bins/bimcraft-egui-main/bimcraft-egui-main"
egui_join="$bins/bimcraft-egui-join-628/bimcraft-egui-join-628"
mkdir -p "$out"

# A run that fails to find its window, or whose window goes while it is being moved, is tried again, up to three times.
try() {
    for attempt in 1 2 3; do
        "$@" && return 0
        echo "attempt $attempt failed: $*" >&2
        sleep 3
    done
    return 0
}

# egui's `--bench`: every frame timed on the real window, with its own HOME so no settings of a run reach the next.
bench() {
    local name=$1
    shift
    local home
    home=$(mktemp -d)
    HOME="$home" XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" XDG_CACHE_HOME="$home/cache" timeout 300 "$@" --sample \
        --bench ci/egui-bench.scenario --bench-out "$out/$name.json" >"$out/$name.log" 2>&1 || true
}

for round in 1 2; do
    try ci/measure-ui.sh "rune-gpu-$round" "$out" -- "$rune"
    try env RUNE_CPU_FIRST=only ci/measure-ui.sh "rune-cpu-$round" "$out" -- "$rune"
    try env BIMCRAFT_RUNE_PLAN=pixels ci/measure-ui.sh "rune-gpu-pixels-$round" "$out" -- "$rune"
    try env BIMCRAFT_RUNE_PLAN=pixels RUNE_CPU_FIRST=only ci/measure-ui.sh "rune-cpu-pixels-$round" "$out" -- "$rune"
    try env PAN_BUTTON=2 ci/measure-ui.sh "egui-main-$round" "$out" -- "$egui_main" --sample
    bench "egui-main-bench-$round" "$egui_main"
    if [ -x "$egui_join" ]; then
        try env PAN_BUTTON=2 BIMCRAFT_SOFTWARE=1 ci/measure-ui.sh "egui-join-628-$round" "$out" -- "$egui_join" --sample
        BIMCRAFT_SOFTWARE=1 bench "egui-join-628-bench-$round" "$egui_join"
    fi
done
