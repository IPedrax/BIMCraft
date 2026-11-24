#!/usr/bin/env bash
# What a UI costs on a machine with no graphics card, the way a person would use it: start it under Xvfb on software Vulkan
# (lavapipe), let it settle, read the process's CPU time over an idle stretch, then over a stretch of the pointer moving over the
# plan (xdotool, about 50 moves a second), and keep a screenshot. docs/areas/rune-ui.md has the numbers and what they leave out.
#
# usage: ci/measure-ui.sh NAME OUTDIR -- COMMAND [ARGS...]      (DISPLAY names a running X server; IDLE seconds, MOVES moves)
set -euo pipefail

name=$1
out=$2
shift 2
[ "${1:-}" = "--" ] && shift
idle=${IDLE:-10}
moves=${MOVES:-1500}
mkdir -p "$out"

home=$(mktemp -d)
HOME="$home" XDG_CONFIG_HOME="$home/config" XDG_DATA_HOME="$home/data" XDG_CACHE_HOME="$home/cache" "$@" >"$out/$name.log" 2>&1 &
pid=$!
trap 'kill "$pid" 2>/dev/null || true' EXIT

# The window: found by its process, else by its title, and put at the top left at a size both UIs share.
win=""
for _ in $(seq 1 240); do
    win=$(xdotool search --onlyvisible --pid "$pid" 2>/dev/null | tail -1 || true)
    [ -n "$win" ] || win=$(xdotool search --onlyvisible --name BIMCraft 2>/dev/null | tail -1 || true)
    [ -n "$win" ] && break
    if ! kill -0 "$pid" 2>/dev/null; then
        echo "$name: exited before it showed a window"
        tail -40 "$out/$name.log"
        exit 1
    fi
    sleep 0.5
done
[ -n "$win" ] || { echo "$name: no window in two minutes"; tail -40 "$out/$name.log"; exit 1; }
xdotool windowmove "$win" 0 0 windowsize "$win" 1280 800
sleep 12

ticks() { sed 's/.*) //' "/proc/$pid/stat" | awk '{print $12 + $13}'; }
now() { date +%s.%N; }
hz=$(getconf CLK_TCK)

t0=$(now); c0=$(ticks)
sleep "$idle"
t1=$(now); c1=$(ticks)

awk -v n="$moves" 'BEGIN { for (i = 0; i < n; i++) { a = i * 0.15; printf "mousemove %d %d sleep 0.016\n", 560 + 260 * cos(a), 400 + 200 * sin(a) } }' | xargs xdotool
t2=$(now); c2=$(ticks)

rss=$(awk '/VmRSS/ {print int($2 / 1024)}' "/proc/$pid/status")
import -window root "$out/$name.png" 2>/dev/null || true

awk -v name="$name" -v hz="$hz" -v idle="$idle" -v moves="$moves" -v t0="$t0" -v t1="$t1" -v t2="$t2" -v c0="$c0" -v c1="$c1" -v c2="$c2" -v rss="$rss" 'BEGIN {
    idle_s = t1 - t0; move_s = t2 - t1
    idle_cpu = (c1 - c0) / hz; move_cpu = (c2 - c1) / hz
    printf "{\"name\":\"%s\",\"idle_seconds\":%.1f,\"idle_cores\":%.3f,\"move_seconds\":%.1f,\"moves\":%d,\"move_cores\":%.3f,\"ms_cpu_per_move\":%.2f,\"rss_mb\":%d}\n", \
        name, idle_s, idle_cpu / idle_s, move_s, moves, move_cpu / move_s, move_cpu * 1000 / moves, rss
}' | tee "$out/$name.json"
