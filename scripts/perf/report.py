#!/usr/bin/env python3
"""Merge perf-out/{swift,gpui}/run*.json into perf-out/report.md (medians)."""
import glob, json, os, statistics as st, subprocess, platform

os.chdir(os.path.join(os.path.dirname(__file__), "..", ".."))
apps = {}
for app in ("swift", "gpui"):
    runs = [json.load(open(p)) for p in sorted(glob.glob(f"perf-out/{app}/run*.json"))]
    if runs:
        apps[app] = runs

def med(vals):
    vals = [v for v in vals if v is not None]
    return round(st.median(vals), 1) if vals else None

def fmt(v):
    return "—" if v is None else str(v)

lines = ["# Melo perf: SwiftUI vs GPUI", ""]
try:
    sha = subprocess.check_output(["git", "rev-parse", "--short", "HEAD"], text=True).strip()
except Exception:
    sha = "?"
lines += [f"- commit `{sha}` · macOS {platform.mac_ver()[0]} · {platform.machine()}",
          f"- runs: " + ", ".join(f"{a}×{len(r)}" for a, r in apps.items()),
          "- medians across runs; frame stats are the main-thread stall probe (8 ms timer): p50/p95/max wake-up delta in ms, over16 = deltas > 16.7 ms, over8 = > two 120 Hz frames",
          ""]
cols = list(apps.keys())
lines.append("| metric | " + " | ".join(cols) + " |")
lines.append("|---|" + "---|" * len(cols))
def row(name, getter):
    lines.append(f"| {name} | " + " | ".join(fmt(med([getter(r) for r in apps[a]])) for a in cols) + " |")
row("binary size (KB)", lambda r: r.get("bin_kb"))
row("cold launch → first window (ms)", lambda r: r.get("first_window_ms"))
row("cold launch → first status (ms)", lambda r: r.get("first_status_ms"))
phases = []
for a in cols:
    for r in apps[a]:
        for p in r["phases"]:
            if p not in phases: phases.append(p)
for p in phases:
    for metric, label in [("cpu_mean", "CPU % mean"), ("cpu_max", "CPU % max"), ("rss_mb", "RSS MB"),
                          ("frame_p50", "frame p50 ms"), ("frame_p95", "frame p95 ms"), ("frame_max", "frame max ms"),
                          ("over16", "stalls >16.7 ms"), ("over8", "stalls >2×8.3 ms")]:
        row(f"{p}: {label}", lambda r, p=p, m=metric: r["phases"].get(p, {}).get(m))
lines.append("")
for a in cols:
    fps = [r.get("footprint") for r in apps[a] if r.get("footprint")]
    if fps:
        lines.append(f"- {a} phys footprint (last run): {fps[-1]}")
open("perf-out/report.md", "w").write("\n".join(lines) + "\n")
print("\n".join(lines))
