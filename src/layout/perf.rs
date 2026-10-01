//! Performance instrumentation for the layout engine.
//!
//! Measures time spent in each phase of the rendering pipeline.
//! Enable with `LayoutEngine::enable_perf_tracking()`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Performance counters for one layout pass.
#[derive(Clone, Debug, Default)]
pub struct PerfCounters {
    /// Total cascade time.
    pub cascade_ms: f32,
    /// Total layout time (includes all sub-phases).
    pub layout_ms: f32,
    /// Display list build time.
    pub display_list_ms: f32,
    /// Display list replay (paint) time.
    pub paint_ms: f32,
    /// Number of layout_box calls.
    pub layout_calls: u32,
    /// Number of layout_box calls skipped (cache hit).
    pub layout_skipped: u32,
    /// Number of intrinsic_sizes calls.
    pub intrinsic_calls: u32,
    /// Number of text measurements.
    pub text_measure_calls: u32,
    /// Number of text measurement cache hits.
    pub text_cache_hits: u32,
    /// Number of DOM nodes.
    pub node_count: u32,
    /// Number of CSS rules.
    pub rule_count: u32,
    /// Peak layout recursion depth.
    pub max_depth: u32,
}

impl PerfCounters {
    /// Format as a human-readable summary.
    pub fn summary(&self) -> String {
        let total = self.cascade_ms + self.layout_ms + self.display_list_ms + self.paint_ms;
        let cache_rate = if self.layout_calls + self.layout_skipped > 0 {
            self.layout_skipped as f32 / (self.layout_calls + self.layout_skipped) as f32 * 100.0
        } else {
            0.0
        };
        let text_cache_rate = if self.text_measure_calls > 0 {
            self.text_cache_hits as f32 / self.text_measure_calls as f32 * 100.0
        } else {
            0.0
        };
        format!(
            "Total: {:.1}ms | Cascade: {:.1}ms | Layout: {:.1}ms | DL: {:.1}ms | Paint: {:.1}ms\n\
             Nodes: {} | Rules: {} | Depth: {}\n\
             Layout calls: {} (skipped: {} = {:.0}%)\n\
             Text measurements: {} (cache: {:.0}%)",
            total,
            self.cascade_ms,
            self.layout_ms,
            self.display_list_ms,
            self.paint_ms,
            self.node_count,
            self.rule_count,
            self.max_depth,
            self.layout_calls,
            self.layout_skipped,
            cache_rate,
            self.text_measure_calls,
            text_cache_rate,
        )
    }

    /// Is this frame within the 16ms budget (60fps)?
    pub fn within_budget(&self) -> bool {
        self.cascade_ms + self.layout_ms + self.display_list_ms + self.paint_ms < 16.0
    }

    /// Total frame time in ms.
    pub fn total_ms(&self) -> f32 {
        self.cascade_ms + self.layout_ms + self.display_list_ms + self.paint_ms
    }
}

// Thread-local perf tracking state. A `//` comment, not `///`: a doc comment
// on a macro invocation is silently dropped, which is what the warning says.
thread_local! {
    static PERF: RefCell<PerfState> = RefCell::new(PerfState::default());
    static HOT_NODES: RefCell<HashMap<(&'static str, u32), HotNode>> = RefCell::new(HashMap::new());
}

struct HotNode {
    calls: u32,
    total: Duration,
    max: Duration,
}

pub struct LayoutNodeSpan {
    kind: &'static str,
    node_id: u32,
    started: Option<Instant>,
}

pub fn node_span(kind: &'static str, node_id: u32) -> LayoutNodeSpan {
    let started = crate::profile::is_enabled().then(Instant::now);
    LayoutNodeSpan {
        kind,
        node_id,
        started,
    }
}

impl Drop for LayoutNodeSpan {
    fn drop(&mut self) {
        let Some(started) = self.started else { return };
        let elapsed = started.elapsed();
        HOT_NODES.with(|nodes| {
            let mut nodes = nodes.borrow_mut();
            let entry = nodes
                .entry((self.kind, self.node_id))
                .or_insert_with(|| HotNode {
                    calls: 0,
                    total: Duration::ZERO,
                    max: Duration::ZERO,
                });
            entry.calls += 1;
            entry.total += elapsed;
            entry.max = entry.max.max(elapsed);
        });
    }
}

pub fn flush_hot_nodes() {
    if !crate::profile::is_enabled() {
        return;
    }
    let epoch = crate::profile::epoch();
    HOT_NODES.with(|nodes| {
        let mut entries: Vec<_> = std::mem::take(&mut *nodes.borrow_mut())
            .into_iter()
            .collect();
        entries.sort_unstable_by_key(|(_, node)| std::cmp::Reverse(node.total));
        for ((kind, id), node) in entries.into_iter().take(20) {
            let label = format!(
                "node:{id} calls:{} max:{:.1}ms",
                node.calls,
                node.max.as_secs_f64() * 1000.0
            );
            crate::profile::record_resource_for(epoch, kind, &label, "inclusive", 0, node.total);
        }
    });
}

#[derive(Default)]
struct PerfState {
    enabled: bool,
    counters: PerfCounters,
    phase_start: Option<Instant>,
}

/// Enable performance tracking for the current thread.
pub fn enable() {
    PERF.with(|p| p.borrow_mut().enabled = true);
}

/// Disable performance tracking.
pub fn disable() {
    PERF.with(|p| p.borrow_mut().enabled = false);
}

/// Is tracking enabled?
pub fn is_enabled() -> bool {
    PERF.with(|p| p.borrow().enabled)
}

/// Reset counters for a new frame.
pub fn reset() {
    HOT_NODES.with(|nodes| nodes.borrow_mut().clear());
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        s.counters = PerfCounters::default();
    });
}

/// Get the current counters.
pub fn counters() -> PerfCounters {
    PERF.with(|p| p.borrow().counters.clone())
}

/// Start timing a phase.
pub fn start_phase() {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if s.enabled {
            s.phase_start = Some(Instant::now());
        }
    });
}

/// End timing and add to cascade_ms.
pub fn end_cascade() {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if let Some(start) = s.phase_start.take() {
            s.counters.cascade_ms += start.elapsed().as_secs_f32() * 1000.0;
        }
    });
}

/// End timing and add to layout_ms.
pub fn end_layout() {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if let Some(start) = s.phase_start.take() {
            s.counters.layout_ms += start.elapsed().as_secs_f32() * 1000.0;
        }
    });
}

/// End timing and add to display_list_ms.
pub fn end_display_list() {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if let Some(start) = s.phase_start.take() {
            s.counters.display_list_ms += start.elapsed().as_secs_f32() * 1000.0;
        }
    });
}

/// End timing and add to paint_ms.
pub fn end_paint() {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if let Some(start) = s.phase_start.take() {
            s.counters.paint_ms += start.elapsed().as_secs_f32() * 1000.0;
        }
    });
}

/// Record a layout_box call.
pub fn record_layout_call() {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if s.enabled {
            s.counters.layout_calls += 1;
        }
    });
}

/// Record a layout_box skip (cache hit).
pub fn record_layout_skip() {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if s.enabled {
            s.counters.layout_skipped += 1;
        }
    });
}

/// Record a text measurement.
pub fn record_text_measure(cache_hit: bool) {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if s.enabled {
            s.counters.text_measure_calls += 1;
            if cache_hit {
                s.counters.text_cache_hits += 1;
            }
        }
    });
}

/// Set node/rule counts.
pub fn set_counts(nodes: u32, rules: u32) {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        s.counters.node_count = nodes;
        s.counters.rule_count = rules;
    });
}

/// Record max depth.
pub fn record_depth(depth: u32) {
    PERF.with(|p| {
        let mut s = p.borrow_mut();
        if depth > s.counters.max_depth {
            s.counters.max_depth = depth;
        }
    });
}
