//! Media backend boundary.
//!
//! The DOM/control runtime is independent from codecs. Decoder implementations
//! feed metadata, audio samples, and video frames through this narrow surface.

#[derive(Clone, Debug, PartialEq)]
pub struct MediaMetadata {
    pub duration: Option<f32>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub sample_rate: Option<u32>,
    pub channels: Option<u16>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AudioSamples {
    pub sample_rate: u32,
    pub channels: u16,
    pub samples: Vec<f32>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VideoFrame {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
    pub timestamp: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub enum DecodedMedia {
    Audio {
        metadata: MediaMetadata,
        samples: AudioSamples,
    },
    Video {
        metadata: MediaMetadata,
        first_frame: Option<VideoFrame>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaDecodeError {
    Unsupported,
    InvalidData(String),
    DecodeFailed(String),
}

pub trait MediaDecoder {
    fn decode(&self, bytes: &[u8], mime: Option<&str>) -> Result<DecodedMedia, MediaDecodeError>;
}

pub struct NullMediaDecoder;

impl MediaDecoder for NullMediaDecoder {
    fn decode(&self, _bytes: &[u8], _mime: Option<&str>) -> Result<DecodedMedia, MediaDecodeError> {
        Err(MediaDecodeError::Unsupported)
    }
}
