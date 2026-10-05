//! Shared MP4 reader, independent AAC/video workers, common device clock.

use super::parallel::{Reservation, active, disable_audio, pad_audio, send, send_pcm};
use super::{AUDIO_LEAD, Control, POLL, PcmSender, PendingAudioClock, VIDEO_LEAD, VideoUpdate};
use std::collections::VecDeque;
use std::io::Read;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicUsize, Ordering},
    mpsc,
};
use std::time::Instant;
use webmedia::audio::{aac::AacConfig, mp4::Mp4AacDecoder};
use webmedia::video::{
    MediaMetadata,
    mp4::Mp4VideoIndex,
    mp4_demux::{Mp4Demux, Packet, Track},
    mp4_video::Mp4VideoPackets,
};

struct EncodedVideo {
    packet: Packet,
    epoch: u64,
    _reservation: Reservation,
}

fn audio_worker(
    control: &Control,
    pcm: &PcmSender,
    generation: u64,
    offset: f64,
    stop: &AtomicBool,
    rx: mpsc::Receiver<Packet>,
    mut decoder: Mp4AacDecoder,
    startup: &Instant,
    trace: bool,
    duration: Option<f32>,
    pending_clock: &PendingAudioClock<'_>,
) {
    let mut first = true;
    let mut end = 0.0;
    let mut format = None;
    while active(control, generation, stop) {
        let packet = match rx.recv_timeout(POLL) {
            Ok(packet) => packet,
            Err(mpsc::RecvTimeoutError::Timeout) => continue,
            Err(mpsc::RecvTimeoutError::Disconnected) => break,
        };
        let decoded = match decoder.decode(packet.presentation_time, &packet.data) {
            Ok(Some(decoded)) => decoded,
            Ok(None) => continue,
            Err(error) => {
                eprintln!("MP4 AAC decode failed: {error:?}");
                disable_audio(control, generation);
                pending_clock.release();
                return;
            }
        };
        format = Some((decoded.samples.sample_rate, decoded.samples.channels));
        end = decoded.timestamp
            + decoded.samples.samples.len() as f64
                / f64::from(decoded.samples.channels)
                / f64::from(decoded.samples.sample_rate);
        if first && trace {
            eprintln!(
                "MP4 audio first PCM generation={generation} startup_ms={:.1} pts={:.3}",
                startup.elapsed().as_secs_f64() * 1000.0,
                decoded.timestamp + offset
            );
        }
        first = false;
        let eligible_end = end + offset;
        if !send_pcm(
            pcm,
            generation,
            decoded.timestamp + offset,
            decoded.samples,
            stop,
        ) {
            pending_clock.release();
            return;
        }
        pending_clock.handoff(eligible_end);
    }
    pending_clock.release();
    // Only a successfully decoded track's declared trailing gap is silence.
    if let (Some((rate, channels)), Some(duration)) = (format, duration) {
        pad_audio(
            control,
            pcm,
            generation,
            offset,
            stop,
            &mut end,
            f64::from(duration),
            rate,
            channels,
        );
    }
}

fn video_worker(
    control: &Control,
    generation: u64,
    offset: f32,
    stop: &AtomicBool,
    rx: mpsc::Receiver<EncodedVideo>,
    index: &Mp4VideoIndex,
    emit: &mut (impl FnMut(VideoUpdate, &AtomicBool) -> bool + Send),
) {
    let mut decoder = None;
    let mut epoch = None;
    let mut pending = VecDeque::new();
    let mut finished = false;
    let mut first = true;
    let seek_time = f32::from_bits(control.time.load(Ordering::Acquire));
    let trace = crate::profile::media_enabled();
    let mut interval = Instant::now();
    let mut decode_time = std::time::Duration::ZERO;
    let mut frames = 0;
    while active(control, generation, stop) {
        if pending.is_empty() && !finished {
            match rx.recv_timeout(POLL) {
                Ok(packet) => pending.push_back(packet),
                Err(mpsc::RecvTimeoutError::Timeout) => continue,
                Err(mpsc::RecvTimeoutError::Disconnected) => finished = true,
            }
        }
        for _ in pending.len()..256 {
            match rx.try_recv() {
                Ok(packet) => pending.push_back(packet),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    finished = true;
                    break;
                }
            }
        }
        if let Some(clock) = control.audio_time() {
            let key = pending.iter().rposition(|encoded| {
                encoded.packet.keyframe
                    && encoded.packet.presentation_time as f32 / encoded.packet.timescale as f32
                        + offset
                        <= clock
            });
            if let Some(key) = key.filter(|key| *key > 0) {
                pending.drain(..key);
                epoch = None;
            }
        }
        let Some(encoded) = pending.pop_front() else {
            if finished {
                return;
            }
            continue;
        };
        while control.paused() && !first && active(control, generation, stop) {
            std::thread::sleep(POLL);
        }
        if !active(control, generation, stop) {
            return;
        }
        if epoch != Some(encoded.epoch) {
            if !encoded.packet.keyframe {
                continue;
            }
            decoder = match Mp4VideoPackets::new(index.clone(), encoded.packet.sample_number) {
                Ok(decoder) => Some(decoder),
                Err(error) => {
                    eprintln!("MP4 video setup failed: {error:?}");
                    return;
                }
            };
            epoch = Some(encoded.epoch);
        }
        let start = Instant::now();
        let decoded = match decoder
            .as_mut()
            .unwrap()
            .push(encoded.packet.sample_number, &encoded.packet.data)
        {
            Ok(frames) => frames,
            Err(error) => {
                eprintln!("MP4 video decode failed: {error:?}");
                return;
            }
        };
        decode_time += start.elapsed();
        for mut frame in decoded {
            frame.timestamp += offset;
            if frame.timestamp < seek_time {
                continue;
            }
            while ((!first && control.paused()) || frame.timestamp > control.time() + VIDEO_LEAD)
                && active(control, generation, stop)
            {
                std::thread::sleep(POLL);
            }
            if !active(control, generation, stop) {
                return;
            }
            if !emit(VideoUpdate::Frame { generation, frame }, stop) {
                stop.store(true, Ordering::Release);
                return;
            }
            first = false;
            frames += 1;
        }
        if trace && interval.elapsed().as_secs_f64() >= 1.0 {
            eprintln!(
                "MP4 video generation={generation} frames={frames} decode_ms={:.1} queue={} clock={:.3}",
                decode_time.as_secs_f64() * 1000.0,
                pending.len(),
                control.time()
            );
            frames = 0;
            decode_time = std::time::Duration::ZERO;
            interval = Instant::now();
        }
    }
}

pub(super) fn decode_mp4(
    control: Arc<Control>,
    pcm: PcmSender,
    mut open: impl FnMut() -> Option<Box<dyn Read + Send>>,
    mut emit: impl FnMut(VideoUpdate, &AtomicBool) -> bool + Send,
) {
    let mut duration: Option<f32> = None;
    let mut offset = 0.0f32;
    let mut metadata_sent = false;
    let mut last_generation = control.generation();
    let trace = crate::profile::media_enabled();
    let mut bytes = [0; 16 * 1024];
    'restart: while !control.cancelled() {
        let startup = Instant::now();
        let generation = control.generation();
        if generation != last_generation
            && let Some(duration) = duration
        {
            offset = if control.looping() {
                (control.time() / duration).floor() * duration
            } else {
                0.0
            };
        }
        last_generation = generation;
        let stop = AtomicBool::new(false);
        let pending_clock = PendingAudioClock::new(&control, generation);
        let Some(mut reader) = open() else {
            eprintln!("MP4 media download failed");
            return;
        };
        let seek_time = (f32::from_bits(control.time.load(Ordering::Acquire)) - offset).max(0.0);
        let mut demux = Mp4Demux::with_start_time(f64::from(seek_time)).unwrap();
        let initial = loop {
            if control.cancelled() {
                return;
            }
            if control.generation() != generation {
                continue 'restart;
            }
            let count = match reader.read(&mut bytes) {
                Ok(count) => count,
                Err(error) => {
                    eprintln!("MP4 read failed: {error}");
                    return;
                }
            };
            let packets = match demux.push(&bytes[..count]) {
                Ok(packets) => packets,
                Err(error) => {
                    eprintln!("MP4 demux failed: {error:?}");
                    return;
                }
            };
            if demux.audio_index().is_some() || demux.video_index().is_some() {
                break packets;
            }
            if count == 0 {
                eprintln!("MP4 metadata truncated");
                return;
            }
        };
        let video_index = demux.video_index().cloned();
        let audio_decoder = demux
            .audio_index()
            .and_then(|index| match Mp4AacDecoder::new(index) {
                Ok(decoder) => Some(decoder),
                Err(error) => {
                    eprintln!("MP4 AAC setup failed: {error:?}");
                    None
                }
            });
        if audio_decoder.is_some() {
            pending_clock.pin();
        } else if demux.audio_index().is_some() {
            disable_audio(&control, generation);
        }
        let video_duration = video_index
            .as_ref()
            .map(|index| index.duration_ticks as f32 / index.timescale as f32);
        let audio_duration = demux
            .audio_index()
            .map(|index| index.duration_ticks as f32 / index.timescale as f32);
        duration = video_duration
            .into_iter()
            .chain(audio_duration)
            .reduce(f32::max);
        if !metadata_sent {
            let config = demux
                .audio_index()
                .and_then(|index| AacConfig::parse(&index.audio_specific_config).ok());
            let metadata = MediaMetadata {
                presentation_size: None,
                duration,
                width: video_index.as_ref().map(|index| index.width),
                height: video_index.as_ref().map(|index| index.height),
                sample_rate: config.as_ref().map(|config| config.sample_rate),
                channels: config.as_ref().map(|config| config.channels),
            };
            if !emit(VideoUpdate::Metadata(metadata), &stop) {
                return;
            }
            metadata_sent = true;
        }
        let total = Arc::new(AtomicUsize::new(0));
        let (audio_tx, audio_rx) = mpsc::sync_channel(128);
        let (video_tx, video_rx) = mpsc::sync_channel(256);
        std::thread::scope(|scope| {
            let audio = scope.spawn(|| {
                if let Some(decoder) = audio_decoder {
                    audio_worker(
                        &control,
                        &pcm,
                        generation,
                        f64::from(offset),
                        &stop,
                        audio_rx,
                        decoder,
                        &startup,
                        trace,
                        duration,
                        &pending_clock,
                    );
                }
            });
            let video = scope.spawn(|| {
                if let Some(index) = &video_index {
                    video_worker(
                        &control, generation, offset, &stop, video_rx, index, &mut emit,
                    );
                }
            });
            let mut packets = initial;
            let mut epoch = 0u64;
            let mut need_key = true;
            'demux: loop {
                for packet in packets {
                    if !active(&control, generation, &stop) {
                        break 'demux;
                    }
                    match packet.track {
                        Track::Audio => {
                            if !control.audio_disabled.load(Ordering::Acquire) {
                                // A disconnected codec must not halt the other track.
                                send(&audio_tx, packet, &control, generation, &stop);
                            }
                        }
                        Track::Video => {
                            if need_key && !packet.keyframe {
                                continue;
                            }
                            if control.audio_disabled.load(Ordering::Acquire) {
                                while packet.presentation_time as f32 / packet.timescale as f32
                                    + offset
                                    > control.time() + 1.0
                                    && active(&control, generation, &stop)
                                {
                                    std::thread::sleep(POLL);
                                }
                            }
                            if !active(&control, generation, &stop) {
                                break 'demux;
                            }
                            let Some(reservation) = Reservation::acquire(&total, packet.data.len())
                            else {
                                epoch = epoch.wrapping_add(1);
                                need_key = true;
                                continue;
                            };
                            let encoded = EncodedVideo {
                                packet,
                                epoch,
                                _reservation: reservation,
                            };
                            match video_tx.try_send(encoded) {
                                Ok(()) => need_key = false,
                                Err(_) => {
                                    epoch = epoch.wrapping_add(1);
                                    need_key = true;
                                }
                            }
                        }
                    }
                }
                if !active(&control, generation, &stop) {
                    break;
                }
                packets = match demux.push(&[]) {
                    Ok(packets) => packets,
                    Err(error) => {
                        eprintln!("MP4 demux failed: {error:?}");
                        stop.store(true, Ordering::Release);
                        break;
                    }
                };
                if !packets.is_empty() {
                    continue;
                }
                let count = match reader.read(&mut bytes) {
                    Ok(count) => count,
                    Err(error) => {
                        eprintln!("MP4 read failed: {error}");
                        stop.store(true, Ordering::Release);
                        break;
                    }
                };
                if count == 0 {
                    if let Err(error) = demux.finish() {
                        eprintln!("MP4 media truncated: {error:?}");
                        stop.store(true, Ordering::Release);
                    }
                    break;
                }
                packets = match demux.push(&bytes[..count]) {
                    Ok(packets) => packets,
                    Err(error) => {
                        eprintln!("MP4 demux failed: {error:?}");
                        stop.store(true, Ordering::Release);
                        break;
                    }
                };
            }
            drop(audio_tx);
            drop(video_tx);
            if stop.load(Ordering::Acquire) { pending_clock.release(); }
            audio.join().expect("MP4 audio worker panicked");
            video.join().expect("MP4 video worker panicked");
        });
        if control.cancelled() || stop.load(Ordering::Acquire) {
            return;
        }
        if control.generation() != generation {
            continue;
        }
        if !control.looping() {
            while !control.cancelled() && control.generation() == generation {
                std::thread::sleep(POLL);
            }
            continue;
        }
        let Some(duration) = duration.filter(|duration| *duration > 0.0) else {
            return;
        };
        offset += duration;
        while control.time() + AUDIO_LEAD < offset {
            if control.cancelled() {
                return;
            }
            if control.generation() != generation {
                continue 'restart;
            }
            std::thread::sleep(POLL);
        }
    }
}
