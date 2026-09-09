# WebCore Video/Audio Module

This module owns HTML media element behavior that is independent of a codec
backend:

- `mod.rs`: `Document` media state, lifecycle events, time progression, seeking,
  volume/mute/rate state, and SVG eventbase bridging.
- `controls.rs`: native `<audio controls>` / `<video controls>` display-list
  painting and shared control hit geometry.
- `source.rs`: source selection and conservative `canPlayType` MIME support.
- `tracks.rs`: `<track>` discovery as text-track metadata.
- `backend.rs`: codec/backend trait and neutral decoded media types.
- `symphonia_backend.rs`: optional `audio-symphonia` implementation for audio
  decoding into interleaved `f32` samples.

Actual byte decoding, demuxing, audio output, and frame scheduling should plug
in behind this state model. The renderer should consume decoded frames the same
way it already consumes poster images, while the DOM-facing media state remains
the authority for events and controls.
