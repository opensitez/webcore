//! Bounded output of already-decoded, interleaved PCM. No codec is delegated
//! to the operating system. Hosts submit from a playback thread, not the UI.

use std::time::Duration;

const BUFFER_FRAMES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PcmOutputError {
    Unsupported,
    InvalidData,
    WouldBlock,
    Device(i32),
}

/// Four reusable device buffers. Buffer completion drives backpressure without
/// requiring a renderer tick. The handle is confined to its creating thread.
pub struct PcmOutput {
    channels: usize,
    #[cfg(target_os = "macos")]
    device: macos::Device,
}

fn validate_format(sample_rate: u32, channels: u16) -> Result<(), PcmOutputError> {
    if !(4000..=384000).contains(&sample_rate) || !(1..=8).contains(&channels) {
        return Err(PcmOutputError::InvalidData);
    }
    Ok(())
}

fn accepted_samples(samples: &[f32], channels: usize) -> Result<usize, PcmOutputError> {
    if channels == 0 || samples.len() % channels != 0 {
        return Err(PcmOutputError::InvalidData);
    }
    let count = samples.len().min(BUFFER_FRAMES * channels);
    if samples[..count].iter().any(|sample| !sample.is_finite()) {
        return Err(PcmOutputError::InvalidData);
    }
    Ok(count)
}

impl PcmOutput {
    pub fn new(sample_rate: u32, channels: u16) -> Result<Self, PcmOutputError> {
        validate_format(sample_rate, channels)?;
        #[cfg(target_os = "macos")]
        {
            Ok(Self {
                channels: usize::from(channels),
                device: macos::Device::new(sample_rate, channels)?,
            })
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(PcmOutputError::Unsupported)
        }
    }

    /// Returns the number of interleaved samples accepted, at whole-frame
    /// boundaries. WouldBlock never consumes input. Retry after buffer completion.
    pub fn submit(&mut self, samples: &[f32]) -> Result<usize, PcmOutputError> {
        let count = accepted_samples(samples, self.channels)?;
        if count == 0 {
            return Ok(0);
        }
        #[cfg(target_os = "macos")]
        {
            self.device.submit(&samples[..count])?;
            Ok(count)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(PcmOutputError::Unsupported)
        }
    }

    pub fn wait_for_buffer(&self, timeout: Duration) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.device.wait_for_buffer(timeout)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = timeout;
            false
        }
    }

    pub fn set_volume(&mut self, volume: f32) -> Result<(), PcmOutputError> {
        if !volume.is_finite() || !(0.0..=1.0).contains(&volume) {
            return Err(PcmOutputError::InvalidData);
        }
        #[cfg(target_os = "macos")]
        {
            self.device.set_volume(volume)
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(PcmOutputError::Unsupported)
        }
    }

    pub fn set_paused(&mut self, paused: bool) -> Result<(), PcmOutputError> {
        #[cfg(target_os = "macos")]
        {
            self.device.set_paused(paused)
        }
        #[cfg(not(target_os = "macos"))]
        {
            let _ = paused;
            Err(PcmOutputError::Unsupported)
        }
    }

    /// Discard queued PCM and restart completion accounting, preserving pause state.
    pub fn reset(&mut self) -> Result<(), PcmOutputError> {
        #[cfg(target_os = "macos")]
        {
            self.device.reset()
        }
        #[cfg(not(target_os = "macos"))]
        {
            Err(PcmOutputError::Unsupported)
        }
    }

    pub fn drained(&self) -> bool {
        #[cfg(target_os = "macos")]
        {
            self.device.drained()
        }
        #[cfg(not(target_os = "macos"))]
        {
            false
        }
    }

    /// Sample frames returned by the device's buffer-completion callbacks.
    pub fn completed_frames(&self) -> u64 {
        #[cfg(target_os = "macos")]
        {
            self.device.completed_frames()
        }
        #[cfg(not(target_os = "macos"))]
        {
            0
        }
    }
}

#[cfg(target_os = "macos")]
mod macos {
    use super::{BUFFER_FRAMES, PcmOutputError};
    use std::{
        ffi::c_void,
        mem::ManuallyDrop,
        ptr,
        sync::{
            Condvar, Mutex,
            atomic::{AtomicU64, Ordering},
        },
        time::Duration,
    };

    const BUFFER_COUNT: usize = 4;
    const ALL_FREE: u8 = (1 << BUFFER_COUNT) - 1;
    type Queue = *mut c_void;

    #[repr(C)]
    struct StreamDescription {
        sample_rate: f64,
        format: u32,
        flags: u32,
        bytes_per_packet: u32,
        frames_per_packet: u32,
        bytes_per_frame: u32,
        channels: u32,
        bits_per_channel: u32,
        reserved: u32,
    }

    #[repr(C)]
    struct Buffer {
        capacity: u32,
        data: *mut c_void,
        byte_size: u32,
        user_data: *mut c_void,
        packet_capacity: u32,
        packets: *mut c_void,
        packet_count: u32,
    }

    #[link(name = "AudioToolbox", kind = "framework")]
    unsafe extern "C" {
        fn AudioQueueNewOutput(
            format: *const StreamDescription,
            callback: unsafe extern "C" fn(*mut c_void, Queue, *mut Buffer),
            user: *mut c_void,
            run_loop: *const c_void,
            mode: *const c_void,
            flags: u32,
            out: *mut Queue,
        ) -> i32;
        fn AudioQueueAllocateBuffer(queue: Queue, bytes: u32, out: *mut *mut Buffer) -> i32;
        fn AudioQueueEnqueueBuffer(
            queue: Queue,
            buffer: *mut Buffer,
            count: u32,
            packets: *const c_void,
        ) -> i32;
        fn AudioQueueStart(queue: Queue, start: *const c_void) -> i32;
        #[cfg(test)]
        fn AudioQueuePrime(queue: Queue, frames: u32, prepared: *mut u32) -> i32;
        fn AudioQueuePause(queue: Queue) -> i32;
        fn AudioQueueStop(queue: Queue, immediate: u8) -> i32;
        fn AudioQueueSetParameter(queue: Queue, parameter: u32, value: f32) -> i32;
        fn AudioQueueDispose(queue: Queue, immediate: u8) -> i32;
    }

    struct CallbackState {
        free: Mutex<u8>,
        ready: Condvar,
        completed: AtomicU64,
        bytes_per_frame: u32,
    }

    unsafe extern "C" fn completed(user: *mut c_void, _queue: Queue, buffer: *mut Buffer) {
        if user.is_null() || buffer.is_null() {
            return;
        }
        // The boxed state and AudioQueue buffers live until synchronous disposal
        // has returned. No callback calls into disposal or host rendering.
        let state = unsafe { &*user.cast::<CallbackState>() };
        let index = unsafe { (*buffer).user_data as usize };
        if index >= BUFFER_COUNT {
            return;
        }
        let mut free = state.free.lock().unwrap_or_else(|error| error.into_inner());
        let bit = 1 << index;
        if *free & bit == 0 {
            state.completed.fetch_add(
                u64::from(unsafe { (*buffer).byte_size } / state.bytes_per_frame),
                Ordering::Release,
            );
            *free |= bit;
        }
        state.ready.notify_all();
    }

    fn status(code: i32) -> Result<(), PcmOutputError> {
        if code == 0 {
            Ok(())
        } else {
            Err(PcmOutputError::Device(code))
        }
    }

    pub(super) struct Device {
        queue: Queue,
        buffers: [*mut Buffer; BUFFER_COUNT],
        state: ManuallyDrop<Box<CallbackState>>,
        running: bool,
        paused: bool,
        volume: f32,
    }

    impl Device {
        pub(super) fn new(sample_rate: u32, channels: u16) -> Result<Self, PcmOutputError> {
            let bytes_per_frame = u32::from(channels) * 4;
            let format = StreamDescription {
                sample_rate: f64::from(sample_rate),
                format: u32::from_be_bytes(*b"lpcm"),
                flags: 1 | 8,
                bytes_per_packet: bytes_per_frame,
                frames_per_packet: 1,
                bytes_per_frame,
                channels: u32::from(channels),
                bits_per_channel: 32,
                reserved: 0,
            };
            let mut device = Self {
                queue: ptr::null_mut(),
                buffers: [ptr::null_mut(); BUFFER_COUNT],
                state: ManuallyDrop::new(Box::new(CallbackState {
                    free: Mutex::new(ALL_FREE),
                    ready: Condvar::new(),
                    completed: AtomicU64::new(0),
                    bytes_per_frame,
                })),
                running: false,
                paused: false,
                volume: 1.0,
            };
            status(unsafe {
                AudioQueueNewOutput(
                    &format,
                    completed,
                    (&mut **device.state as *mut CallbackState).cast(),
                    ptr::null(),
                    ptr::null(),
                    0,
                    &mut device.queue,
                )
            })?;
            if device.queue.is_null() {
                return Err(PcmOutputError::Device(-1));
            }
            for (index, slot) in device.buffers.iter_mut().enumerate() {
                status(unsafe {
                    AudioQueueAllocateBuffer(
                        device.queue,
                        BUFFER_FRAMES as u32 * bytes_per_frame,
                        slot,
                    )
                })?;
                if slot.is_null() {
                    return Err(PcmOutputError::Device(-1));
                }
                unsafe {
                    (**slot).user_data = index as *mut c_void;
                }
            }
            Ok(device)
        }

        pub(super) fn submit(&mut self, samples: &[f32]) -> Result<(), PcmOutputError> {
            let index = {
                let mut free = self
                    .state
                    .free
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                if *free == 0 {
                    return Err(PcmOutputError::WouldBlock);
                }
                let index = free.trailing_zeros() as usize;
                *free &= !(1 << index);
                index
            };
            let buffer = self.buffers[index];
            let bytes = std::mem::size_of_val(samples);
            if bytes > unsafe { (*buffer).capacity } as usize {
                *self
                    .state
                    .free
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) |= 1 << index;
                return Err(PcmOutputError::InvalidData);
            }
            unsafe {
                ptr::copy_nonoverlapping(
                    samples.as_ptr().cast::<u8>(),
                    (*buffer).data.cast::<u8>(),
                    bytes,
                );
                (*buffer).byte_size = bytes as u32;
            }
            let startup_trace =
                (!self.running && crate::profile::media_enabled()).then(std::time::Instant::now);
            if let Err(error) =
                status(unsafe { AudioQueueEnqueueBuffer(self.queue, buffer, 0, ptr::null()) })
            {
                *self
                    .state
                    .free
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) |= 1 << index;
                return Err(error);
            }
            if let Some(started) = startup_trace {
                eprintln!(
                    "audio native first enqueue_ms={:.1}",
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
            if !self.running && !self.paused {
                self.start()?;
            }
            Ok(())
        }

        fn start(&mut self) -> Result<(), PcmOutputError> {
            let started = crate::profile::media_enabled().then(std::time::Instant::now);
            status(unsafe { AudioQueueStart(self.queue, ptr::null()) })?;
            if let Some(started) = started {
                eprintln!(
                    "audio native start_ms={:.1}",
                    started.elapsed().as_secs_f64() * 1000.0
                );
            }
            self.running = true;
            Ok(())
        }

        pub(super) fn wait_for_buffer(&self, timeout: Duration) -> bool {
            let free = self
                .state
                .free
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            let (free, _) = self
                .state
                .ready
                .wait_timeout_while(free, timeout, |mask| *mask == 0)
                .unwrap_or_else(|error| error.into_inner());
            *free != 0
        }

        pub(super) fn drained(&self) -> bool {
            *self
                .state
                .free
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                == ALL_FREE
        }

        pub(super) fn completed_frames(&self) -> u64 {
            self.state.completed.load(Ordering::Acquire)
        }

        pub(super) fn set_volume(&mut self, volume: f32) -> Result<(), PcmOutputError> {
            if volume == self.volume {
                return Ok(());
            }
            status(unsafe { AudioQueueSetParameter(self.queue, 1, volume) })?;
            self.volume = volume;
            Ok(())
        }

        pub(super) fn reset(&mut self) -> Result<(), PcmOutputError> {
            // Called only by the owner thread, never a buffer callback. Immediate
            // stop synchronously returns all queued buffers before they are reused.
            status(unsafe { AudioQueueStop(self.queue, 1) })?;
            self.running = false;
            *self
                .state
                .free
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = ALL_FREE;
            self.state.completed.store(0, Ordering::Release);
            self.state.ready.notify_all();
            Ok(())
        }

        pub(super) fn set_paused(&mut self, paused: bool) -> Result<(), PcmOutputError> {
            if paused && self.running {
                status(unsafe { AudioQueuePause(self.queue) })?;
                self.running = false;
            } else if !paused && !self.running && !self.drained() {
                self.start()?;
            }
            self.paused = paused;
            Ok(())
        }
    }

    impl Drop for Device {
        fn drop(&mut self) {
            // On a device disposal error retain callback storage rather than
            // risking a callback dereferencing freed memory.
            if self.queue.is_null() || unsafe { AudioQueueDispose(self.queue, 1) } == 0 {
                unsafe {
                    ManuallyDrop::drop(&mut self.state);
                }
            }
        }
    }

    #[cfg(test)]
    mod startup_probe {
        use super::*;
        use std::time::Instant;

        struct ProbeState {
            first_completion: Mutex<Option<Instant>>,
            ready: Condvar,
        }

        // This callback has no media PCM, completed-frame counter or playback
        // clock. Buffer availability is not a measurement of audible output.
        unsafe extern "C" fn returned(user: *mut c_void, _queue: Queue, _buffer: *mut Buffer) {
            let state = unsafe { &*user.cast::<ProbeState>() };
            let mut first = state.first_completion.lock().unwrap();
            first.get_or_insert_with(Instant::now);
            state.ready.notify_all();
        }

        struct Probe {
            queue: Queue,
            buffer: *mut Buffer,
            state: ManuallyDrop<Box<ProbeState>>,
            sample_rate: u32,
            frames: u32,
        }

        impl Probe {
            fn new(sample_rate: u32) -> Self {
                assert!(matches!(sample_rate, 44100 | 48000));
                let frames = sample_rate / 50;
                let format = StreamDescription {
                    sample_rate: f64::from(sample_rate),
                    format: u32::from_be_bytes(*b"lpcm"),
                    flags: 1 | 8,
                    bytes_per_packet: 8,
                    frames_per_packet: 1,
                    bytes_per_frame: 8,
                    channels: 2,
                    bits_per_channel: 32,
                    reserved: 0,
                };
                let mut probe = Self {
                    queue: ptr::null_mut(),
                    buffer: ptr::null_mut(),
                    state: ManuallyDrop::new(Box::new(ProbeState {
                        first_completion: Mutex::new(None),
                        ready: Condvar::new(),
                    })),
                    sample_rate,
                    frames,
                };
                let started = Instant::now();
                status(unsafe {
                    AudioQueueNewOutput(
                        &format,
                        returned,
                        (&mut **probe.state as *mut ProbeState).cast(),
                        ptr::null(),
                        ptr::null(),
                        0,
                        &mut probe.queue,
                    )
                })
                .unwrap();
                let create = started.elapsed();
                assert!(!probe.queue.is_null());
                let started = Instant::now();
                status(unsafe {
                    AudioQueueAllocateBuffer(probe.queue, frames * 8, &mut probe.buffer)
                })
                .unwrap();
                let allocate = started.elapsed();
                assert!(!probe.buffer.is_null());
                status(unsafe { AudioQueueSetParameter(probe.queue, 1, 0.0) }).unwrap();
                unsafe {
                    ptr::write_bytes((*probe.buffer).data.cast::<u8>(), 0, (frames * 8) as usize);
                    (*probe.buffer).byte_size = frames * 8;
                }
                eprintln!(
                    "native startup probe rate={sample_rate} create_us={} allocate_us={}",
                    create.as_micros(),
                    allocate.as_micros()
                );
                probe
            }

            fn trial(&mut self, name: &str, prime: bool) {
                let frames = self.frames;
                let rate = self.sample_rate;
                *self.state.first_completion.lock().unwrap() = None;
                let began = Instant::now();
                status(unsafe { AudioQueueEnqueueBuffer(self.queue, self.buffer, 0, ptr::null()) })
                    .unwrap();
                let enqueue = began.elapsed();
                let mut prepared = 0;
                let prime_time = if prime {
                    let began = Instant::now();
                    status(unsafe { AudioQueuePrime(self.queue, 0, &mut prepared) }).unwrap();
                    Some(began.elapsed())
                } else {
                    None
                };
                let start = Instant::now();
                status(unsafe { AudioQueueStart(self.queue, ptr::null()) }).unwrap();
                let start_time = start.elapsed();
                let first = self.state.first_completion.lock().unwrap();
                let (first, timeout) = self
                    .state
                    .ready
                    .wait_timeout_while(first, Duration::from_secs(10), |first| first.is_none())
                    .unwrap();
                assert!(
                    !timeout.timed_out() || first.is_some(),
                    "native completion timed out"
                );
                let completed_at = first.unwrap();
                drop(first);
                let unix_ms = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap()
                    .as_millis();
                eprintln!(
                    "native startup probe unix_ms={unix_ms} case={name} rate={rate} frames={frames} enqueue_us={} prime_us={:?} prepared={prepared} start_us={} completion_from_start_us={:?} completion_before_start={}",
                    enqueue.as_micros(),
                    prime_time.map(|time| time.as_micros()),
                    start_time.as_micros(),
                    completed_at
                        .checked_duration_since(start)
                        .map(|time| time.as_micros()),
                    completed_at < start
                );
                status(unsafe { AudioQueuePause(self.queue) }).unwrap();
            }
        }

        fn idle_seconds(value: Option<&str>) -> Vec<u64> {
            let Some(value) = value else {
                return Vec::new();
            };
            assert!(
                !value.is_empty(),
                "idle seconds must be an explicit subset of 5,30,60"
            );
            let seconds: Vec<u64> = value
                .split(',')
                .map(|value| {
                    let value = value.parse().expect("invalid idle seconds");
                    assert!(matches!(value, 5 | 30 | 60), "idle seconds must be 5,30,60");
                    value
                })
                .collect();
            assert!(seconds.len() <= 3, "at most three idle cases");
            for (index, value) in seconds.iter().enumerate() {
                assert!(!seconds[..index].contains(value), "duplicate idle case");
            }
            seconds
        }

        #[test]
        fn idle_probe_options_never_enable_implicit_waits() {
            assert!(idle_seconds(None).is_empty());
            assert_eq!(idle_seconds(Some("5,30,60")), [5, 30, 60]);
            assert_eq!(idle_seconds(Some("60")), [60]);
            for invalid in ["", "0", "1", "61", "5,5", "5,30,60,5", "5,"] {
                assert!(std::panic::catch_unwind(|| idle_seconds(Some(invalid))).is_err());
            }
        }

        impl Drop for Probe {
            fn drop(&mut self) {
                if self.queue.is_null() || unsafe { AudioQueueDispose(self.queue, 1) } == 0 {
                    unsafe {
                        ManuallyDrop::drop(&mut self.state);
                    }
                }
            }
        }

        #[test]
        #[ignore = "isolated muted native startup probe; requires exclusive browser/device timing slot"]
        fn native_pcm_startup_timings() {
            let idle = idle_seconds(
                std::env::var("WEBCORE_NATIVE_STARTUP_IDLE_SECONDS")
                    .ok()
                    .as_deref(),
            );
            let reopen_formats = match std::env::var("WEBCORE_NATIVE_STARTUP_FORMAT_REOPEN")
                .ok()
                .as_deref()
            {
                None | Some("0") => false,
                Some("1") => true,
                _ => panic!("WEBCORE_NATIVE_STARTUP_FORMAT_REOPEN must be 0 or 1"),
            };
            let unix_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_millis();
            eprintln!(
                "native startup probe unix_ms={unix_ms}: ordered trials; first trial is NOT guaranteed hardware-cold; no playback clocks or content PCM"
            );
            let mut first = Probe::new(48000);
            first.trial("fresh-unprimed", false);
            first.trial("same-queue-resume", false);
            drop(first);
            let mut primed = Probe::new(48000);
            primed.trial("fresh-primed-after-baseline", true);
            drop(primed);
            let mut last = Probe::new(48000);
            last.trial("fresh-unprimed-after-prime", false);
            drop(last);
            for seconds in idle {
                eprintln!(
                    "native startup probe opt_in_idle_seconds={seconds} own_queues_disposed=true other_audio_clients_not_checked=true hardware_cold_not_assumed=true"
                );
                std::thread::sleep(Duration::from_secs(seconds));
                let mut reopened = Probe::new(48000);
                reopened.trial(&format!("reopen-after-{seconds}s-idle"), false);
                drop(reopened);
            }
            if reopen_formats {
                for rate in [44100, 48000] {
                    let mut reopened = Probe::new(rate);
                    reopened.trial("opt-in-format-reopen", false);
                    drop(reopened);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pcm_shape_and_capacity_are_bounded_at_whole_frames() {
        assert!(validate_format(48000, 2).is_ok());
        assert!(validate_format(0, 2).is_err());
        assert!(validate_format(48000, 0).is_err());
        assert!(accepted_samples(&[0.0], 2).is_err());
        assert!(accepted_samples(&[f32::NAN, 0.0], 2).is_err());
        assert!(accepted_samples(&[f32::INFINITY], 1).is_err());
        assert_eq!(accepted_samples(&[], 2).unwrap(), 0);
        assert_eq!(
            accepted_samples(&vec![0.0; BUFFER_FRAMES * 4], 2).unwrap(),
            BUFFER_FRAMES * 2
        );
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit muted sound-device pause and backpressure test"]
    fn native_pcm_pause_and_backpressure_preserve_queued_frames() {
        let mut output = PcmOutput::new(48000, 2).unwrap();
        output.set_volume(0.0).unwrap();
        output.set_paused(true).unwrap();
        let samples = vec![0.0; BUFFER_FRAMES * 2];
        for _ in 0..4 {
            assert_eq!(output.submit(&samples).unwrap(), samples.len());
        }
        assert_eq!(output.submit(&samples), Err(PcmOutputError::WouldBlock));
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(output.completed_frames(), 0);
        assert!(!output.drained());
        output.set_paused(false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !output.drained() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(output.completed_frames(), (BUFFER_FRAMES * 4) as u64);
        output.set_paused(true).unwrap();
        assert_eq!(output.submit(&samples).unwrap(), samples.len());
        assert!(!output.drained());
        output.reset().unwrap();
        assert!(output.drained());
        assert_eq!(output.completed_frames(), 0);
        assert_eq!(output.submit(&samples).unwrap(), samples.len());
        std::thread::sleep(Duration::from_millis(30));
        assert_eq!(output.completed_frames(), 0);
        output.set_paused(false).unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !output.drained() {
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(Duration::from_millis(5));
        }
        assert_eq!(output.completed_frames(), BUFFER_FRAMES as u64);
        output.set_paused(true).unwrap();
        output.submit(&samples).unwrap();
        drop(output);
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit muted native-device underrun recovery test"]
    fn native_queue_recovers_after_repeated_decode_gaps() {
        let mut output = PcmOutput::new(48_000, 2).unwrap();
        output.set_volume(0.0).unwrap();
        let samples = vec![0.25; 960 * 2];
        for pass in 1..=8 {
            assert_eq!(output.submit(&samples).unwrap(), samples.len());
            let deadline = std::time::Instant::now() + Duration::from_secs(2);
            while !output.drained() {
                assert!(
                    std::time::Instant::now() < deadline,
                    "queue failed to resume after gap {pass}"
                );
                output.wait_for_buffer(Duration::from_millis(5));
            }
            assert_eq!(output.completed_frames(), pass * 960);
            std::thread::sleep(Duration::from_millis(150));
        }
    }

    #[cfg(target_os = "macos")]
    #[test]
    #[ignore = "explicit sound-device test with a supplied Vorbis WebM clip"]
    fn plays_supplied_vorbis_clip_with_native_pcm_buffers() {
        use webmedia::video::{MediaSample, StreamingMediaDecoder, webm::WebmMediaDecoder};
        let path = std::env::var_os("WEBCORE_AUDIO_WEBM").expect("WEBCORE_AUDIO_WEBM");
        let bytes = std::fs::read(path).unwrap();
        let mut decoder = WebmMediaDecoder::new();
        let mut output = None;
        let mut expected_frames = 0;
        let start = std::time::Instant::now();
        for chunk in bytes.chunks(4096) {
            let mut input = chunk;
            loop {
                for sample in decoder.push_media(input).unwrap() {
                    match sample {
                        MediaSample::Audio { samples, .. } => {
                            let output = output.get_or_insert_with(|| {
                                let mut output =
                                    PcmOutput::new(samples.sample_rate, samples.channels).unwrap();
                                output.set_volume(0.25).unwrap();
                                output.set_paused(true).unwrap();
                                output
                            });
                            expected_frames +=
                                (samples.samples.len() / usize::from(samples.channels)) as u64;
                            let mut remaining = samples.samples.as_slice();
                            while !remaining.is_empty() {
                                assert!(start.elapsed() < Duration::from_secs(10));
                                match output.submit(remaining) {
                                    Ok(count) => {
                                        remaining = &remaining[count..];
                                        output.set_paused(false).unwrap();
                                    }
                                    Err(PcmOutputError::WouldBlock) => {
                                        output.wait_for_buffer(Duration::from_millis(20));
                                    }
                                    result => panic!("PCM output failed: {result:?}"),
                                }
                            }
                        }
                        MediaSample::AudioError(error) => panic!("audio decode failed: {error:?}"),
                        MediaSample::Video(_) => {}
                    }
                }
                if !decoder.has_buffered_samples() {
                    break;
                }
                input = &[];
            }
        }
        decoder.finish().unwrap();
        let output = output.unwrap();
        while !output.drained() {
            assert!(start.elapsed() < Duration::from_secs(10));
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(expected_frames > 0);
        assert_eq!(output.completed_frames(), expected_frames);
        eprintln!(
            "native PCM completed {expected_frames} frames in {:?}",
            start.elapsed()
        );
    }
}
