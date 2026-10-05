//! Bounded PCM delivery. Decoding and native device calls never run on the UI thread.

use super::audio_output::{PcmOutput, PcmOutputError};
use super::AudioSamples;
use std::sync::{Arc, atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering}, mpsc};
use std::time::{Duration, Instant};
use std::collections::VecDeque;

const POLL: Duration = Duration::from_millis(5);
const AUDIO_LEAD: f32 = 0.08;
const VIDEO_LEAD: f32 = 0.5;

#[cfg(test)]
#[path = "../../tests/media_audio/playback_verification.rs"]
mod verification_tests;

pub(crate) struct Control {
    cancelled: AtomicBool,
    paused: AtomicBool,
    looping: AtomicBool,
    volume: AtomicU32,
    time: AtomicU32,
    rate: AtomicU32,
    origin: Instant,
    anchor_ns: AtomicU64,
    generation: AtomicU64,
    completed: AtomicU64,
    audio_clock: AtomicU64,
    audio_disabled: AtomicBool,
}

impl Control {
    pub(crate) fn cancelled(&self) -> bool { self.cancelled.load(Ordering::Acquire) }
    pub(crate) fn generation(&self) -> u64 { self.generation.load(Ordering::Acquire) }
    pub(crate) fn time(&self) -> f32 {
        let base = f32::from_bits(self.time.load(Ordering::Acquire));
        if self.paused() { return base; }
        if let Some(time) = self.audio_time() { return time; }
        let elapsed = self.origin.elapsed().as_nanos() as u64;
        let anchor = self.anchor_ns.load(Ordering::Acquire);
        base + elapsed.saturating_sub(anchor) as f32 / 1e9 * f32::from_bits(self.rate.load(Ordering::Acquire))
    }
    pub(crate) fn looping(&self) -> bool { self.looping.load(Ordering::Acquire) }
    pub(crate) fn paused(&self) -> bool { self.paused.load(Ordering::Acquire) }
    fn audio_time(&self) -> Option<f32> {
        if self.audio_disabled.load(Ordering::Acquire) { return None; }
        let clock = self.audio_clock.load(Ordering::Acquire);
        let time = f32::from_bits(clock as u32);
        (clock >> 32 == u64::from(self.generation() as u32) && time.is_finite()).then_some(time)
    }
    fn publish_audio_time(&self, generation: u64, time: f64) {
        self.audio_clock.store((u64::from(generation as u32) << 32) | u64::from((time as f32).to_bits()), Ordering::Release);
    }
}

struct Packet {
    generation: u64,
    timestamp: f64,
    samples: AudioSamples,
}

// A pass owns only its provisional clock, never a submitted or newer-generation clock.
struct PendingAudioClock<'a> {
    control: &'a Control,
    generation: u64,
    owned: std::sync::Mutex<Option<u64>>,
}

impl<'a> PendingAudioClock<'a> {
    fn new(control: &'a Control, generation: u64) -> Self {
        Self { control, generation, owned: std::sync::Mutex::new(None) }
    }

    fn pin(&self) {
        let mut owned = self.owned.lock().unwrap();
        if owned.is_some() || self.control.generation() != self.generation
            || self.control.audio_disabled.load(Ordering::Acquire)
        { return; }
        let previous = self.control.audio_clock.load(Ordering::Acquire);
        if self.control.generation() != self.generation { return; }
        let tag = u64::from(self.generation as u32) << 32;
        if previous >> 32 == tag >> 32 && f32::from_bits(previous as u32).is_finite() {
            return;
        }
        let requested = self.control.time.load(Ordering::Acquire);
        if !f32::from_bits(requested).is_finite() { return; }
        let clock = tag | u64::from(requested);
        if self.control.audio_clock.compare_exchange(previous, clock, Ordering::AcqRel, Ordering::Acquire).is_ok() {
            *owned = Some(clock);
            if self.control.generation() != self.generation {
                self.control.audio_clock.compare_exchange(clock, tag | u64::from(f32::NAN.to_bits()), Ordering::AcqRel, Ordering::Acquire).ok();
                *owned = None;
            }
        }
    }

    fn handoff(&self, end: f64) {
        if end > f64::from(f32::from_bits(self.control.time.load(Ordering::Acquire))) {
            self.owned.lock().unwrap().take();
        }
    }

    fn release(&self) {
        if let Some(clock) = self.owned.lock().unwrap().take() {
            let invalid = (clock & 0xffff_ffff_0000_0000) | u64::from(f32::NAN.to_bits());
            self.control.audio_clock.compare_exchange(clock, invalid, Ordering::AcqRel, Ordering::Acquire).ok();
        }
    }
}

impl Drop for PendingAudioClock<'_> {
    fn drop(&mut self) { self.release(); }
}

pub(crate) struct Playback {
    pub(crate) control: Arc<Control>,
    last_seek_revision: u64,
    last_rate: f32,
}

pub(crate) struct PcmSender {
    control: Arc<Control>,
    tx: mpsc::SyncSender<Packet>,
}

impl Playback {
    pub(crate) fn new(time: f32) -> (Self, PcmSender) {
        let control = Arc::new(Control {
            cancelled: AtomicBool::new(false), paused: AtomicBool::new(true),
            looping: AtomicBool::new(false), volume: AtomicU32::new(0.0f32.to_bits()),
            time: AtomicU32::new(time.to_bits()), generation: AtomicU64::new(0),
            rate: AtomicU32::new(1.0f32.to_bits()), origin: Instant::now(), anchor_ns: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            audio_clock: AtomicU64::new(u64::from(f32::NAN.to_bits())),
            audio_disabled: AtomicBool::new(false),
        });
        let (tx, rx) = mpsc::sync_channel(8);
        let worker = control.clone();
        std::thread::spawn(move || {
            if let Err(error) = run_output(worker.clone(), rx, PcmOutput::new) {
                worker.audio_disabled.store(true, Ordering::Release);
                eprintln!("Media PCM output failed: {error:?}");
            }
        });
        let sender = PcmSender { control: control.clone(), tx };
        (Self { control, last_seek_revision: 0, last_rate: 1.0 }, sender)
    }

    /// Atomics only: no decoder/device locks or waits in a render callback.
    pub(crate) fn update(&mut self, time: f32, paused: bool, muted: bool, volume: f32, looping: bool, rate: f32, seek_revision: u64) -> bool {
        let seek = seek_revision != self.last_seek_revision;
        if seek || paused != self.control.paused() || rate != self.last_rate {
            let anchor = if seek { time } else { self.control.time() };
            self.control.time.store(anchor.to_bits(), Ordering::Release);
            self.control.anchor_ns.store(self.control.origin.elapsed().as_nanos() as u64, Ordering::Release);
            self.control.rate.store(rate.to_bits(), Ordering::Release);
        }
        self.control.volume.store(if muted { 0.0 } else { volume.clamp(0.0, 1.0) }.to_bits(), Ordering::Release);
        self.control.paused.store(paused, Ordering::Release);
        self.control.looping.store(looping, Ordering::Release);
        if seek { self.control.generation.fetch_add(1, Ordering::AcqRel); }
        self.last_seek_revision = seek_revision;
        self.last_rate = rate;
        seek
    }

    pub(crate) fn synchronize_audio_time(&mut self, duration: Option<f32>, looping: bool) -> Option<f32> {
        self.control.audio_time()?;
        let mut time = self.control.time();
        if let Some(duration) = duration.filter(|value| value.is_finite() && *value > 0.0) {
            time = if looping { time.rem_euclid(duration) } else { time.min(duration) };
        }
        Some(time)
    }
}

impl Drop for Playback {
    fn drop(&mut self) { self.control.cancelled.store(true, Ordering::Release); }
}

impl PcmSender {
    #[cfg(test)]
    pub(crate) fn send(&self, generation: u64, timestamp: f64, samples: AudioSamples) -> bool {
        let mut packet = Packet { generation, timestamp, samples };
        loop {
            if self.control.cancelled() { return false; }
            if self.control.generation() != generation { return true; }
            match self.tx.try_send(packet) {
                Ok(()) => return true,
                Err(mpsc::TrySendError::Full(value)) => { packet = value; std::thread::sleep(POLL); }
                Err(mpsc::TrySendError::Disconnected(_)) => return false,
            }
        }
    }
}

pub(crate) enum VideoUpdate {
    Metadata(super::MediaMetadata),
    Frame { generation: u64, frame: super::VideoFrame },
}

#[path = "playback_parallel.rs"]
mod parallel;

#[path = "playback_mp4.rs"]
mod mp4;

pub(crate) fn decode_mp4(
    control: Arc<Control>, pcm: PcmSender,
    open: impl FnMut() -> Option<Box<dyn std::io::Read + Send>>,
    emit: impl FnMut(VideoUpdate, &AtomicBool) -> bool + Send,
) {
    mp4::decode_mp4(control, pcm, open, emit);
}

pub(crate) fn decode_webm(
    control: Arc<Control>, pcm: PcmSender,
    open: impl FnMut() -> Option<Box<dyn std::io::Read + Send>>,
    emit: impl FnMut(VideoUpdate, &AtomicBool) -> bool + Send,
) {
    parallel::decode_webm(control, pcm, open, emit);
}

trait Output {
    fn submit(&mut self, samples: &[f32]) -> Result<usize, PcmOutputError>;
    fn set_volume(&mut self, volume: f32) -> Result<(), PcmOutputError>;
    fn set_paused(&mut self, paused: bool) -> Result<(), PcmOutputError>;
    fn wait(&self);
    fn drained(&self) -> bool;
    fn reset(&mut self) -> Result<(), PcmOutputError>;
    fn completed_frames(&self) -> u64;
}

impl Output for PcmOutput {
    fn submit(&mut self, samples: &[f32]) -> Result<usize, PcmOutputError> { PcmOutput::submit(self, samples) }
    fn set_volume(&mut self, volume: f32) -> Result<(), PcmOutputError> { PcmOutput::set_volume(self, volume) }
    fn set_paused(&mut self, paused: bool) -> Result<(), PcmOutputError> { PcmOutput::set_paused(self, paused) }
    fn wait(&self) { self.wait_for_buffer(POLL); }
    fn drained(&self) -> bool { PcmOutput::drained(self) }
    fn reset(&mut self) -> Result<(), PcmOutputError> { PcmOutput::reset(self) }
    fn completed_frames(&self) -> u64 { PcmOutput::completed_frames(self) }
}

fn run_output<O: Output>(control: Arc<Control>, rx: mpsc::Receiver<Packet>, mut create: impl FnMut(u32, u16) -> Result<O, PcmOutputError>) -> Result<(), PcmOutputError> {
    let mut device: Option<O> = None;
    let mut format = None;
    let mut generation = control.generation();
    let mut seek_time = f64::from(f32::from_bits(control.time.load(Ordering::Acquire)));
    let mut packet: Option<Packet> = None;
    let mut packet_validated = false;
    let mut offset = 0;
    let mut disconnected = false;
    let mut timeline = AudioTimeline::default();
    let trace_audio = crate::profile::media_enabled()
        || std::env::var_os("WEBCORE_TRACE_AUDIO_DELIVERY").is_some();
    let mut output_start = Instant::now();
    let mut trace_completed = 0;
    let mut trace_peak = 0.0f32;
    let mut delivery_trace = trace_audio.then(AudioDeliveryTrace::new);
    while !control.cancelled() {
        if control.generation() != generation {
            generation = control.generation();
            seek_time = f64::from(f32::from_bits(control.time.load(Ordering::Acquire)));
            if let Some(output) = device.as_mut() { output.reset()?; }
            control.completed.store(0, Ordering::Release);
            timeline = AudioTimeline::default();
            trace_completed = 0;
            trace_peak = 0.0;
            output_start = Instant::now();
            if let Some(trace) = &mut delivery_trace { *trace = AudioDeliveryTrace::new(); }
            if trace_audio { eprintln!("audio reset generation={generation} seek={seek_time:.3}"); }
            packet = None;
            packet_validated = false;
            offset = 0;
        }
        if let Some(output) = device.as_mut() {
            let completed = output.completed_frames();
            control.completed.store(completed, Ordering::Release);
            timeline.publish(&control, generation, completed);
            if let Some(trace) = &mut delivery_trace {
                trace.observe(completed, timeline.submitted,
                    f32::from_bits(control.volume.load(Ordering::Acquire)), control.paused());
            }
            if trace_audio && let Some((rate, _)) = format {
                if completed.saturating_sub(trace_completed) >= u64::from(rate) {
                    eprintln!("audio device generation={generation} submitted={} completed={completed} clock={:.3} peak={trace_peak:.6} volume={:.3} paused={}",
                        timeline.submitted, control.time(), f32::from_bits(control.volume.load(Ordering::Acquire)), control.paused());
                    trace_completed = completed;
                    trace_peak = 0.0;
                }
            }
            output.set_volume(f32::from_bits(control.volume.load(Ordering::Acquire)))?;
            output.set_paused(control.paused())?;
        }
        if packet.is_none() && !disconnected {
            match rx.recv_timeout(POLL) {
                Ok(value) if value.generation == generation => {
                    packet = Some(value); offset = 0; packet_validated = false;
                }
                Ok(_) => continue,
                Err(mpsc::RecvTimeoutError::Timeout) => {
                    if let Some(trace) = &mut delivery_trace { trace.queue_timeouts += 1; }
                    continue;
                }
                Err(mpsc::RecvTimeoutError::Disconnected) => disconnected = true,
            }
        }
        let Some(current) = packet.as_ref() else {
            if disconnected && device.as_ref().is_none_or(Output::drained) {
                if let Some(output) = device.as_ref() {
                    control.completed.store(output.completed_frames(), Ordering::Release);
                    timeline.publish(&control, generation, output.completed_frames());
                }
                return Ok(());
            }
            std::thread::sleep(POLL);
            continue;
        };
        // A seek can arrive while recv_timeout is asleep; do not submit its stale packet.
        if current.generation != control.generation() { continue; }
        let samples = &current.samples;
        let channels = usize::from(samples.channels);
        if !packet_validated {
            if channels == 0 || samples.sample_rate == 0 || samples.samples.len() % channels != 0
                || samples.samples.iter().any(|s| !s.is_finite()) || !current.timestamp.is_finite() {
                return Err(PcmOutputError::InvalidData);
            }
            packet_validated = true;
        }
        // Trim negative CodecDelay and samples before the requested seek point.
        let time = f64::from(control.time());
        // Device startup/backpressure must not repeatedly cut a continuous stream.
        // Only codec preroll and the explicitly requested seek position are trimmed.
        let first = ((seek_time - current.timestamp).max(0.0) * f64::from(samples.sample_rate)).floor() as usize;
        offset = offset.max(first.saturating_mul(channels)).min(samples.samples.len());
        if offset == samples.samples.len() { packet = None; continue; }
        let timestamp = current.timestamp + (offset / channels) as f64 / f64::from(samples.sample_rate);
        let current_format = (samples.sample_rate, samples.channels);
        // Device creation/start can wait on the OS. Establish the actual PCM
        // timeline first so video cannot run ahead on the temporary wall clock.
        if !control.paused() && timeline.submitted == 0 && control.audio_time().is_none() {
            control.publish_audio_time(generation, timestamp);
        }
        if format != Some(current_format) {
            let start = Instant::now();
            let mut output = create(samples.sample_rate, samples.channels)?;
            output.set_paused(control.paused())?;
            if trace_audio { eprintln!("audio device creation_ms={:.1} paused={}", start.elapsed().as_secs_f64() * 1000.0, control.paused()); }
            device = Some(output);
            format = Some(current_format);
            timeline = AudioTimeline::default();
        }
        // Device creation can outlive cancellation or a seek.
        if control.cancelled() { return Ok(()); }
        if current.generation != control.generation() { continue; }
        if control.paused() || (control.audio_time().is_none() && timestamp > time + f64::from(AUDIO_LEAD)) {
            std::thread::sleep(POLL);
            continue;
        }
        let output = device.as_mut().unwrap();
        output.set_volume(f32::from_bits(control.volume.load(Ordering::Acquire)))?;
        output.set_paused(false)?;
        if control.cancelled() { return Ok(()); }
        if current.generation != control.generation() { continue; }
        // Four native buffers represent at most 80 ms rather than 4x4096 frames.
        let batch = (samples.sample_rate as usize / 50).max(1) * channels;
        let last = (offset + batch).min(samples.samples.len());
        let submit_start = trace_audio.then(Instant::now);
        match output.submit(&samples.samples[offset..last]) {
            Ok(0) => return Err(PcmOutputError::InvalidData),
            Ok(count) if count <= last - offset && count % channels == 0 => {
                if trace_audio {
                    if timeline.submitted == 0 {
                        eprintln!("audio first submission startup_ms={:.1} submit_ms={:.1} pts={timestamp:.6} volume={:.3}",
                            output_start.elapsed().as_secs_f64() * 1000.0,
                            submit_start.unwrap().elapsed().as_secs_f64() * 1000.0,
                            f32::from_bits(control.volume.load(Ordering::Acquire)));
                    }
                    trace_peak = samples.samples[offset..offset + count].iter()
                        .fold(trace_peak, |peak, sample| peak.max(sample.abs()));
                }
                if let Some(trace) = &mut delivery_trace {
                    trace.submitted(timestamp, count / channels, samples.sample_rate);
                }
                timeline.submit(&control, generation, timestamp, count / channels, samples.sample_rate);
                offset += count;
                if offset == samples.samples.len() { packet = None; }
            }
            Ok(_) => return Err(PcmOutputError::InvalidData),
            Err(PcmOutputError::WouldBlock) => {
                let start = delivery_trace.as_ref().map(|_| Instant::now());
                output.wait();
                if let Some(trace) = &mut delivery_trace {
                    trace.device_wait += start.unwrap().elapsed();
                }
            }
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

struct AudioDeliveryTrace {
    start: Instant,
    report: Instant,
    last_completed: u64,
    first_completion: bool,
    first_unmuted_completion: bool,
    volume: Option<f32>,
    unmuted_since: Option<Instant>,
    unmute_completed: u64,
    drained: bool,
    underruns: u64,
    queue_timeouts: u64,
    device_wait: Duration,
    expected_pts: Option<f64>,
    gap_frames: u64,
    overlap_frames: u64,
}

impl AudioDeliveryTrace {
    fn new() -> Self {
        let now = Instant::now();
        Self { start: now, report: now, last_completed: 0, first_completion: false,
            first_unmuted_completion: false, volume: None, unmuted_since: None,
            unmute_completed: 0, drained: false,
            underruns: 0, queue_timeouts: 0, device_wait: Duration::ZERO,
            expected_pts: None, gap_frames: 0, overlap_frames: 0 }
    }

    fn submitted(&mut self, timestamp: f64, frames: usize, rate: u32) {
        if let Some(expected) = self.expected_pts {
            let delta = ((timestamp - expected) * f64::from(rate)).round();
            if delta > 0.0 { self.gap_frames = self.gap_frames.saturating_add(delta as u64); }
            if delta < 0.0 { self.overlap_frames = self.overlap_frames.saturating_add((-delta) as u64); }
        }
        self.expected_pts = Some(timestamp + frames as f64 / f64::from(rate));
    }

    fn observe(&mut self, completed: u64, submitted: u64, volume: f32, paused: bool) {
        let elapsed = self.start.elapsed().as_secs_f64() * 1000.0;
        if self.volume != Some(volume) {
            eprintln!("audio volume change elapsed_ms={elapsed:.1} old={:?} new={volume:.3} completed={completed}", self.volume);
            if volume > 0.0 && self.volume.is_none_or(|old| old <= 0.0) {
                self.unmuted_since = Some(Instant::now());
                self.unmute_completed = completed;
                self.first_unmuted_completion = false;
            }
            self.volume = Some(volume);
        }
        if completed > self.last_completed {
            if !self.first_completion {
                eprintln!("audio first device completion elapsed_ms={elapsed:.1} completed={completed} volume={volume:.3}");
                self.first_completion = true;
            }
            if volume > 0.0 && completed > self.unmute_completed && !self.first_unmuted_completion {
                eprintln!("audio first unmuted device completion elapsed_ms={elapsed:.1} after_unmute_ms={:.1} completed={completed}",
                    self.unmuted_since.map_or(0.0, |start| start.elapsed().as_secs_f64() * 1000.0));
                self.first_unmuted_completion = true;
            }
        }
        // This is device-buffer starvation, not proof of lost codec samples.
        let drained = !paused && submitted > 0 && completed >= submitted;
        if drained && !self.drained { self.underruns += 1; }
        self.drained = drained;
        self.last_completed = completed;
        if self.report.elapsed() >= Duration::from_secs(1) {
            eprintln!("audio delivery submitted={submitted} completed={completed} buffered_frames={} starvation_events={} pcm_gap_frames={} pcm_overlap_frames={} pcm_queue_timeouts={} device_wait_ms={:.1} paused={paused}",
                submitted.saturating_sub(completed), self.underruns,
                self.gap_frames, self.overlap_frames, self.queue_timeouts,
                self.device_wait.as_secs_f64() * 1000.0);
            self.report = Instant::now();
            self.queue_timeouts = 0;
            self.device_wait = Duration::ZERO;
        }
    }
}

#[derive(Default)]
struct AudioTimeline {
    submitted: u64,
    segments: VecDeque<(u64, u64, f64, u32)>,
}

impl AudioTimeline {
    fn submit(&mut self, control: &Control, generation: u64, timestamp: f64, frames: usize, rate: u32) {
        if self.submitted == 0 { control.publish_audio_time(generation, timestamp); }
        self.segments.push_back((self.submitted, frames as u64, timestamp, rate));
        self.submitted += frames as u64;
    }

    fn publish(&mut self, control: &Control, generation: u64, completed: u64) {
        while let Some(&(first, frames, timestamp, rate)) = self.segments.front() {
            let consumed = completed.saturating_sub(first).min(frames);
            if consumed == 0 { break; }
            control.publish_audio_time(generation, timestamp + consumed as f64 / f64::from(rate));
            if consumed < frames { break; }
            self.segments.pop_front();
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn audio_delivery_trace_counts_timestamp_gaps_without_rounding_noise() {
        let mut trace = super::AudioDeliveryTrace::new();
        trace.submitted(0.0, 960, 48_000);
        trace.submitted(0.02000000001, 960, 48_000);
        assert_eq!((trace.gap_frames, trace.overlap_frames), (0, 0));
        trace.submitted(0.05, 960, 48_000);
        assert_eq!(trace.gap_frames, 480);
        trace.submitted(0.06, 960, 48_000);
        assert_eq!(trace.overlap_frames, 480);
    }

    #[test]
    fn audio_delivery_trace_separates_pauses_and_repeated_empty_polls() {
        let mut trace = super::AudioDeliveryTrace::new();
        trace.observe(0, 0, 0.0, false);
        trace.observe(960, 960, 0.0, true);
        assert_eq!(trace.underruns, 0);
        trace.observe(960, 960, 0.0, false);
        trace.observe(960, 960, 0.0, false);
        assert_eq!(trace.underruns, 1);
        trace.observe(960, 1920, 1.0, false);
        assert!(!trace.first_unmuted_completion);
        trace.observe(1920, 1920, 1.0, false);
        assert_eq!(trace.underruns, 2);
        assert!(trace.first_completion && trace.first_unmuted_completion);
    }
    use super::*;
    use std::sync::Mutex;

    fn control() -> Arc<Control> {
        Arc::new(Control {
            cancelled: AtomicBool::new(false), paused: AtomicBool::new(false),
            looping: AtomicBool::new(false), volume: AtomicU32::new(1.0f32.to_bits()),
            time: AtomicU32::new(0.0f32.to_bits()), generation: AtomicU64::new(0),
            // A frozen clock makes packet-boundary assertions deterministic.
            rate: AtomicU32::new(0.0f32.to_bits()), origin: Instant::now(), anchor_ns: AtomicU64::new(0),
            completed: AtomicU64::new(0),
            audio_clock: AtomicU64::new(u64::from(f32::NAN.to_bits())),
            audio_disabled: AtomicBool::new(false),
        })
    }

    #[test]
    #[ignore = "explicit local video-only VP8 fixture; silent decoder pacing check"]
    fn video_without_audio_cannot_decode_past_bounded_clock_lead() {
        let path = std::env::var("WEBCORE_VIDEO_PACING_WEBM").expect("video-only VP8 fixture path");
        let control = control();
        let worker_control = control.clone();
        let (tx, rx) = mpsc::sync_channel(8);
        let (frames_tx, frames_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            decode_webm(worker_control.clone(), PcmSender { control: worker_control, tx },
                || Some(Box::new(std::fs::File::open(&path).unwrap())),
                |update, _stop| {
                    if let VideoUpdate::Frame { frame, .. } = update {
                        return frames_tx.send(frame.timestamp).is_ok();
                    }
                    true
                });
        });
        let first = frames_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(first <= VIDEO_LEAD);
        let deadline = Instant::now() + Duration::from_millis(300);
        while Instant::now() < deadline {
            if let Ok(timestamp) = frames_rx.recv_timeout(POLL) {
                assert!(timestamp <= VIDEO_LEAD, "future frame escaped clock bound: {timestamp}");
            }
        }
        control.time.store(1.0f32.to_bits(), Ordering::Release);
        let deadline = Instant::now() + Duration::from_secs(5);
        let timestamp = loop {
            let timestamp = frames_rx.recv_timeout(deadline.saturating_duration_since(Instant::now())).unwrap();
            if timestamp > VIDEO_LEAD { break timestamp; }
        };
        control.cancelled.store(true, Ordering::Release);
        drop(rx);
        worker.join().unwrap();
        assert!(timestamp <= 1.0 + VIDEO_LEAD);
    }

    #[derive(Default)]
    struct State {
        pcm: Vec<f32>,
        channels: usize,
        volumes: Vec<f32>,
        pauses: Vec<bool>,
        resets: usize,
    }
    struct Fake(Arc<Mutex<State>>);
    impl Output for Fake {
        fn submit(&mut self, samples: &[f32]) -> Result<usize, PcmOutputError> {
            self.0.lock().unwrap().pcm.extend_from_slice(samples);
            Ok(samples.len())
        }
        fn set_volume(&mut self, volume: f32) -> Result<(), PcmOutputError> {
            self.0.lock().unwrap().volumes.push(volume); Ok(())
        }
        fn set_paused(&mut self, paused: bool) -> Result<(), PcmOutputError> {
            self.0.lock().unwrap().pauses.push(paused); Ok(())
        }
        fn wait(&self) { std::thread::sleep(POLL); }
        fn drained(&self) -> bool { true }
        fn reset(&mut self) -> Result<(), PcmOutputError> { self.0.lock().unwrap().resets += 1; Ok(()) }
        fn completed_frames(&self) -> u64 {
            let state = self.0.lock().unwrap();
            (state.pcm.len() / state.channels.max(1)) as u64
        }
    }
    fn packet(generation: u64, timestamp: f64, value: f32) -> Packet {
        Packet { generation, timestamp, samples: AudioSamples { sample_rate: 48000, channels: 1, samples: vec![value; 480] } }
    }

    #[test]
    fn device_startup_does_not_let_video_clock_run_ahead_of_pcm() {
        let control = control();
        control.rate.store(1.0f32.to_bits(), Ordering::Release);
        let (tx, rx) = mpsc::sync_channel(8);
        tx.send(packet(0, 0.0, 0.25)).unwrap();
        drop(tx);
        let state = Arc::new(Mutex::new(State::default()));
        run_output(control.clone(), rx, |_, _| {
            std::thread::sleep(Duration::from_millis(80));
            assert_eq!(control.audio_time(), Some(0.0));
            assert_eq!(control.time(), 0.0);
            Ok(Fake(state.clone()))
        }).unwrap();
        assert_eq!(state.lock().unwrap().pcm.len(), 480);
    }

    #[test]
    fn cancellation_during_device_creation_does_not_submit_pcm() {
        let control = control();
        let (tx, rx) = mpsc::sync_channel(8);
        tx.send(packet(0, 0.0, 0.25)).unwrap();
        drop(tx);
        let state = Arc::new(Mutex::new(State::default()));
        run_output(control.clone(), rx, |_, _| {
            control.cancelled.store(true, Ordering::Release);
            Ok(Fake(state.clone()))
        }).unwrap();
        assert!(state.lock().unwrap().pcm.is_empty());
    }

    #[test]
    fn seek_during_device_creation_does_not_submit_old_pcm() {
        let control = control();
        let (tx, rx) = mpsc::sync_channel(8);
        tx.send(packet(0, 0.0, 0.25)).unwrap();
        tx.send(packet(1, 5.0, 0.5)).unwrap();
        drop(tx);
        let state = Arc::new(Mutex::new(State::default()));
        run_output(control.clone(), rx, |_, _| {
            control.time.store(5.0f32.to_bits(), Ordering::Release);
            control.generation.store(1, Ordering::Release);
            Ok(Fake(state.clone()))
        }).unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.pcm, vec![0.5; 480]);
        assert_eq!(state.resets, 1);
    }

    #[test]
    fn each_new_pcm_packet_is_validated_after_a_valid_packet() {
        for case in 0..4 {
            let control = control();
            let state = Arc::new(Mutex::new(State::default()));
            let (tx, rx) = mpsc::sync_channel(8);
            tx.send(packet(0, 0.0, 0.25)).unwrap();
            let mut invalid = packet(0, 0.01, 0.25);
            match case {
                0 => invalid.samples.samples[479] = f32::NAN,
                1 => invalid.samples.sample_rate = 0,
                2 => { invalid.samples.channels = 2; invalid.samples.samples.push(0.0); }
                _ => invalid.timestamp = f64::NAN,
            }
            tx.send(invalid).unwrap();
            drop(tx);
            assert_eq!(run_output(control, rx, |_, _| Ok(Fake(state.clone()))), Err(PcmOutputError::InvalidData));
            assert_eq!(state.lock().unwrap().pcm, vec![0.25; 480]);
        }
    }

    #[test]
    #[ignore = "explicit valid VP8/Vorbis fixture with audio shorter than video; silent"]
    fn shorter_audio_track_does_not_freeze_video_at_its_endpoint() {
        let path = std::env::var_os("WEBCORE_SHORT_AUDIO_AV_WEBM").expect("WEBCORE_SHORT_AUDIO_AV_WEBM");
        let control = control();
        let state = Arc::new(Mutex::new(State::default()));
        let (tx, rx) = mpsc::sync_channel(8);
        let output_control = control.clone();
        let output_state = state.clone();
        let output = std::thread::spawn(move || run_output(output_control, rx, |_, channels| {
            output_state.lock().unwrap().channels = usize::from(channels);
            Ok(Fake(output_state.clone()))
        }));
        let sender = PcmSender { control: control.clone(), tx };
        let decode_control = control.clone();
        let duration = Arc::new(AtomicU32::new(0));
        let metadata_duration = duration.clone();
        let video_count = Arc::new(AtomicU64::new(0));
        let decoded_video_count = video_count.clone();
        let video_time = Arc::new(AtomicU32::new(0));
        let decoded_video_time = video_time.clone();
        let decode = std::thread::spawn(move || decode_webm(decode_control, sender,
            || Some(Box::new(std::fs::File::open(&path).unwrap())),
            |sample, _stop| {
                match sample {
                    VideoUpdate::Metadata(metadata) => {
                        metadata_duration.store(metadata.duration.unwrap().to_bits(), Ordering::Release);
                    }
                    VideoUpdate::Frame { frame, .. } => {
                        decoded_video_count.fetch_add(1, Ordering::Relaxed);
                        decoded_video_time.store(frame.timestamp.to_bits(), Ordering::Release);
                    }
                }
                true
            }));
        let deadline = Instant::now() + Duration::from_secs(10);
        loop {
            let duration = f32::from_bits(duration.load(Ordering::Acquire));
            if duration > 2.0 && control.time() >= duration - 0.001
                && f32::from_bits(video_time.load(Ordering::Acquire)) >= duration - 0.05
            { break; }
            assert!(Instant::now() < deadline, "audio master stalled before the video endpoint");
            std::thread::sleep(POLL);
        }
        control.cancelled.store(true, Ordering::Release);
        decode.join().unwrap();
        output.join().unwrap().unwrap();
        // The accelerated device may outrun video decoding and trigger valid
        // keyframe catch-up. Reaching the last picture matters, not frame count.
        assert!(video_count.load(Ordering::Relaxed) > 0);
        let state = state.lock().unwrap();
        assert!(state.pcm.iter().any(|value| value.abs() > 0.0001), "real Vorbis PCM must precede the gap");
        eprintln!("short-audio fixture: {} stereo frames, endpoint {}, duration {}",
            state.pcm.len() / 2, control.time(), f32::from_bits(duration.load(Ordering::Acquire)));
        let declared_frames = (f64::from(f32::from_bits(duration.load(Ordering::Acquire))) * 48_000.0).floor() as usize;
        assert!(state.pcm.len() / 2 >= declared_frames);
        assert!(state.pcm[state.pcm.len() - 48_000 * 2..].iter().all(|&value| value == 0.0));
    }

    #[test]
    fn audio_master_clock_stalls_on_underrun_and_rejects_stale_seek_updates() {
        let control = control();
        control.rate.store(1.0f32.to_bits(), Ordering::Release);
        let mut timeline = AudioTimeline::default();
        timeline.submit(&control, 0, 0.0, 4800, 48_000);
        timeline.publish(&control, 0, 4800);
        assert!((control.time() - 0.1).abs() < 0.00001);
        std::thread::sleep(Duration::from_millis(30));
        assert!((control.time() - 0.1).abs() < 0.00001, "wall time must not outrun stalled PCM");
        timeline.submit(&control, 0, 0.1, 4800, 48_000);
        timeline.publish(&control, 0, 9600);
        assert!((control.time() - 0.2).abs() < 0.00001);
        control.time.store(5.0f32.to_bits(), Ordering::Release);
        control.generation.fetch_add(1, Ordering::AcqRel);
        timeline.submit(&control, 0, 0.2, 4800, 48_000);
        timeline.publish(&control, 0, 14400);
        assert!(control.audio_time().is_none(), "old device completion must not move a new seek");
        let mut timeline = AudioTimeline::default();
        timeline.submit(&control, 1, 5.0, 4800, 48_000);
        timeline.publish(&control, 1, 4800);
        assert!((control.time() - 5.1).abs() < 0.00001);
        control.audio_disabled.store(true, Ordering::Release);
        assert!(control.audio_time().is_none(), "a codec/device error must release the video clock");
    }

    #[test]
    fn looping_video_keeps_cumulative_audio_clock_while_audio_controls_wrap() {
        let control = control();
        control.publish_audio_time(0, 2.5);
        let mut playback = Playback { control, last_seek_revision: 0, last_rate: 1.0 };
        assert_eq!(playback.synchronize_audio_time(None, true), Some(2.5));
        assert_eq!(playback.synchronize_audio_time(Some(2.0), true), Some(0.5));
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit muted continuous native PCM playback test"]
    fn native_output_continues_for_multiple_seconds_without_ui_ticks() {
        let control = control();
        control.volume.store(0.0f32.to_bits(), Ordering::Release);
        control.rate.store(1.0f32.to_bits(), Ordering::Release);
        let (tx, rx) = mpsc::sync_channel(8);
        let worker_control = control.clone();
        let worker = std::thread::spawn(move || run_output(worker_control, rx, PcmOutput::new));
        for index in 0..150 {
            tx.send(Packet {
                generation: 0,
                timestamp: 0.1 + index as f64 * 0.02,
                samples: AudioSamples { sample_rate: 48_000, channels: 2, samples: vec![0.0; 1920] },
            }).unwrap();
        }
        drop(tx);
        worker.join().unwrap().unwrap();
        assert_eq!(control.completed.load(Ordering::Acquire), 144_000,
            "native output must retain all three seconds of continuous PCM");
        assert!((control.time() - 3.1).abs() < 0.00001);
    }
    fn until(mut condition: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(2);
        while !condition() { assert!(Instant::now() < deadline); std::thread::sleep(POLL); }
    }

    #[test]
    fn paused_pcm_is_retained_and_mute_volume_follow_controls() {
        let control = control();
        control.paused.store(true, Ordering::Release);
        control.volume.store(0.0f32.to_bits(), Ordering::Release);
        let (tx, rx) = mpsc::sync_channel(8);
        let state = Arc::new(Mutex::new(State::default()));
        let worker_control = control.clone();
        let result = state.clone();
        let worker = std::thread::spawn(move || run_output(worker_control, rx, |_, _| Ok(Fake(result.clone()))));
        tx.send(packet(0, 0.0, 0.5)).unwrap();
        until(|| state.lock().unwrap().pauses.last() == Some(&true));
        assert!(state.lock().unwrap().pcm.is_empty());
        assert_eq!(control.time(), 0.0, "preparing the paused device must not start its clock");
        control.paused.store(false, Ordering::Release);
        until(|| state.lock().unwrap().pcm.len() == 480);
        assert_eq!(state.lock().unwrap().volumes.last(), Some(&0.0));
        control.paused.store(true, Ordering::Release);
        until(|| state.lock().unwrap().pauses.last() == Some(&true));
        control.volume.store(0.25f32.to_bits(), Ordering::Release);
        tx.send(packet(0, 0.01, 0.75)).unwrap();
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(state.lock().unwrap().pcm.len(), 480);
        control.paused.store(false, Ordering::Release);
        until(|| state.lock().unwrap().pcm.len() == 960);
        assert_eq!(state.lock().unwrap().volumes.last(), Some(&0.25));
        drop(tx);
        worker.join().unwrap().unwrap();
    }

    #[test]
    fn seek_resets_device_discards_old_generation_and_trims_pcm() {
        let control = control();
        let (tx, rx) = mpsc::sync_channel(8);
        let state = Arc::new(Mutex::new(State::default()));
        let result = state.clone();
        let worker_control = control.clone();
        let worker = std::thread::spawn(move || run_output(worker_control, rx, |_, _| Ok(Fake(result.clone()))));
        tx.send(packet(0, 0.0, 0.1)).unwrap();
        until(|| state.lock().unwrap().pcm.len() == 480);
        control.time.store(1.005f32.to_bits(), Ordering::Release);
        control.generation.store(1, Ordering::Release);
        tx.send(packet(0, 1.0, 0.9)).unwrap();
        tx.send(packet(1, 1.0, 0.3)).unwrap();
        drop(tx);
        worker.join().unwrap().unwrap();
        let state = state.lock().unwrap();
        assert_eq!(state.resets, 1);
        assert!(state.pcm[480..].iter().all(|sample| *sample == 0.3));
        assert!((720..=721).contains(&state.pcm.len()));
    }

    #[test]
    fn audio_clock_advances_without_ui_updates() {
        let control = control();
        control.rate.store(1.0f32.to_bits(), Ordering::Release);
        let (tx, rx) = mpsc::sync_channel(8);
        let state = Arc::new(Mutex::new(State::default()));
        let result = state.clone();
        let worker_control = control.clone();
        let worker = std::thread::spawn(move || {
            run_output(worker_control, rx, |_, _| Ok(Fake(result.clone())))
        });
        for timestamp in [0.1, 0.2, 0.4] {
            tx.send(packet(0, timestamp, 0.5)).unwrap();
        }
        drop(tx);
        worker.join().unwrap().unwrap();
        assert_eq!(state.lock().unwrap().pcm.len(), 1440);
        assert!(control.time() >= 0.3);
    }

    #[test]
    fn dropping_handle_cancels_a_full_paused_queue_without_ui_wait() {
        let control = control();
        control.paused.store(true, Ordering::Release);
        let playback = Playback { control: control.clone(), last_seek_revision: 0, last_rate: 1.0 };
        let (tx, _rx) = mpsc::sync_channel(8);
        for _ in 0..8 { tx.send(packet(0, 0.0, 0.0)).unwrap(); }
        let sender = PcmSender { control: control.clone(), tx };
        let worker = std::thread::spawn(move || sender.send(0, 0.0, packet(0, 0.0, 0.0).samples));
        let start = Instant::now();
        drop(playback);
        assert!(start.elapsed() < Duration::from_millis(50));
        assert!(!worker.join().unwrap());
    }

    #[test]
    fn loop_clock_is_continuous_and_backward_seek_changes_generation() {
        let control = control();
        let mut playback = Playback { control: control.clone(), last_seek_revision: 0, last_rate: 1.0 };
        assert!(!playback.update(1.01, false, false, 1.0, true, 1.0, 0));
        assert_eq!(control.generation(), 0);
        assert!(control.looping());
        assert!(playback.update(0.0, false, false, 1.0, true, 1.0, 1));
        assert_eq!(control.generation(), 1);
        assert!(control.time() < 0.01);
    }

    #[test]
    fn clock_corrections_do_not_restart_audio_without_an_explicit_seek() {
        let control = control();
        let mut playback = Playback { control: control.clone(), last_seek_revision: 0, last_rate: 1.0 };
        for time in [0.0, 0.5, 5.0, 4.0, 30.0, 29.0] {
            assert!(!playback.update(time, false, false, 1.0, false, 1.0, 0));
            assert_eq!(control.generation(), 0);
        }
        assert!(playback.update(12.0, false, false, 1.0, false, 1.0, 1));
        assert_eq!(control.generation(), 1);
        assert!(!playback.update(12.5, false, false, 1.0, false, 1.0, 1));
        assert_eq!(control.generation(), 1);
    }
}
