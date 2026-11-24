#!/usr/bin/env bash
# What a UI costs on a machine with no graphics card, the way a person would use it: start it under Xvfb on software Vulkan
# (lavapipe), let it settle, read the process's CPU time over an idle stretch, then over a stretch of the pointer moving over the
# plan (xdotool, about 50 moves a second), then over the wheel zooming it in and out, then over a drag that pans it (the middle button
# for egui, which pans with it: PAN_BUTTON=2; the left one otherwise), and keep a screenshot. docs/areas/rune-ui.md has the numbers
# and what they leave out.
#
# usage: ci/measure-ui.sh NAME OUTDIR -- COMMAND [ARGS...]      (DISPLAY names a running X server; IDLE seconds, MOVES moves)
set -euo pipefail

name=$1
out=$2
shift 2
[ "${1:-}" = "--" ] && shift
idle=${IDLE:-10}
moves=${MOVES:-1500}
notches=${NOTCHES:-200}
drags=${DRAGS:-1000}
button=${PAN_BUTTON:-1}
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
xdotool windowmove "$win" 0 0 windowsize "$win" 1280 800 || true
sleep "${SETTLE:-12}"
kill -0 "$pid" 2>/dev/null || { echo "$name: the window went away while it was being placed"; tail -40 "$out/$name.log"; exit 1; }

ticks() { sed 's/.*) //' "/proc/$pid/stat" | awk '{print $12 + $13}'; }
now() { date +%s.%N; }
hz=$(getconf CLK_TCK)

t0=$(now); c0=$(ticks)
sleep "$idle"
t1=$(now); c1=$(ticks)

awk -v n="$moves" 'BEGIN { for (i = 0; i < n; i++) { a = i * 0.15; printf "mousemove %d %d sleep 0.016\n", 560 + 260 * cos(a), 400 + 200 * sin(a) } }' | xargs xdotool
t2=$(now); c2=$(ticks)

# The wheel: ten notches in, ten out, at a point over the plan, about thirty a second.
awk -v n="$notches" 'BEGIN { printf "mousemove 560 400 sleep 0.2\n"; for (i = 0; i < n; i++) { printf "click %d sleep 0.03\n", (int(i / 10) % 2 == 0) ? 4 : 5 } }' | xargs xdotool
t3=$(now); c3=$(ticks)

# A drag with the pan button held, once round a circle back to where it started.
{
    echo "mousemove 560 400 sleep 0.2 mousedown $button"
    awk -v n="$drags" 'BEGIN { for (i = 0; i <= n; i++) { a = i * 6.2832 / n; printf "mousemove %d %d sleep 0.016\n", 460 + 100 * cos(a), 400 + 100 * sin(a) } }'
    echo "mouseup $button"
} | xargs xdotool
t4=$(now); c4=$(ticks)

rss=$(awk '/VmRSS/ {print int($2 / 1024)}' "/proc/$pid/status")
import -window root "$out/$name.png" 2>/dev/null || true

awk -v name="$name" -v hz="$hz" -v moves="$moves" -v notches="$notches" -v drags="$drags" \
    -v t0="$t0" -v t1="$t1" -v t2="$t2" -v t3="$t3" -v t4="$t4" -v c0="$c0" -v c1="$c1" -v c2="$c2" -v c3="$c3" -v c4="$c4" -v rss="$rss" 'BEGIN {
    printf "{\"name\":\"%s\",\"idle_cores\":%.3f,\"move_cores\":%.3f,\"ms_cpu_per_move\":%.2f,\"zoom_cores\":%.3f,\"ms_cpu_per_zoom\":%.2f,\"pan_cores\":%.3f,\"ms_cpu_per_pan\":%.2f,\"rss_mb\":%d}\n", \
        name, (c1 - c0) / hz / (t1 - t0), (c2 - c1) / hz / (t2 - t1), (c2 - c1) * 1000 / hz / moves,
        (c3 - c2) / hz / (t3 - t2), (c3 - c2) * 1000 / hz / notches, (c4 - c3) / hz / (t4 - t3), (c4 - c3) * 1000 / hz / drags, rss
}' | tee "$out/$name.json"
