#!/bin/bash
# Perf harness: drives the SwiftUI app or the GPUI prototype through the same
# scenario, samples CPU/RSS/footprint per phase, collects the apps' MELO_PERF
# probes, optionally records a Time Profiler trace, and writes JSON per run.
#
#   scripts/perf/bench.sh <swift|gpui> [runs=3] [--no-build] [--no-input] [--trace]
#   scripts/perf/report.py                     # → perf-out/report.md
#
# Prerequisites: Xcode (xcodebuild/xctrace), Rust toolchain, a local mpd on
# 127.0.0.1:6600 with a queue + library, and Accessibility permission for the
# terminal running this (synthetic input + window moves). Release builds only.
#
# The `swift` mode builds the SwiftUI app from the Melo monorepo checkout; point
# MELO_SWIFT_ROOT at it (the directory containing Melo.xcworkspace). Defaults to
# a sibling checkout named `melo`.
set -euo pipefail
cd "$(dirname "$0")/../.."          # repo root
SWIFT_ROOT="${MELO_SWIFT_ROOT:-$(cd .. && pwd)/melo}"
export PATH="$HOME/.cargo/bin:$PATH"
export DEVELOPER_DIR="${DEVELOPER_DIR:-/Applications/Xcode.app/Contents/Developer}"

APP="${1:-}"; RUNS="${2:-3}"
[[ "$APP" == "swift" || "$APP" == "gpui" ]] || { echo "usage: $0 <swift|gpui> [runs] [--no-build] [--no-input] [--trace]"; exit 2; }
NO_BUILD=0; NO_INPUT=0; TRACE=0
for arg in "${@:3}"; do
  case "$arg" in --no-build) NO_BUILD=1;; --no-input) NO_INPUT=1;; --trace) TRACE=1;; esac
done

OUT="perf-out/$APP"; mkdir -p "$OUT" target/perf
GEN="target/perf/inputgen"
[[ -x "$GEN" && "$GEN" -nt scripts/perf/inputgen.swift ]] || xcrun swiftc -O scripts/perf/inputgen.swift -o "$GEN"

# ---- build ------------------------------------------------------------------
if [[ "$APP" == "gpui" ]]; then
  [[ $NO_BUILD == 1 ]] || cargo build --release 2>&1 | tail -1
  BIN="target/release/melo"; PROC_NAME="melo"
else
  [[ -d "$SWIFT_ROOT/Melo.xcworkspace" ]] || { echo "swift mode: Melo.xcworkspace not found under $SWIFT_ROOT (set MELO_SWIFT_ROOT)"; exit 1; }
  [[ $NO_BUILD == 1 ]] || xcodebuild -workspace "$SWIFT_ROOT/Melo.xcworkspace" -scheme Melo -configuration Release \
      -destination 'platform=macOS' -derivedDataPath target/perf-swift build \
      CODE_SIGN_IDENTITY=- CODE_SIGNING_ALLOWED=NO 2>&1 | grep -E "BUILD (SUCCEEDED|FAILED)"
  BIN="$(find target/perf-swift/Build/Products/Release -maxdepth 1 -name '*.app' | head -1)/Contents/MacOS/Melo"; PROC_NAME="Melo"
fi
[[ -x "$BIN" ]] || { echo "binary not found: $BIN"; exit 1; }
BIN_SIZE=$(du -k "$BIN" | cut -f1)

# ---- helpers ----------------------------------------------------------------
now_ms() { python3 -c 'import time; print(int(time.time()*1000))'; }
osa() { osascript -e "$1" >/dev/null 2>&1 || true; }
front() { osa "tell application \"System Events\" to set frontmost of (first process whose unix id is $PID) to true"; }
place_window() {  # x y w h
  osa "tell application \"System Events\" to tell (first process whose unix id is $PID)
        set position of window 1 to {$1, $2}
        set size of window 1 to {$3, $4}
      end tell"
}
WX=100; WY=100; WW=1100; WH=720
list_x=$((WX + 700)); list_y=$((WY + 360))

sample_loop() {  # phase csv — samples until stop file appears
  local phase=$1
  while [[ ! -f "$STOP" ]]; do
    local cpu rss t
    read -r cpu rss < <(ps -o %cpu=,rss= -p "$PID" 2>/dev/null || echo "0 0")
    t=$(now_ms)
    echo "$RUN,$phase,$((t - T0)),$cpu,$rss" >> "$CSV"
    sleep 0.5
  done
}
phase() {  # name seconds [action-fn]
  local name=$1 secs=$2 action=${3:-}
  local start=$(( $(now_ms) - T0 ))
  STOP="$(mktemp -u)"; sample_loop "$name" & local sampler=$!
  if [[ -n "$action" && $NO_INPUT == 0 ]]; then $action & local act=$!; fi
  sleep "$secs"
  touch "$STOP"; wait $sampler 2>/dev/null || true; rm -f "$STOP"
  [[ -n "${act:-}" ]] && { kill $act 2>/dev/null || true; wait $act 2>/dev/null || true; unset act; }
  echo "$RUN,$name,$start,$(( $(now_ms) - T0 ))" >> "$PHASES"
}

# scenario actions (run in background for the phase duration)
act_queue_scroll() { front; "$GEN" key 2 cmd; sleep 0.5; "$GEN" move $list_x $list_y
  for _ in $(seq 1 6); do "$GEN" scroll -12 40 16; "$GEN" scroll 12 40 16; done; }
act_library() { front; "$GEN" key 3 cmd; sleep 2; "$GEN" move $((WX + 220)) $((WY + 380))
  for _ in $(seq 1 3); do "$GEN" scroll -10 40 16; "$GEN" scroll 10 40 16; done
  "$GEN" click $((WX + 120)) $((WY + 200)); sleep 1; "$GEN" move $((WX + 800)) $((WY + 500))
  for _ in $(seq 1 3); do "$GEN" scroll -10 30 16; "$GEN" scroll 10 30 16; done; }
act_resize() { for _ in $(seq 1 5); do place_window $WX $WY 920 620; sleep 0.6; place_window $WX $WY 1400 900; sleep 0.6; done
  place_window $WX $WY $WW $WH; }
act_appearance() { for _ in 1 2; do osa 'tell application "System Events" to tell appearance preferences to set dark mode to not dark mode'; sleep 2; done; }

# ---- runs -------------------------------------------------------------------
for RUN in $(seq 1 "$RUNS"); do
  LOG="$OUT/run$RUN.log"; CSV="$OUT/run$RUN.csv"; PHASES="$OUT/run$RUN.phases"; : > "$CSV"; : > "$PHASES"
  echo "== $APP run $RUN/$RUNS"
  T0=$(now_ms)
  MELO_PERF=1 "$BIN" > "$LOG" 2>&1 &
  PID=$!
  # wait for first status (or 20 s)
  for _ in $(seq 1 200); do grep -q "perf first_status" "$LOG" 2>/dev/null && break; sleep 0.1; done
  sleep 1; front; place_window $WX $WY $WW $WH; sleep 1
  [[ $NO_INPUT == 0 ]] && { front; "$GEN" key 1 cmd; }
  phase idle 10
  if [[ $TRACE == 1 ]]; then
    xctrace record --template 'Time Profiler' --attach "$PID" --time-limit 12s --output "$OUT/run$RUN.trace" >/dev/null 2>&1 &
  fi
  phase queue_scroll 12 act_queue_scroll
  phase library 14 act_library
  phase resize 7 act_resize
  phase appearance 5 act_appearance
  footprint "$PID" > "$OUT/run$RUN.footprint" 2>/dev/null || true
  kill "$PID" 2>/dev/null || true; wait "$PID" 2>/dev/null || true
  python3 - "$APP" "$RUN" "$OUT" "$BIN_SIZE" <<'PY'
import sys, json, re, statistics as st
app, run, out, bin_kb = sys.argv[1], int(sys.argv[2]), sys.argv[3], int(sys.argv[4])
log = open(f"{out}/run{run}.log").read().splitlines()
marks = {m.group(1): float(m.group(2)) for l in log for m in [re.match(r"perf (first_\w+) ([\d.]+)", l)] if m}
frames = [json.loads(l.split("perf frames ",1)[1]) for l in log if l.startswith("perf frames ")]
phases = {}
for l in open(f"{out}/run{run}.phases"):
    r, name, s, e = l.strip().split(","); phases[name] = (int(s), int(e))
samples = [l.strip().split(",") for l in open(f"{out}/run{run}.csv") if l.strip()]
res = {"app": app, "run": run, "bin_kb": bin_kb, "first_window_ms": marks.get("first_window"), "first_status_ms": marks.get("first_status"), "phases": {}}
# frame probe 't' is ms since process start; phases are ms since launch (≈ same origin)
for name, (s, e) in phases.items():
    ph = [x for x in samples if x[1] == name]
    cpu = [float(x[3]) for x in ph]; rss = [int(x[4]) for x in ph]
    fr = [f for f in frames if s <= f["t"] <= e]
    res["phases"][name] = {
        "cpu_mean": round(st.mean(cpu), 1) if cpu else None, "cpu_max": max(cpu) if cpu else None,
        "rss_mb": round(max(rss)/1024, 1) if rss else None,
        "frame_p50": round(st.median([f["p50"] for f in fr]), 2) if fr else None,
        "frame_p95": round(max(f["p95"] for f in fr), 2) if fr else None,
        "frame_max": round(max(f["max"] for f in fr), 2) if fr else None,
        "over16": sum(f["over16"] for f in fr) if fr else None,
        "over8": sum(f["over8"] for f in fr) if fr else None,
    }
try:
    fp = open(f"{out}/run{run}.footprint").read()
    m = re.search(r"phys_footprint:\s+([\d.]+)\s*(\w+)", fp) or re.search(r"Phys footprint:\s+([\d.]+)\s*(\w+)", fp)
    if m: res["footprint"] = f"{m.group(1)} {m.group(2)}"
except FileNotFoundError: pass
json.dump(res, open(f"{out}/run{run}.json", "w"), indent=1)
print(json.dumps({k: v for k, v in res.items() if k != "phases"}))
PY
  sleep 2
done
echo "done → $OUT (run scripts/perf/report.py for the side-by-side report)"
