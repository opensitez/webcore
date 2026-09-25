//! Opt-in cross-thread timing for page loading and rendering.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

static ENABLED: AtomicBool = AtomicBool::new(false);
static EPOCH: AtomicU64 = AtomicU64::new(0);
static STATE: OnceLock<Mutex<ProfileState>> = OnceLock::new();

#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum Phase {
    HtmlFetch,
    HtmlParse,
    CssFetch,
    CssParse,
    ImageFetch,
    ImageDecode,
    ResourcePoll,
    Cascade,
    Geometry,
    FrameUpdate,
    DisplayList,
    TileRaster,
    TileComposite,
    FixedReplay,
    ContentCache,
    DirectReplay,
    Render,
    BrowserDraw,
    ScrollPaint,
}

impl Phase {
    pub const ALL: [Self; 19] = [
        Self::HtmlFetch,
        Self::HtmlParse,
        Self::CssFetch,
        Self::CssParse,
        Self::ImageFetch,
        Self::ImageDecode,
        Self::ResourcePoll,
        Self::Cascade,
        Self::Geometry,
        Self::FrameUpdate,
        Self::DisplayList,
        Self::TileRaster,
        Self::TileComposite,
        Self::FixedReplay,
        Self::ContentCache,
        Self::DirectReplay,
        Self::Render,
        Self::BrowserDraw,
        Self::ScrollPaint,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Self::HtmlFetch => "html_fetch",
            Self::HtmlParse => "html_parse",
            Self::CssFetch => "css_fetch",
            Self::CssParse => "css_parse",
            Self::ImageFetch => "image_fetch",
            Self::ImageDecode => "image_decode",
            Self::ResourcePoll => "resource_poll",
            Self::Cascade => "cascade",
            Self::Geometry => "geometry",
            Self::FrameUpdate => "frame_update",
            Self::DisplayList => "display_list",
            Self::TileRaster => "tile_raster",
            Self::TileComposite => "tile_composite",
            Self::FixedReplay => "fixed_replay",
            Self::ContentCache => "content_cache",
            Self::DirectReplay => "direct_replay",
            Self::Render => "render",
            Self::BrowserDraw => "browser_draw",
            Self::ScrollPaint => "scroll_paint",
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Timing {
    pub count: u64,
    pub total_ns: u64,
    pub max_ns: u64,
}

impl Timing {
    fn add(&mut self, duration: Duration) {
        let ns = duration.as_nanos().min(u64::MAX as u128) as u64;
        self.count += 1;
        self.total_ns = self.total_ns.saturating_add(ns);
        self.max_ns = self.max_ns.max(ns);
    }
}

#[derive(Clone, Debug)]
pub struct ResourceTiming {
    pub kind: &'static str,
    pub url: String,
    pub source: &'static str,
    pub bytes: usize,
    pub duration_ns: u64,
}

#[derive(Clone, Debug)]
pub struct Snapshot {
    pub elapsed: Duration,
    pub timings: [Timing; Phase::ALL.len()],
    pub resources: Vec<ResourceTiming>,
}

struct ProfileState {
    epoch: u64,
    started: Instant,
    timings: [Timing; Phase::ALL.len()],
    resources: Vec<ResourceTiming>,
    pending_scroll: Option<Instant>,
}

impl ProfileState {
    fn new(epoch: u64) -> Self {
        Self {
            epoch,
            started: Instant::now(),
            timings: [Timing::default(); Phase::ALL.len()],
            resources: Vec::new(),
            pending_scroll: None,
        }
    }
}

fn state() -> &'static Mutex<ProfileState> {
    STATE.get_or_init(|| Mutex::new(ProfileState::new(EPOCH.load(Ordering::Relaxed))))
}

pub fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
    reset();
}

pub fn is_enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

pub fn epoch() -> u64 {
    EPOCH.load(Ordering::Relaxed)
}

pub fn reset() {
    if !is_enabled() {
        return;
    }
    let epoch = EPOCH.fetch_add(1, Ordering::Relaxed) + 1;
    *state().lock().unwrap_or_else(|poisoned| poisoned.into_inner()) = ProfileState::new(epoch);
}

pub struct Span {
    phase: Phase,
    started: Option<Instant>,
    epoch: u64,
}

pub fn span(phase: Phase) -> Span {
    Span {
        phase,
        started: is_enabled().then(Instant::now),
        epoch: EPOCH.load(Ordering::Relaxed),
    }
}

impl Drop for Span {
    fn drop(&mut self) {
        if let Some(started) = self.started {
            record_epoch(self.phase, started.elapsed(), self.epoch);
        }
    }
}

fn record_epoch(phase: Phase, duration: Duration, epoch: u64) {
    let mut profile = state().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if profile.epoch == epoch {
        profile.timings[phase as usize].add(duration);
    }
}

pub fn record(phase: Phase, duration: Duration) {
    if is_enabled() {
        record_epoch(phase, duration, EPOCH.load(Ordering::Relaxed));
    }
}

pub fn mark_scroll_input() {
    if !is_enabled() {
        return;
    }
    let mut profile = state().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    profile.pending_scroll.get_or_insert_with(Instant::now);
}

pub fn finish_scroll_paint() {
    if !is_enabled() {
        return;
    }
    let mut profile = state().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(started) = profile.pending_scroll.take() {
        profile.timings[Phase::ScrollPaint as usize].add(started.elapsed());
    }
}

pub fn record_resource_for(
    epoch: u64,
    kind: &'static str,
    url: &str,
    source: &'static str,
    bytes: usize,
    duration: Duration,
) {
    if !is_enabled() {
        return;
    }
    let mut profile = state().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if profile.epoch != epoch {
        return;
    }
    let sample = ResourceTiming {
        kind,
        url: url.to_string(),
        source,
        bytes,
        duration_ns: duration.as_nanos().min(u64::MAX as u128) as u64,
    };
    if profile.resources.len() < 128 {
        profile.resources.push(sample);
    } else if let Some((index, shortest)) = profile
        .resources
        .iter()
        .enumerate()
        .min_by_key(|(_, entry)| entry.duration_ns)
        && sample.duration_ns > shortest.duration_ns
    {
        profile.resources[index] = sample;
    }
}

pub fn snapshot() -> Snapshot {
    let profile = state().lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let mut resources = profile.resources.clone();
    resources.sort_unstable_by_key(|entry| std::cmp::Reverse(entry.duration_ns));
    Snapshot {
        elapsed: profile.started.elapsed(),
        timings: profile.timings,
        resources,
    }
}

pub fn summary() -> String {
    let snapshot = snapshot();
    let mut phases: Vec<_> = Phase::ALL
        .iter()
        .copied()
        .filter(|phase| snapshot.timings[*phase as usize].count > 0)
        .collect();
    phases.sort_unstable_by_key(|phase| {
        std::cmp::Reverse(snapshot.timings[*phase as usize].total_ns)
    });
    let mut out = format!("[profile] {:.1}s elapsed", snapshot.elapsed.as_secs_f64());
    let mut shown: Vec<_> = phases.into_iter().take(6).collect();
    if snapshot.timings[Phase::ScrollPaint as usize].count > 0
        && !shown.iter().any(|phase| matches!(phase, Phase::ScrollPaint))
    {
        shown.push(Phase::ScrollPaint);
    }
    for phase in shown {
        let timing = snapshot.timings[phase as usize];
        out.push_str(&format!(
            "\n  {}: {:.1}ms total / {} calls / {:.1}ms max",
            phase.name(),
            timing.total_ns as f64 / 1_000_000.0,
            timing.count,
            timing.max_ns as f64 / 1_000_000.0,
        ));
    }
    for resource in snapshot.resources.iter().take(2) {
        out.push_str(&format!(
            "\n  {} {}: {:.1}ms, {} bytes, {}",
            resource.kind,
            resource.source,
            resource.duration_ns as f64 / 1_000_000.0,
            resource.bytes,
            resource.url,
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timing_tracks_total_and_slowest_sample() {
        let mut timing = Timing::default();
        timing.add(Duration::from_millis(3));
        timing.add(Duration::from_millis(7));
        assert_eq!(timing.count, 2);
        assert_eq!(timing.total_ns, 10_000_000);
        assert_eq!(timing.max_ns, 7_000_000);
    }

    #[test]
    fn phase_names_are_unique() {
        let names: std::collections::HashSet<_> = Phase::ALL.iter().map(|phase| phase.name()).collect();
        assert_eq!(names.len(), Phase::ALL.len());
    }
}
