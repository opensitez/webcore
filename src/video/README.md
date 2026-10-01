# WebCore Video/Audio Module

This module owns HTML media element behavior. Format parsing and decoding now
live in the standalone `webmedia` crate; WebCore re-exports those modules to
preserve existing paths.

- `mod.rs`: `Document` media state, lifecycle events, time progression, seeking,
  volume/mute/rate state, and SVG eventbase bridging.
- `controls.rs`: native `<audio controls>` / `<video controls>` display-list
  painting and shared control hit geometry.
- `source.rs`: source selection and conservative `canPlayType` MIME support.
- `tracks.rs`: `<track>` discovery as text-track metadata.
- `integration_tests.rs`: browser-side tests of decoded frames, metadata, and
  playback presentation.

In `webmedia::video`:

- `backend.rs`: codec/backend trait and neutral decoded media types.
- `av1.rs`: bounded incremental AV1 OBU framing; pixel decoding is not implemented.
- `h264.rs`: bounded `avcC`, length-prefixed NAL, 2003/2005 parameter sets,
  IDR I-slice headers, and limited 2003 Baseline intra reconstruction.
- `h264_cabac.rs`: original 2003 CABAC arithmetic core, I-slice macroblock
  type, prediction, coded-block-pattern, QP, 4x4 luma and 4:2:0 chroma-DC
  residual syntax, plus the 2005 8x8 transform flag. CABAC picture
  reconstruction and the remaining residual forms are not connected yet.
- `h264_transform.rs`: original 2003 frame zig-zag scan, 4x4 inverse
  scaling/integer transform, chroma QP mapping and chroma DC transform, with
  bounded coefficient checks.
- `h264_intra.rs`: all nine original 2003 luma Intra_4x4 prediction modes
  and decoding-order 16x16 luma macroblock construction, plus DC-only 4:2:0
  chroma macroblock reconstruction.
- `mp4.rs`: front-loaded MP4/AVC sample-table indexing, including decode and
  presentation timing and byte offsets.
- `mp4_avc.rs`: incremental MP4 sample extraction and bounded 2003 Baseline
  frame delivery through the media backend.

The in-house AVC implementation is scoped to the original 2003 recommendation
and its March 2005 High-family additions. Later AVC extensions are out of scope.
The 2003 picture path currently reconstructs complete uncropped progressive
Baseline IDR I-slices containing I_PCM and zero-residual Intra16x16 DC
macroblocks. Full CAVLC residuals, inter prediction, deblocking, CABAC picture
reconstruction, and 2005 High-profile picture features remain unimplemented.
The MP4 indexer has been exercised against the maroc.ma video prefix and is
connected to the browser media source, including early dimensions and duration.
That site's High-profile pictures are still unsupported and do not play yet.
This technical boundary is not a patent-clearance determination.
- `y4m.rs`: dependency-free incremental YUV4MPEG2 frame decoding for raw video streams.
- `symphonia_backend.rs`: optional `audio-symphonia` implementation for audio
  decoding into interleaved `f32` samples.

Y4M sources are fetched incrementally from local files or HTTP(S), decoded,
queued against the media clock, and painted through the existing image command.
Compressed MP4/H.264 support is currently limited to the Baseline intra subset
above; AV1 and WebM picture decoding, AVIF/AV1 container and pixel decoding,
and audio output are not yet connected.
