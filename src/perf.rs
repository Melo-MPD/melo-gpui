//! Opt-in performance probes (`MELO_PERF=1`), printed to stderr in a
//! machine-readable form so `scripts/perf/bench.sh` can parse them:
//!
//!   perf first_window <ms>          first root render after process start
//!   perf first_status <ms>          first MPD status applied
//!   perf frames {"t":..,"n":..,"p50":..,"p95":..,"max":..,"over16":..,"over8":..}
//!                                    once per second: main-thread stall probe
//!                                    (an 8 ms foreground timer; deltas beyond
//!                                    the period mean the UI thread was busy)
//!
//! The SwiftUI app prints the same lines (Melo-macOS/App/PerfProbe.swift).

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static START: OnceLock<Instant> = OnceLock::new();
static MARKS: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);

pub fn enabled() -> bool {
    std::env::var_os("MELO_PERF").is_some()
}

/// Call first thing in `main`.
pub fn init() {
    let _ = START.set(Instant::now());
}

fn ms_since_start() -> f64 {
    START
        .get()
        .map(|s| s.elapsed().as_secs_f64() * 1000.0)
        .unwrap_or(0.0)
}

/// Prints `perf <name> <ms>` the first time each name is marked.
pub fn mark(name: &'static str) {
    if !enabled() {
        return;
    }
    let mut marks = MARKS.lock().unwrap();
    let set = marks.get_or_insert_with(HashSet::new);
    if set.insert(name) {
        eprintln!("perf {name} {:.1}", ms_since_start());
    }
}

/// Main-thread stall probe: an 8 ms foreground timer; every ~1 s the observed
/// wake-up deltas are summarised. Runs for the life of the app.
pub fn start_frame_probe(cx: &mut gpui::App) {
    if !enabled() {
        return;
    }
    const PERIOD: Duration = Duration::from_millis(8);
    cx.spawn(async move |cx| {
        let mut deltas: Vec<f64> = Vec::with_capacity(256);
        let mut last = Instant::now();
        let mut window_start = last;
        loop {
            cx.background_executor().timer(PERIOD).await;
            let now = Instant::now();
            deltas.push((now - last).as_secs_f64() * 1000.0);
            last = now;
            if now.duration_since(window_start) >= Duration::from_secs(1) {
                report(&mut deltas);
                window_start = now;
            }
        }
    })
    .detach();
}

fn report(deltas: &mut Vec<f64>) {
    if deltas.is_empty() {
        return;
    }
    let mut sorted = deltas.clone();
    sorted.sort_by(|a, b| a.partial_cmp(b).unwrap());
    let pct = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
    let over16 = sorted.iter().filter(|d| **d > 16.7).count();
    let over8 = sorted.iter().filter(|d| **d > 8.4 * 2.0).count(); // > two 120 Hz frames late
    eprintln!(
        "perf frames {{\"t\":{:.1},\"n\":{},\"p50\":{:.2},\"p95\":{:.2},\"max\":{:.2},\"over16\":{},\"over8\":{}}}",
        ms_since_start(),
        sorted.len(),
        pct(0.5),
        pct(0.95),
        sorted[sorted.len() - 1],
        over16,
        over8
    );
    deltas.clear();
}
