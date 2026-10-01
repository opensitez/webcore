use super::{MediaMetadata, StreamingVideoDecoder, y4m::Y4mStream};

#[test]
fn mp4_metadata_sizes_video_before_picture_arrives() {
    let mut doc = crate::html::parse_html("<video id=movie src=movie.mp4></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    assert!(doc.media_apply_video_metadata(
        id,
        MediaMetadata {
            duration: Some(87.06),
            width: Some(1280),
            height: Some(720),
            sample_rate: None,
            channels: None,
        }
    ));
    let node = doc.find_webcore(id).unwrap();
    assert_eq!((node.image_width, node.image_height), (1280, 720));
    assert!(node.image_data.is_none());
    assert_eq!(doc.media_duration(id), Some(87.06));
}

#[test]
fn decoded_frame_reaches_video_paint_data() {
    let mut doc = crate::html::parse_html("<video id=movie src=clip.y4m></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    let mut stream = Y4mStream::new();
    assert!(
        stream
            .push(b"YUV4MPEG2 W2 H2 F25:1 C420\nFRAME\n\x10\x10")
            .unwrap()
            .is_empty()
    );
    assert!(doc.find_webcore(id).unwrap().image_data.is_none());
    let frames = stream.push(b"\x10\x10\x80\x80").unwrap();
    assert_eq!(frames.len(), 1);
    assert!(doc.media_present_video_frame(id, frames.into_iter().next().unwrap()));
    let node = doc.find_webcore(id).unwrap();
    assert_eq!((node.image_width, node.image_height), (2, 2));
    assert_eq!(&node.image_data.as_ref().unwrap()[..4], &[0, 0, 0, 255]);
    assert!(doc.needs_animation_frame);
}

#[test]
fn streamed_frame_is_painted_by_video_element() {
    let doc = crate::html::parse_html("<video id=movie width=100 height=80 controls></video>");
    let mut engine = crate::frame::EngineFrame::new(doc, 200.0, 150.0);
    let id = engine.doc.get_element_by_id("movie").unwrap();
    let mut stream = Y4mStream::new();
    let frames = stream
        .push(b"YUV4MPEG2 W2 H2 F25:1 C420\nFRAME\n\xeb\xeb\xeb\xeb\x80\x80")
        .unwrap();
    assert!(
        engine
            .doc
            .media_present_video_frame(id, frames.into_iter().next().unwrap())
    );
    engine.update_frame();
    let list =
        crate::renderer::display_list_builder::build_display_list(&engine.doc.root, 200.0, 150.0);
    assert!(list.commands.iter().any(|command| matches!(
        command,
        crate::renderer::display_list::PaintCmd::Image {
            data: crate::renderer::display_list::ImageRef::Shared(_, 2, 2),
            ..
        }
    )));
}

#[test]
fn playback_clock_presents_frames_at_their_timestamps() {
    let mut doc = crate::html::parse_html("<video id=movie src=clip.y4m></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    let mut stream = Y4mStream::new();
    let frames = stream.push(b"YUV4MPEG2 W2 H2 F25:1 C420\nFRAME\n\x10\x10\x10\x10\x80\x80FRAME\n\xeb\xeb\xeb\xeb\x80\x80").unwrap();
    assert!(doc.media_queue_video_frames(id, frames));
    assert_eq!(
        &doc.find_webcore(id).unwrap().image_data.as_ref().unwrap()[..4],
        &[0, 0, 0, 255]
    );
    assert!(doc.media_play(id));
    let start = std::time::Instant::now();
    doc.media_states.get_mut(&id).unwrap().last_tick = Some(start);
    doc.tick_media(start + std::time::Duration::from_millis(50));
    assert_eq!(
        &doc.find_webcore(id).unwrap().image_data.as_ref().unwrap()[..4],
        &[255, 255, 255, 255]
    );
    assert!(
        doc.media_states
            .get(&id)
            .unwrap()
            .pending_video_frames
            .is_empty()
    );
}

#[test]
fn playback_waits_for_first_and_subsequent_streamed_frames() {
    let mut doc = crate::html::parse_html("<video id=movie src=clip.y4m></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    assert!(doc.media_play(id));
    let start = std::time::Instant::now();
    doc.media_states.get_mut(&id).unwrap().last_tick = Some(start);
    doc.tick_media(start + std::time::Duration::from_secs(30));
    assert_eq!(doc.media_current_time(id), Some(0.0));

    let mut stream = Y4mStream::new();
    let first = stream
        .push(b"YUV4MPEG2 W2 H2 F25:1 C420\nFRAME\n\x10\x10\x10\x10\x80\x80")
        .unwrap();
    assert!(doc.media_queue_video_frames(id, first));
    assert_eq!(doc.media_current_time(id), Some(0.0));
    let second = stream.push(b"FRAME\n\xeb\xeb\xeb\xeb\x80\x80").unwrap();
    assert!(doc.media_queue_video_frames(id, second));
    let playing = doc.media_states.get(&id).unwrap().last_tick.unwrap();
    doc.tick_media(playing + std::time::Duration::from_millis(50));
    assert_eq!(doc.media_current_time(id), Some(0.05));
    assert_eq!(
        &doc.find_webcore(id).unwrap().image_data.as_ref().unwrap()[..4],
        &[255, 255, 255, 255]
    );
    doc.tick_media(playing + std::time::Duration::from_secs(10));
    assert_eq!(doc.media_current_time(id), Some(0.05));
}

#[test]
fn full_video_queue_does_not_drop_unpresented_frames() {
    let mut doc = crate::html::parse_html("<video id=movie src=clip.y4m></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    for number in 0..8 {
        assert!(doc.media_queue_video_frames(
            id,
            vec![super::VideoFrame {
                width: 1,
                height: 1,
                rgba: std::sync::Arc::new(vec![number, 0, 0, 255]),
                timestamp: number as f32,
            }]
        ));
    }
    assert!(!doc.media_queue_video_frames(
        id,
        vec![super::VideoFrame {
            width: 1,
            height: 1,
            rgba: std::sync::Arc::new(vec![8, 0, 0, 255]),
            timestamp: 8.0,
        }]
    ));
    let pending = &doc.media_states.get(&id).unwrap().pending_video_frames;
    assert_eq!(pending.len(), 8);
    assert_eq!(pending.front().unwrap().timestamp, 0.0);
}
