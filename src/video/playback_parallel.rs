//! A single incremental demuxer, with independent bounded codec workers.

use super::{Control, Packet, PcmSender, PendingAudioClock, VideoUpdate, POLL, AUDIO_LEAD, VIDEO_LEAD};
use super::super::{AudioSamples, MediaSample, StreamingMediaDecoder};
use super::super::webm::{VideoPacket, WebmMediaDecoder, WebmPacket, WebmStream};
use std::collections::VecDeque;
use std::io::Read;
use std::sync::{Arc, atomic::{AtomicBool, AtomicUsize, Ordering}, mpsc};
use std::time::Instant;

const VIDEO_BYTES: usize = 32 * 1024 * 1024;

struct AudioPreroll {
    cutoff_ns: i128,
    previous: Option<webmedia::audio::webm::WebmAudioBlock>,
    reached: bool,
}

impl AudioPreroll {
    fn new(time: f64, track: Option<&webmedia::audio::webm::WebmAudioTrack>) -> Self {
        let cutoff_ns = track.map_or(0, |track| {
            (time.max(0.0) * 1e9) as i128 + i128::from(track.codec_delay_ns)
                - i128::from(track.seek_pre_roll_ns)
        });
        Self { cutoff_ns, previous: None, reached: time <= 0.0 }
    }

    fn push(&mut self, block: webmedia::audio::webm::WebmAudioBlock)
        -> (Option<webmedia::audio::webm::WebmAudioBlock>, Option<webmedia::audio::webm::WebmAudioBlock>)
    {
        if self.reached { return (None, Some(block)); }
        if i128::from(block.timestamp_ns) < self.cutoff_ns {
            self.previous = Some(block);
            return (None, None);
        }
        self.reached = true;
        // Retain the boundary block too: lacing may cross the preroll cutoff,
        // and Vorbis needs the preceding window for its first overlap.
        (self.previous.take(), Some(block))
    }
}

pub(super) struct Reservation {
    bytes: usize,
    total: Arc<AtomicUsize>,
}

impl Reservation {
    pub(super) fn acquire(total: &Arc<AtomicUsize>, bytes: usize) -> Option<Self> {
        total.fetch_update(Ordering::AcqRel, Ordering::Acquire,
            |used| used.checked_add(bytes).filter(|next| *next <= VIDEO_BYTES)).ok()?;
        Some(Self { bytes, total: total.clone() })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, AtomicU64};
    use std::time::{Duration, Instant};

    #[test]
    fn seeking_keeps_track_preroll_and_the_overlapping_boundary_block() {
        use webmedia::audio::webm::{WebmAudioBlock, WebmAudioCodec, WebmAudioTrack};
        let track = WebmAudioTrack { number: 1, codec: WebmAudioCodec::Opus,
            sampling_frequency: 48_000.0, channels: 2, codec_private: vec![],
            codec_delay_ns: 6_500_000, seek_pre_roll_ns: 80_000_000,
            default_duration_ns: None };
        let mut preroll = AudioPreroll::new(60.0, Some(&track));
        for timestamp_ns in [0, 10_000_000_000, 59_900_000_000] {
            let (previous, current) = preroll.push(WebmAudioBlock { timestamp_ns,
                packets: vec![], discard_padding_ns: None });
            assert!(previous.is_none() && current.is_none());
        }
        let (previous, current) = preroll.push(WebmAudioBlock { timestamp_ns: 59_940_000_000,
            packets: vec![], discard_padding_ns: None });
        assert_eq!(previous.unwrap().timestamp_ns, 59_900_000_000);
        assert_eq!(current.unwrap().timestamp_ns, 59_940_000_000);
        let (previous, current) = preroll.push(WebmAudioBlock { timestamp_ns: 59_960_000_000,
            packets: vec![], discard_padding_ns: None });
        assert!(previous.is_none());
        assert!(current.is_some());
    }

    #[test]
    fn initial_audio_playback_does_not_skip_codec_delay_or_first_window() {
        let mut preroll = AudioPreroll::new(0.0, None);
        let (previous, current) = preroll.push(webmedia::audio::webm::WebmAudioBlock {
            timestamp_ns: 0, packets: vec![], discard_padding_ns: None });
        assert!(previous.is_none());
        assert!(current.is_some());
    }

    #[test]
    #[ignore = "explicit local WebM fixture; silent seek/preroll regression"]
    fn seeking_does_not_decode_the_entire_audio_prefix_or_emit_old_pictures() {
        let path = std::env::var("WEBCORE_PARALLEL_WEBM").expect("WebM fixture");
        let control = control(false);
        control.time.store(60.0f32.to_bits(), Ordering::Release);
        control.rate.store(0.0f32.to_bits(), Ordering::Release);
        let (pcm_tx, pcm_rx) = mpsc::sync_channel(8);
        let (video_tx, video_rx) = mpsc::channel();
        let worker_control = control.clone();
        let worker = std::thread::spawn(move || decode_webm(worker_control.clone(),
            PcmSender { control: worker_control, tx: pcm_tx },
            || Some(Box::new(std::fs::File::open(&path).unwrap())),
            |sample, _stop| {
                if let VideoUpdate::Frame { frame, .. } = sample {
                    video_tx.send(frame.timestamp).unwrap();
                }
                true
            }));
        let packet = pcm_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let deadline = Instant::now() + Duration::from_secs(5);
        let picture = loop {
            if let Ok(picture) = video_rx.try_recv() { break Some(picture); }
            if Instant::now() >= deadline { break None; }
            let _ = pcm_rx.recv_timeout(POLL);
        };
        control.cancelled.store(true, Ordering::Release);
        drop(pcm_rx);
        worker.join().unwrap();
        assert!(packet.timestamp >= 59.8 && packet.timestamp <= 60.1,
            "unexpected preroll timestamp {}", packet.timestamp);
        assert!(packet.samples.samples.iter().any(|sample| sample.abs() > 0.0001));
        assert!(picture.unwrap() >= 60.0, "pre-seek pictures must not reach presentation");
    }

    fn control(paused: bool) -> Arc<Control> {
        Arc::new(Control {
            cancelled: AtomicBool::new(false), paused: AtomicBool::new(paused),
            looping: AtomicBool::new(false), volume: AtomicU32::new(1.0f32.to_bits()),
            time: AtomicU32::new(0.0f32.to_bits()), rate: AtomicU32::new(1.0f32.to_bits()),
            origin: Instant::now(), anchor_ns: AtomicU64::new(0), generation: AtomicU64::new(0),
            completed: AtomicU64::new(0), audio_clock: AtomicU64::new(u64::from(f32::NAN.to_bits())),
            audio_disabled: AtomicBool::new(false),
        })
    }

    #[test]
    #[ignore = "set WEBCORE_PARALLEL_WEBM to Partyfire (or another audio/video WebM)"]
    fn stalled_video_emit_does_not_stop_pcm_delivery() {
        let path = std::env::var_os("WEBCORE_PARALLEL_WEBM").expect("WEBCORE_PARALLEL_WEBM");
        let control = control(false);
        let gate = Arc::new(AtomicBool::new(false));
        let held = Arc::new(AtomicBool::new(false));
        let (tx, rx) = mpsc::sync_channel(8);
        let worker_control = control.clone();
        let worker_gate = gate.clone();
        let worker_held = held.clone();
        let worker = std::thread::spawn(move || {
            let pcm = PcmSender { control: worker_control.clone(), tx };
            decode_webm(worker_control.clone(), pcm,
                || Some(Box::new(std::fs::File::open(&path).unwrap())),
                |update, stop| {
                    if matches!(update, VideoUpdate::Frame { .. }) {
                        worker_held.store(true, Ordering::Release);
                        while !worker_gate.load(Ordering::Acquire) && !worker_control.cancelled()
                            && !stop.load(Ordering::Acquire) {
                            std::thread::sleep(POLL);
                        }
                    }
                    !worker_control.cancelled() && !stop.load(Ordering::Acquire)
                });
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut first = None;
        let mut delivered = 0.0;
        let mut packets = 0;
        while Instant::now() < deadline && delivered < 2.0 {
            match rx.recv_timeout(POLL) {
                Ok(packet) => {
                    let samples = &packet.samples;
                    let end = packet.timestamp + (samples.samples.len() / usize::from(samples.channels)) as f64
                        / f64::from(samples.sample_rate);
                    // Emulate the separate output consumer advancing its master clock.
                    control.publish_audio_time(0, end);
                    if held.load(Ordering::Acquire) {
                        let start = *first.get_or_insert(packet.timestamp);
                        delivered = end - start;
                        packets += 1;
                    }
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        control.cancelled.store(true, Ordering::Release);
        gate.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(held.load(Ordering::Acquire), "video never reached the emit gate");
        assert!(delivered >= 2.0 && packets > 8,
            "audio must keep decoding beyond the PCM capacity while video is held: {packets} packets, {delivered}s");
        assert!(!control.audio_disabled.load(Ordering::Acquire), "must deliver actual decoded PCM");
    }

    #[test]
    #[ignore = "set WEBCORE_PARALLEL_WEBM to Partyfire (audio/video WebM); bounded delivery regression"]
    fn bounded_video_emit_preserves_pcm_and_abandons_seek_then_cancels() {
        let path = std::env::var_os("WEBCORE_PARALLEL_WEBM").expect("WEBCORE_PARALLEL_WEBM");
        let control = control(false);
        let blocked_generation = Arc::new(AtomicU64::new(0));
        let abandoned = Arc::new(AtomicBool::new(false));
        let (pcm_tx, pcm_rx) = mpsc::sync_channel(8);
        let (video_tx, video_rx) = mpsc::sync_channel(8);
        let (done_tx, done_rx) = mpsc::channel();
        let worker_control = control.clone();
        let worker_blocked = blocked_generation.clone();
        let worker_abandoned = abandoned.clone();
        let worker = std::thread::spawn(move || {
            let pcm = PcmSender { control: worker_control.clone(), tx: pcm_tx };
            decode_webm(worker_control.clone(), pcm,
                || Some(Box::new(std::fs::File::open(&path).unwrap())),
                |mut update, stop| {
                    if matches!(update, VideoUpdate::Metadata(_)) { return true; }
                    loop {
                        if worker_control.cancelled() || stop.load(Ordering::Acquire) { return false; }
                        let VideoUpdate::Frame { generation, .. } = &update else { unreachable!() };
                        if *generation != worker_control.generation() {
                            worker_abandoned.store(true, Ordering::Release);
                            return true;
                        }
                        match video_tx.try_send(update) {
                            Ok(()) => return true,
                            Err(mpsc::TrySendError::Full(value)) => {
                                let VideoUpdate::Frame { generation, .. } = &value else { unreachable!() };
                                worker_blocked.store(*generation + 1, Ordering::Release);
                                update = value;
                                std::thread::sleep(POLL);
                            }
                            Err(mpsc::TrySendError::Disconnected(_)) => return false,
                        }
                    }
                });
            done_tx.send(()).unwrap();
        });
        let deadline = Instant::now() + Duration::from_secs(30);
        let mut first = None;
        let mut delivered = 0.0;
        let mut packets = 0;
        let mut actual_pcm = false;
        while Instant::now() < deadline && delivered < 2.0 {
            if let Ok(packet) = pcm_rx.recv_timeout(POLL) {
                let samples = &packet.samples;
                let end = packet.timestamp + (samples.samples.len() / usize::from(samples.channels)) as f64
                    / f64::from(samples.sample_rate);
                control.publish_audio_time(packet.generation, end);
                if packet.generation == 0 && blocked_generation.load(Ordering::Acquire) == 1 {
                    let start = *first.get_or_insert(packet.timestamp);
                    delivered = end - start;
                    packets += 1;
                    actual_pcm |= samples.samples.iter().any(|sample| sample.abs() > 0.0001);
                }
            }
        }
        // Keep all eight old pictures queued through seek: no consumer frees space.
        control.time.store(0.0f32.to_bits(), Ordering::Release);
        control.generation.fetch_add(1, Ordering::AcqRel);
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut new_pcm = false;
        while Instant::now() < deadline
            && !(new_pcm && blocked_generation.load(Ordering::Acquire) == 2)
        {
            if let Ok(packet) = pcm_rx.recv_timeout(POLL) {
                if packet.generation != 1 { continue; }
                let samples = &packet.samples;
                let end = packet.timestamp + (samples.samples.len() / usize::from(samples.channels)) as f64
                    / f64::from(samples.sample_rate);
                control.publish_audio_time(1, end);
                new_pcm = true;
            }
        }
        control.cancelled.store(true, Ordering::Release);
        let finished = done_rx.recv_timeout(Duration::from_secs(5)).is_ok();
        if finished { worker.join().unwrap(); }
        assert!(finished, "cancel must join while decoded video channel remains full");
        assert!(delivered >= 2.0 && packets > 8 && actual_pcm,
            "actual PCM must advance beyond queue capacity during video backpressure: {packets} packets, {delivered}s");
        assert!(abandoned.load(Ordering::Acquire), "seek must abandon blocked old-generation frame");
        assert!(new_pcm && blocked_generation.load(Ordering::Acquire) == 2,
            "seek must reopen and reach bounded video emit again without draining old pictures");
        assert!(!control.audio_disabled.load(Ordering::Acquire));
        let mut pictures = 0;
        while let Ok(update) = video_rx.try_recv() {
            let VideoUpdate::Frame { generation, .. } = update else { unreachable!() };
            assert_eq!(generation, 0, "old queue must remain unchanged through seek");
            pictures += 1;
        }
        assert_eq!(pictures, 8);
    }

    #[test]
    fn paused_video_preview_does_not_advance_time_and_cancels() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../webmedia/tests/fixtures/vp8-motion.webm");
        let control = control(true);
        let worker_control = control.clone();
        let (pcm_tx, _pcm_rx) = mpsc::sync_channel(8);
        let (preview_tx, preview_rx) = mpsc::channel();
        let worker = std::thread::spawn(move || {
            decode_webm(worker_control.clone(), PcmSender { control: worker_control, tx: pcm_tx },
                || Some(Box::new(std::fs::File::open(path).unwrap())),
                |update, _stop| {
                    if matches!(update, VideoUpdate::Frame { .. }) { preview_tx.send(()).unwrap(); }
                    true
                });
        });
        let preview = preview_rx.recv_timeout(Duration::from_secs(10));
        std::thread::sleep(Duration::from_millis(30));
        let time = control.time();
        let extra_preview = preview_rx.try_recv().is_ok();
        control.cancelled.store(true, Ordering::Release);
        worker.join().unwrap();
        assert!(preview.is_ok(), "paused preload must produce a first preview");
        assert_eq!(time, 0.0);
        assert!(!extra_preview, "paused preload must stop after the first preview");
    }

    #[test]
    fn read_failure_interrupts_full_presentation_callback() {
        struct FaultReader {
            prefix: std::io::Cursor<Vec<u8>>,
            control: Arc<Control>,
            blocked: Arc<AtomicBool>,
            injected: Arc<AtomicBool>,
        }
        impl Read for FaultReader {
            fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
                if bytes.is_empty() { return Ok(0); }
                if self.prefix.position() < self.prefix.get_ref().len() as u64 {
                    return self.prefix.read(bytes);
                }
                let deadline = Instant::now() + Duration::from_secs(10);
                while !self.blocked.load(Ordering::Acquire) {
                    if self.control.cancelled() {
                        return Err(std::io::ErrorKind::Interrupted.into());
                    }
                    if Instant::now() >= deadline {
                        return Err(std::io::ErrorKind::TimedOut.into());
                    }
                    std::thread::sleep(POLL);
                }
                self.injected.store(true, Ordering::Release);
                Err(std::io::Error::other("injected read fault after presentation backpressure"))
            }
        }
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../webmedia/tests/fixtures/vp8-motion.webm");
        let bytes = std::fs::read(path).unwrap();
        // Use the real parser to stop just after the first complete keyframe.
        // Feeding the whole two-second fixture could hit video-only demux pacing
        // before reaching our injected read, obscuring the callback failure.
        let mut stream = WebmStream::new();
        let mut prefix_end = None;
        for (index, chunk) in bytes.chunks(128).enumerate() {
            let packets = stream.push_packets(chunk).unwrap();
            if packets.iter().any(|packet| matches!(packet,
                WebmPacket::Video(packet) if packet.key_frame))
            {
                prefix_end = Some(((index + 1) * 128).min(bytes.len()));
                break;
            }
        }
        let prefix = bytes[..prefix_end.expect("fixture must contain a complete keyframe")].to_vec();
        let control = control(true);
        let blocked = Arc::new(AtomicBool::new(false));
        let injected = Arc::new(AtomicBool::new(false));
        let (pcm_tx, pcm_rx) = mpsc::sync_channel(8);
        let (video_tx, video_rx) = mpsc::sync_channel(1);
        video_tx.try_send(()).unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        let worker_control = control.clone();
        let worker_blocked = blocked.clone();
        let reader_blocked = blocked.clone();
        let reader_injected = injected.clone();
        let worker = std::thread::spawn(move || {
            let mut prefix = Some(prefix);
            decode_webm(worker_control.clone(),
                PcmSender { control: worker_control.clone(), tx: pcm_tx },
                || prefix.take().map(|prefix| Box::new(FaultReader {
                    prefix: std::io::Cursor::new(prefix), control: worker_control.clone(),
                    blocked: reader_blocked.clone(), injected: reader_injected.clone(),
                }) as Box<dyn Read + Send>),
                |update, stop| {
                    let VideoUpdate::Frame { generation, .. } = update else { return true; };
                    loop {
                        if worker_control.cancelled() || stop.load(Ordering::Acquire) { return false; }
                        if worker_control.generation() != generation { return true; }
                        match video_tx.try_send(()) {
                            Ok(()) => return true,
                            Err(mpsc::TrySendError::Full(_)) => {
                                worker_blocked.store(true, Ordering::Release);
                                std::thread::sleep(POLL);
                            }
                            Err(mpsc::TrySendError::Disconnected(_)) => return false,
                        }
                    }
                });
            let _ = done_tx.send(());
        });
        let deadline = Instant::now() + Duration::from_secs(10);
        while !injected.load(Ordering::Acquire) && Instant::now() < deadline {
            std::thread::sleep(POLL);
        }
        let fault_observed = injected.load(Ordering::Acquire);
        let finished_before_cleanup = done_rx.recv_timeout(Duration::from_secs(1)).is_ok();
        let control_unchanged = !control.cancelled() && control.generation() == 0;
        // Always unblock every test-owned wait and join before asserting the red
        // expectation. Never leave the deliberately wedged decoder behind.
        control.cancelled.store(true, Ordering::Release);
        drop(video_rx);
        drop(pcm_rx);
        worker.join().unwrap();
        assert!(blocked.load(Ordering::Acquire), "fixture never blocked presentation");
        assert!(fault_observed, "reader fault was not injected after presentation blocked");
        assert!(control_unchanged, "pass failure must not cancel playback or change seek generation");
        assert!(finished_before_cleanup,
            "read failure did not terminate the pass until external cancellation/disconnection");
    }

    #[test]
    fn pass_failure_interrupts_full_paused_pcm_queue() {
        let control = control(true);
        let stop = Arc::new(AtomicBool::new(false));
        let (tx, _rx) = mpsc::sync_channel(8);
        let samples = || AudioSamples { sample_rate: 48000, channels: 1, samples: vec![0.1; 960] };
        for _ in 0..8 {
            tx.try_send(Packet { generation: 0, timestamp: 0.0, samples: samples() }).ok().unwrap();
        }
        let worker_stop = stop.clone();
        let worker = std::thread::spawn(move || {
            send_pcm(&PcmSender { control, tx }, 0, 0.0, samples(), &worker_stop)
        });
        std::thread::sleep(POLL);
        stop.store(true, Ordering::Release);
        assert!(!worker.join().unwrap());
    }

    #[test]
    fn dropping_encoded_prefix_releases_byte_reservations() {
        let total = Arc::new(AtomicUsize::new(1024));
        let reservation = Reservation { bytes: 1024, total: total.clone() };
        drop(reservation);
        assert_eq!(total.load(Ordering::Acquire), 0);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) { self.total.fetch_sub(self.bytes, Ordering::AcqRel); }
}

struct EncodedVideo {
    packet: VideoPacket,
    epoch: u64,
    // Also needed when catch-up skips a complete, otherwise valid GOP.
    reset: Option<WebmMediaDecoder>,
    _reservation: Reservation,
}

enum AudioInput {
    Packet(WebmPacket),
    Finish { duration: Option<f32>, video_end: Option<f64> },
}

enum VideoInput {
    Packet(EncodedVideo),
    Finish,
}

pub(super) fn active(control: &Control, generation: u64, stop: &AtomicBool) -> bool {
    !control.cancelled() && control.generation() == generation && !stop.load(Ordering::Acquire)
}

pub(super) fn send<T>(tx: &mpsc::SyncSender<T>, mut value: T, control: &Control,
    generation: u64, stop: &AtomicBool) -> bool
{
    while active(control, generation, stop) {
        match tx.try_send(value) {
            Ok(()) => return true,
            Err(mpsc::TrySendError::Full(returned)) => value = returned,
            Err(mpsc::TrySendError::Disconnected(_)) => return false,
        }
        std::thread::sleep(POLL);
    }
    false
}

// PcmSender::send only observes generation/cancellation. A local pass failure
// must also interrupt PCM backpressure so scoped joins work while paused.
pub(super) fn send_pcm(pcm: &PcmSender, generation: u64, timestamp: f64, samples: AudioSamples,
    stop: &AtomicBool) -> bool
{
    send(&pcm.tx, Packet { generation, timestamp, samples }, &pcm.control, generation, stop)
}

pub(super) fn disable_audio(control: &Control, generation: u64) {
    if control.generation() == generation {
        control.audio_disabled.store(true, Ordering::Release);
    }
}

pub(super) fn pad_audio(control: &Control, pcm: &PcmSender, generation: u64, offset: f64,
    stop: &AtomicBool, end: &mut f64, duration: f64, rate: u32, channels: u16)
{
    let mut remaining = ((duration - *end).max(0.0) * f64::from(rate)).ceil() as u64;
    while remaining > 0 && active(control, generation, stop) {
        let frames = remaining.min(4096) as usize;
        let samples = AudioSamples { sample_rate: rate, channels,
            samples: vec![0.0; frames * usize::from(channels)] };
        if !send_pcm(pcm, generation, *end + offset, samples, stop) {
            if active(control, generation, stop) { disable_audio(control, generation); }
            break;
        }
        *end += frames as f64 / f64::from(rate);
        remaining -= frames as u64;
    }
}

fn audio_worker(control: &Control, pcm: &PcmSender, generation: u64, offset: f64,
    stop: &AtomicBool, rx: mpsc::Receiver<AudioInput>, mut decoder: WebmMediaDecoder,
    startup: &Instant, trace_audio: bool, pending_clock: &PendingAudioClock<'_>) -> f64
{
    let mut end = 0.0f64;
    let mut format = None;
    let mut failed = false;
    let mut first_audio = true;
    while active(control, generation, stop) {
        let input = match rx.recv_timeout(POLL) {
            Ok(input) => input,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        match input {
            AudioInput::Finish { duration, video_end } => {
                // Silence is only a successful track's declared trailing gap,
                // never a substitute for an absent or broken audio decoder.
                if !failed && video_end.is_some()
                    && let (Some((rate, channels)), Some(duration)) = (format, duration)
                {
                    pad_audio(control, pcm, generation, offset, stop, &mut end,
                        f64::from(duration), rate, channels);
                }
                break;
            }
            AudioInput::Packet(packet) => {
                if failed { continue; }
                let mut result = decoder.push_packet(packet);
                loop {
                    if !active(control, generation, stop) { pending_clock.release(); return end; }
                    let samples = match result {
                        Ok(samples) => samples,
                        Err(error) => {
                            eprintln!("WebM audio decode failed (no substitute PCM): {error:?}");
                            failed = true;
                            disable_audio(control, generation);
                            break;
                        }
                    };
                    for sample in samples {
                        match sample {
                            MediaSample::Audio { timestamp_ns, samples } => {
                                if samples.sample_rate != 0 && samples.channels != 0
                                    && !samples.samples.is_empty()
                                {
                                    if trace_audio && first_audio {
                                        eprintln!("audio first PCM startup_ms={:.1}",
                                            startup.elapsed().as_secs_f64() * 1000.0);
                                    }
                                    first_audio = false;
                                    format = Some((samples.sample_rate, samples.channels));
                                    end = end.max(timestamp_ns as f64 / 1e9
                                        + (samples.samples.len() / usize::from(samples.channels)) as f64
                                            / f64::from(samples.sample_rate));
                                }
                                let eligible_end = timestamp_ns as f64 / 1e9 + offset
                                    + if samples.sample_rate != 0 && samples.channels != 0 {
                                        (samples.samples.len() / usize::from(samples.channels)) as f64 / f64::from(samples.sample_rate)
                                    } else { 0.0 };
                                if !send_pcm(pcm, generation, timestamp_ns as f64 / 1e9 + offset, samples, stop) {
                                    if active(control, generation, stop) {
                                        eprintln!("WebM audio output unavailable; video continues without audio");
                                        disable_audio(control, generation);
                                    }
                                    failed = true;
                                    break;
                                }
                                pending_clock.handoff(eligible_end);
                            }
                            MediaSample::AudioError(error) => {
                                eprintln!("WebM audio decode failed (no substitute PCM): {error:?}");
                                failed = true;
                                disable_audio(control, generation);
                                break;
                            }
                            MediaSample::Video(_) => unreachable!("audio queue contains only audio"),
                        }
                    }
                    if failed || !decoder.has_buffered_samples() { break; }
                    result = decoder.drain_packet();
                }
            }
        }
    }
    pending_clock.release();
    end
}

fn video_worker(control: &Control, generation: u64, offset: f32, stop: &AtomicBool,
    failed: &AtomicBool,
    rx: mpsc::Receiver<VideoInput>, mut decoder: WebmMediaDecoder,
    emit: &mut (impl FnMut(VideoUpdate, &AtomicBool) -> bool + Send))
{
    let mut pending = VecDeque::new();
    let mut finished = false;
    let mut epoch = None;
    let mut first = true;
    let seek_time = f32::from_bits(control.time.load(Ordering::Acquire));
    let trace = crate::profile::media_enabled()
        || std::env::var_os("WEBCORE_TRACE_VIDEO_DELIVERY").is_some();
    let mut interval = Instant::now();
    let mut decode_time = std::time::Duration::ZERO;
    let mut pace_time = std::time::Duration::ZERO;
    let mut emit_time = std::time::Duration::ZERO;
    let mut frames = 0;
    let mut resets = 0;
    let mut catchup_packets = 0;
    let mut seek_frames = 0;
    while active(control, generation, stop) {
        if pending.is_empty() && !finished {
            match rx.recv_timeout(POLL) {
                Ok(VideoInput::Packet(packet)) => pending.push_back(packet),
                Ok(VideoInput::Finish) | Err(mpsc::RecvTimeoutError::Disconnected) => finished = true,
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
            }
        }
        // Drain a bounded prefix, not an unbounded moving producer queue.
        for _ in pending.len()..256 {
            match rx.try_recv() {
                Ok(VideoInput::Packet(packet)) => pending.push_back(packet),
                Ok(VideoInput::Finish) | Err(mpsc::TryRecvError::Disconnected) => { finished = true; break; }
                Err(mpsc::TryRecvError::Empty) => break,
            }
        }
        if let Some(clock) = control.audio_time().or_else(|| (seek_time > offset).then_some(seek_time)) {
            let key = pending.iter().rposition(|packet|
                packet.packet.key_frame && packet.packet.timestamp + offset <= clock);
            if let Some(index) = key.filter(|index| *index > 0) {
                if trace { catchup_packets += index; }
                pending.drain(..index);
                epoch = None;
            }
        }
        let Some(mut encoded) = pending.pop_front() else {
            if finished { break; }
            continue;
        };
        while control.paused() && !first && active(control, generation, stop) {
            std::thread::sleep(POLL);
        }
        if !active(control, generation, stop) { break; }
        if epoch != Some(encoded.epoch) {
            if !encoded.packet.key_frame { continue; }
            let Some(reset) = encoded.reset.take() else { continue; };
            decoder = reset;
            epoch = Some(encoded.epoch);
            if trace { resets += 1; }
        }
        let started = trace.then(Instant::now);
        let mut result = decoder.push_packet(WebmPacket::Video(encoded.packet));
        if let Some(started) = started { decode_time += started.elapsed(); }
        loop {
            if !active(control, generation, stop) { return; }
            let samples = match result {
                Ok(samples) => samples,
                Err(error) => {
                    eprintln!("WebM video decode failed: {error:?}");
                    failed.store(true, Ordering::Release);
                    return;
                }
            };
            for sample in samples {
                if let MediaSample::Video(mut frame) = sample {
                    frame.timestamp += offset;
                    if frame.timestamp < seek_time {
                        if trace { seek_frames += 1; }
                        continue;
                    }
                    let started = trace.then(Instant::now);
                    while ((!first && control.paused()) || frame.timestamp > control.time() + VIDEO_LEAD)
                        && active(control, generation, stop)
                    { std::thread::sleep(POLL); }
                    if !active(control, generation, stop) { return; }
                    if let Some(started) = started { pace_time += started.elapsed(); }
                    let timestamp = frame.timestamp;
                    let started = trace.then(Instant::now);
                    if !emit(VideoUpdate::Frame { generation, frame }, stop) {
                        stop.store(true, Ordering::Release);
                        return;
                    }
                    if let Some(started) = started {
                        emit_time += started.elapsed();
                        frames += 1;
                        if interval.elapsed().as_secs_f64() >= 1.0 {
                            let (queue_rejects, channel_drops) = crate::profile::video_backpressure();
                            eprintln!("video delivery generation={generation} frames={frames} interval_ms={:.1} decode_ms={:.1} pace_ms={:.1} emit_ms={:.1} resets={resets} catchup_packets={catchup_packets} seek_frames={seek_frames} queue_rejected_frames={queue_rejects} channel_dropped_frames={channel_drops} queue={} pts={timestamp:.3} clock={:.3}",
                                interval.elapsed().as_secs_f64() * 1000.0,
                                decode_time.as_secs_f64() * 1000.0,
                                pace_time.as_secs_f64() * 1000.0,
                                emit_time.as_secs_f64() * 1000.0, pending.len(), control.time());
                            interval = Instant::now();
                            decode_time = std::time::Duration::ZERO;
                            pace_time = std::time::Duration::ZERO;
                            emit_time = std::time::Duration::ZERO;
                            frames = 0;
                            resets = 0;
                            catchup_packets = 0;
                            seek_frames = 0;
                        }
                    }
                    first = false;
                }
            }
            if !decoder.has_buffered_samples() { break; }
            let started = trace.then(Instant::now);
            result = decoder.drain_packet();
            if let Some(started) = started { decode_time += started.elapsed(); }
        }
        // Keep the byte reservation alive through all frames in this packet.
        drop(encoded._reservation);
    }
}

pub(super) fn decode_webm(control: Arc<Control>, pcm: PcmSender,
    mut open: impl FnMut() -> Option<Box<dyn Read + Send>>,
    mut emit: impl FnMut(VideoUpdate, &AtomicBool) -> bool + Send)
{
    let mut bytes = [0; 16 * 1024];
    let mut metadata_sent = false;
    let mut duration: Option<f32> = None;
    let mut offset = 0.0f32;
    let mut last_generation = control.generation();
    let trace_audio = crate::profile::media_enabled()
        || std::env::var_os("WEBCORE_TRACE_AUDIO_DELIVERY").is_some();
    'restart: while !control.cancelled() {
        let startup = Instant::now();
        let generation = control.generation();
        if generation != last_generation && let Some(duration) = duration {
            offset = if control.looping() { (control.time() / duration).floor() * duration } else { 0.0 };
        }
        last_generation = generation;
        let stop = AtomicBool::new(false);
        let pending_clock = PendingAudioClock::new(&control, generation);
        let Some(mut reader) = open() else { eprintln!("WebM media download failed"); return; };
        let mut stream = WebmStream::new();
        let initial = loop {
            if control.cancelled() { return; }
            if control.generation() != generation { continue 'restart; }
            let count = match reader.read(&mut bytes) {
                Ok(count) => count,
                Err(error) => { eprintln!("WebM media read failed: {error}"); return; }
            };
            let packets = match stream.push_packets(&bytes[..count]) {
                Ok(packets) => packets,
                Err(error) => { eprintln!("WebM demux failed: {error:?}"); return; }
            };
            if stream.audio_track().is_some() { pending_clock.pin(); }
            if let Some(metadata) = stream.metadata() {
                duration = metadata.duration.filter(|value| value.is_finite() && *value > 0.0);
                if !metadata_sent {
                    if !emit(VideoUpdate::Metadata(metadata), &stop) { return; }
                    if trace_audio {
                        eprintln!("media metadata startup_ms={:.1}",
                            startup.elapsed().as_secs_f64() * 1000.0);
                    }
                    metadata_sent = true;
                }
            }
            if !packets.is_empty() { break packets; }
            if count == 0 {
                if let Err(error) = stream.finish() { eprintln!("WebM media finish failed: {error:?}"); }
                return;
            }
        };
        let video_failed = AtomicBool::new(false);
        let total = Arc::new(AtomicUsize::new(0));
        let (audio_tx, audio_rx) = mpsc::sync_channel(128);
        let (video_tx, video_rx) = mpsc::sync_channel(256);
        let audio_decoder = WebmMediaDecoder::from_tracks(&stream);
        let video_decoder = WebmMediaDecoder::from_tracks(&stream);
        let has_audio = stream.audio_track().is_some();
        let mut audio_preroll = AudioPreroll::new(
            f64::from((f32::from_bits(control.time.load(Ordering::Acquire)) - offset).max(0.0)),
            stream.audio_track());
        let (audio_end, video_end) = std::thread::scope(|scope| {
            let audio = scope.spawn(|| audio_worker(&control, &pcm, generation, f64::from(offset),
                &stop, audio_rx, audio_decoder, &startup, trace_audio, &pending_clock));
            let video = scope.spawn(|| video_worker(&control, generation, offset, &stop,
                &video_failed, video_rx, video_decoder, &mut emit));
            let mut packets: VecDeque<_> = initial.into();
            let mut epoch = 0u64;
            let mut need_key = true;
            let mut video_end = None;
            let seek_time = f32::from_bits(control.time.load(Ordering::Acquire)) - offset;
            let mut seek_prefix = (seek_time > 0.0).then(Vec::<VideoPacket>::new);
            let mut seek_prefix_bytes = 0usize;
            'demux: loop {
                while let Some(packet) = packets.pop_front() {
                    if !active(&control, generation, &stop) { break 'demux; }
                    match packet {
                        WebmPacket::Audio(block) => {
                            if !control.audio_disabled.load(Ordering::Acquire) {
                                let (previous, current) = audio_preroll.push(block);
                                for block in previous.into_iter().chain(current) {
                                    if !send(&audio_tx, AudioInput::Packet(WebmPacket::Audio(block)),
                                        &control, generation, &stop)
                                    { break 'demux; }
                                }
                            }
                        }
                        WebmPacket::Video(packet) => {
                            video_end = Some(video_end.unwrap_or(0.0f64)
                                .max(f64::from(packet.timestamp) + 1.0 / 30.0));
                            if let Some(prefix) = &mut seek_prefix {
                                if packet.timestamp < seek_time {
                                    if packet.key_frame {
                                        prefix.clear();
                                        seek_prefix_bytes = 0;
                                    }
                                    if (packet.key_frame || !prefix.is_empty())
                                        && packet.data.len() <= VIDEO_BYTES - seek_prefix_bytes
                                        && prefix.len() < 256
                                    {
                                        seek_prefix_bytes += packet.data.len();
                                        prefix.push(packet);
                                    } else {
                                        prefix.clear();
                                        seek_prefix_bytes = 0;
                                    }
                                    continue;
                                }
                                let prefix = seek_prefix.take().unwrap();
                                packets.push_front(WebmPacket::Video(packet));
                                for packet in prefix.into_iter().rev() {
                                    packets.push_front(WebmPacket::Video(packet));
                                }
                                continue;
                            }
                            if !has_audio || control.audio_disabled.load(Ordering::Acquire) {
                                while packet.timestamp + offset > control.time() + 1.0
                                    && active(&control, generation, &stop)
                                { std::thread::sleep(POLL); }
                            }
                            if !active(&control, generation, &stop) { break 'demux; }
                            if video_failed.load(Ordering::Acquire) { continue; }
                            if need_key && !packet.key_frame { continue; }
                            let size = packet.data.len();
                            let Some(reservation) = Reservation::acquire(&total, size) else {
                                epoch = epoch.wrapping_add(1); need_key = true; continue;
                            };
                            let reset = packet.key_frame.then(|| WebmMediaDecoder::from_tracks(&stream));
                            let encoded = EncodedVideo { packet, epoch, reset,
                                _reservation: reservation };
                            match video_tx.try_send(VideoInput::Packet(encoded)) {
                                Ok(()) => need_key = false,
                                Err(mpsc::TrySendError::Full(_)) => {
                                    epoch = epoch.wrapping_add(1);
                                    need_key = true;
                                }
                                Err(mpsc::TrySendError::Disconnected(_)) => {
                                    video_failed.store(true, Ordering::Release);
                                }
                            }
                        }
                    }
                }
                if !active(&control, generation, &stop) { break; }
                let count = match reader.read(&mut bytes) {
                    Ok(count) => count,
                    Err(error) => {
                        eprintln!("WebM media read failed: {error}");
                        stop.store(true, Ordering::Release);
                        break;
                    }
                };
                if count == 0 {
                    if let Err(error) = stream.finish() {
                        eprintln!("WebM media finish failed: {error:?}");
                        stop.store(true, Ordering::Release);
                        break;
                    }
                    // Finish video without waiting on its full encoded queue.
                    let _ = video_tx.try_send(VideoInput::Finish);
                    if let Some(block) = audio_preroll.previous.take() {
                        send(&audio_tx, AudioInput::Packet(WebmPacket::Audio(block)), &control, generation, &stop);
                    }
                    send(&audio_tx, AudioInput::Finish { duration, video_end }, &control, generation, &stop);
                    break;
                }
                packets = match stream.push_packets(&bytes[..count]) {
                    Ok(packets) => packets.into(),
                    Err(error) => {
                        eprintln!("WebM demux failed: {error:?}");
                        stop.store(true, Ordering::Release);
                        break;
                    }
                };
            }
            drop(video_tx);
            drop(audio_tx);
            if stop.load(Ordering::Acquire) { pending_clock.release(); }
            let end = audio.join().expect("WebM audio worker panicked");
            video.join().expect("WebM video worker panicked");
            (end, video_end)
        });
        if control.cancelled() { return; }
        if control.generation() != generation { continue; }
        if stop.load(Ordering::Acquire) { return; }
        let inferred = audio_end.max(video_end.unwrap_or(0.0));
        let pass_duration = duration.or_else(||
            (inferred.is_finite() && inferred > 0.0).then_some(inferred as f32));
        if !control.looping() {
            while !control.cancelled() && control.generation() == generation { std::thread::sleep(POLL); }
            continue;
        }
        let Some(pass_duration) = pass_duration else { return; };
        offset += pass_duration;
        while control.time() + AUDIO_LEAD < offset {
            if control.cancelled() { return; }
            if control.generation() != generation { continue 'restart; }
            std::thread::sleep(POLL);
        }
    }
}
