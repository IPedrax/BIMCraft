#!/usr/bin/env bash
# Every UI of docs/areas/rune-ui.md's measurement, twice over, one after another on the same machine (a shared runner varies, so a
# single run proves little). usage: ci/measure-all.sh BINS RESULTS   (DISPLAY names a running X server)
set -euo pipefail

bins=$1
out=$2
rune="$bins/bimcraft-rune/bimcraft-rune"
egui_main="$bins/bimcraft-egui-main/bimcraft-egui-main"
egui_join="$bins/bimcraft-egui-join-628/bimcraft-egui-join-628"

for round in 1 2; do
    ci/measure-ui.sh "rune-gpu-$round" "$out" -- "$rune"
    RUNE_CPU_FIRST=only ci/measure-ui.sh "rune-cpu-$round" "$out" -- "$rune"
    BIMCRAFT_RUNE_PLAN=pixels ci/measure-ui.sh "rune-gpu-pixels-$round" "$out" -- "$rune"
    BIMCRAFT_RUNE_PLAN=pixels RUNE_CPU_FIRST=only ci/measure-ui.sh "rune-cpu-pixels-$round" "$out" -- "$rune"
    PAN_BUTTON=2 ci/measure-ui.sh "egui-main-$round" "$out" -- "$egui_main" --sample
    if [ -x "$egui_join" ]; then
        PAN_BUTTON=2 BIMCRAFT_SOFTWARE=1 ci/measure-ui.sh "egui-join-628-$round" "$out" -- "$egui_join" --sample
    fi
done
