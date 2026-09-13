//! Symphonia-backed audio decoder.
//!
//! This is feature-gated because Symphonia is the first real media stack
//! dependency. It decodes audio into interleaved `f32` samples and leaves video
//! containers/codecs to a later backend.

use std::io::Cursor;

use symphonia::core::audio::{AudioBufferRef, Signal};
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error as SymphoniaError;
use symphonia::core::formats::FormatOptions;
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::sample::Sample;

use super::backend::{AudioSamples, DecodedMedia, MediaDecodeError, MediaDecoder, MediaMetadata};

pub struct SymphoniaAudioDecoder;

impl MediaDecoder for SymphoniaAudioDecoder {
    fn decode(&self, bytes: &[u8], mime: Option<&str>) -> Result<DecodedMedia, MediaDecodeError> {
        decode_audio(bytes, mime)
    }
}

pub fn decode_audio(bytes: &[u8], mime: Option<&str>) -> Result<DecodedMedia, MediaDecodeError> {
    if bytes.is_empty() {
        return Err(MediaDecodeError::InvalidData(
            "empty media payload".to_string(),
        ));
    }

    let mut hint = Hint::new();
    if let Some(ext) = extension_for_mime(mime) {
        hint.with_extension(ext);
    }

    let cursor = Cursor::new(bytes.to_vec());
    let mss = MediaSourceStream::new(Box::new(cursor), Default::default());
    let probed = symphonia::default::get_probe()
        .format(
            &hint,
            mss,
            &FormatOptions::default(),
            &MetadataOptions::default(),
        )
        .map_err(map_error)?;
    let mut format = probed.format;

    let track = format
        .tracks()
        .iter()
        .find(|track| track.codec_params.codec != CODEC_TYPE_NULL)
        .ok_or(MediaDecodeError::Unsupported)?;
    let track_id = track.id;
    let codec_params = track.codec_params.clone();
    let mut decoder = symphonia::default::get_codecs()
        .make(&codec_params, &DecoderOptions::default())
        .map_err(map_error)?;

    let mut samples = Vec::<f32>::new();
    let mut sample_rate = codec_params.sample_rate.unwrap_or(0);
    let mut channels = codec_params
        .channels
        .map(|channels| channels.count() as u16)
        .unwrap_or(0);

    const MAX_PACKETS: usize = 100_000;
    for _ in 0..MAX_PACKETS {
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(SymphoniaError::IoError(_)) => break,
            Err(SymphoniaError::ResetRequired) => break,
            Err(err) => return Err(map_error(err)),
        };
        if packet.track_id() != track_id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(SymphoniaError::DecodeError(_)) => continue,
            Err(err) => return Err(map_error(err)),
        };
        append_audio_samples(&decoded, &mut samples);
        if sample_rate == 0 {
            sample_rate = decoded.spec().rate;
        }
        if channels == 0 {
            channels = decoded.spec().channels.count() as u16;
        }
    }

    if sample_rate == 0 || channels == 0 || samples.is_empty() {
        return Err(MediaDecodeError::DecodeFailed(
            "no decodable audio samples".to_string(),
        ));
    }

    let duration = Some(samples.len() as f32 / channels as f32 / sample_rate as f32);
    Ok(DecodedMedia::Audio {
        metadata: MediaMetadata {
            duration,
            width: None,
            height: None,
            sample_rate: Some(sample_rate),
            channels: Some(channels),
        },
        samples: AudioSamples {
            sample_rate,
            channels,
            samples,
        },
    })
}

fn append_audio_samples(decoded: &AudioBufferRef<'_>, out: &mut Vec<f32>) {
    match decoded {
        AudioBufferRef::F32(buf) => append_planar(buf, out, |v| v),
        AudioBufferRef::U8(buf) => append_planar(buf, out, |v| (v as f32 - 128.0) / 128.0),
        AudioBufferRef::U16(buf) => append_planar(buf, out, |v| (v as f32 - 32768.0) / 32768.0),
        AudioBufferRef::U24(buf) => append_planar(buf, out, |v| {
            (v.clamped().inner() as f32 / 8_388_608.0) - 1.0
        }),
        AudioBufferRef::U32(buf) => append_planar(buf, out, |v| {
            (v as f64 - 2_147_483_648.0) as f32 / 2_147_483_648.0
        }),
        AudioBufferRef::S8(buf) => append_planar(buf, out, |v| v as f32 / 128.0),
        AudioBufferRef::S16(buf) => append_planar(buf, out, |v| v as f32 / 32768.0),
        AudioBufferRef::S24(buf) => {
            append_planar(buf, out, |v| v.clamped().inner() as f32 / 8_388_608.0)
        }
        AudioBufferRef::S32(buf) => append_planar(buf, out, |v| v as f32 / 2_147_483_648.0),
        AudioBufferRef::F64(buf) => append_planar(buf, out, |v| v as f32),
    }
}

fn append_planar<T, F>(buf: &symphonia::core::audio::AudioBuffer<T>, out: &mut Vec<f32>, convert: F)
where
    T: Sample,
    F: Fn(T) -> f32,
{
    let channels = buf.spec().channels.count();
    for frame in 0..buf.frames() {
        for channel in 0..channels {
            out.push(convert(buf.chan(channel)[frame]));
        }
    }
}

fn extension_for_mime(mime: Option<&str>) -> Option<&'static str> {
    let mime = mime?.split(';').next()?.trim().to_ascii_lowercase();
    match mime.as_str() {
        "audio/mpeg" | "audio/mp3" => Some("mp3"),
        "audio/mp4" | "audio/aac" => Some("m4a"),
        "audio/ogg" | "application/ogg" => Some("ogg"),
        "audio/webm" => Some("webm"),
        "audio/wav" | "audio/x-wav" => Some("wav"),
        "audio/flac" => Some("flac"),
        _ => None,
    }
}

fn map_error(err: SymphoniaError) -> MediaDecodeError {
    match err {
        SymphoniaError::Unsupported(_) => MediaDecodeError::Unsupported,
        SymphoniaError::DecodeError(msg) => MediaDecodeError::DecodeFailed(msg.to_string()),
        other => MediaDecodeError::InvalidData(other.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn symphonia_decodes_pcm_wav_to_interleaved_f32_samples() {
        let wav = tiny_pcm_wav();
        let decoded = decode_audio(&wav, Some("audio/wav")).expect("decode wav");
        let DecodedMedia::Audio { metadata, samples } = decoded else {
            panic!("expected audio");
        };

        assert_eq!(metadata.sample_rate, Some(8000));
        assert_eq!(metadata.channels, Some(1));
        assert_eq!(samples.sample_rate, 8000);
        assert_eq!(samples.channels, 1);
        assert_eq!(samples.samples.len(), 4);
        assert!(samples.samples[1] > 0.49 && samples.samples[1] < 0.51);
        assert!(samples.samples[2] < -0.49 && samples.samples[2] > -0.51);
        assert!(metadata.duration.is_some_and(|v| v > 0.0));
    }

    fn tiny_pcm_wav() -> Vec<u8> {
        let mut bytes = Vec::new();
        let samples: [i16; 4] = [0, 16384, -16384, 0];
        let data_len = (samples.len() * 2) as u32;
        bytes.extend_from_slice(b"RIFF");
        bytes.extend_from_slice(&(36 + data_len).to_le_bytes());
        bytes.extend_from_slice(b"WAVE");
        bytes.extend_from_slice(b"fmt ");
        bytes.extend_from_slice(&16u32.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&1u16.to_le_bytes());
        bytes.extend_from_slice(&8000u32.to_le_bytes());
        bytes.extend_from_slice(&16000u32.to_le_bytes());
        bytes.extend_from_slice(&2u16.to_le_bytes());
        bytes.extend_from_slice(&16u16.to_le_bytes());
        bytes.extend_from_slice(b"data");
        bytes.extend_from_slice(&data_len.to_le_bytes());
        for sample in samples {
            bytes.extend_from_slice(&sample.to_le_bytes());
        }
        bytes
    }
}
