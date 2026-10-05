use super::{MediaMetadata, StreamingVideoDecoder, y4m::Y4mStream};

#[test]
fn presentation_dimensions_preserve_coded_buffers_and_intrinsic_layout() {
    use std::sync::Arc;
    let mut doc = crate::html::parse_html("<video id=movie style='width:100px;height:100px;object-fit:contain'></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    assert!(doc.media_apply_video_metadata(id, MediaMetadata {
        presentation_size: Some((8.0, 4.0)), duration: Some(1.0), width: Some(4), height: Some(4),
        sample_rate: None, channels: None,
    }));
    assert_eq!(doc.find_webcore(id).map(|node| (node.image_width, node.image_height)), Some((8, 4)));
    let pixels = Arc::new(vec![255u8; 4 * 4 * 4]);
    assert!(doc.media_present_video_frame(id, super::VideoFrame {
        presentation_size: Some((8.0, 4.0)), width: 4, height: 4,
        rgba: pixels.clone(), timestamp: 0.0,
    }));
    let node = doc.find_webcore(id).unwrap();
    assert_eq!((node.image_width, node.image_height), (8, 4));
    assert_eq!((node.image_data_width, node.image_data_height), (4, 4));
    assert!(Arc::ptr_eq(node.image_data.as_ref().unwrap(), &pixels));
    let mut engine = crate::frame::EngineFrame::new(doc, 200.0, 200.0);
    engine.update_frame();
    let list = crate::renderer::display_list_builder::build_display_list(&engine.doc.root, 200.0, 200.0);
    assert!(list.commands.iter().any(|command| matches!(command,
        crate::renderer::display_list::PaintCmd::Image {
            rect, data: crate::renderer::display_list::ImageRef::Shared(data, 4, 4),
        } if Arc::ptr_eq(data, &pixels) && (rect.w - 100.0).abs() < 0.01 && (rect.h - 50.0).abs() < 0.01)));
}

#[test]
fn native_timeline_click_seeks_without_toggling_playback() {
    let mut doc = crate::html::parse_html("<video id=movie src=movie.webm width=640 height=360 controls></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    doc.media_apply_video_metadata(id, MediaMetadata {
        presentation_size: None,
        duration: Some(120.0), width: Some(640), height: Some(360),
        sample_rate: None, channels: None,
    });
    let mut frame = crate::frame::EngineFrame::new(doc, 640.0, 400.0);
    frame.update_frame();
    assert!(frame.doc.media_play(id));
    let layout = super::controls::media_control_layout(frame.doc.find_webcore(id).unwrap()).unwrap();
    let point = (layout.timeline_x + layout.timeline_w * 0.5, layout.rail_y + layout.rail_h * 0.5);
    frame.doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, point, 0);
    frame.doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, point, 0);
    assert_eq!(frame.doc.media_current_time(id), Some(60.0));
    assert_eq!(frame.doc.media_paused(id), Some(false));
    assert_eq!(frame.doc.media_states[&id].seek_revision, 1);
    let mute = layout.mute_rect.unwrap();
    let point = (mute.x + mute.w * 0.5, mute.y + mute.h * 0.5);
    frame.doc.process_mouse_event(crate::dom::HtmlEventType::MouseDown, point, 0);
    frame.doc.process_mouse_event(crate::dom::HtmlEventType::MouseUp, point, 0);
    assert_eq!(frame.doc.media_muted(id), Some(true));
    assert_eq!(frame.doc.media_paused(id), Some(false));
    assert_eq!(frame.doc.media_current_time(id), Some(60.0));
    let list = crate::renderer::display_list_builder::build_display_list(&frame.doc.root, 640.0, 400.0);
    assert!(list.commands.iter().any(|command|
        matches!(command, crate::renderer::display_list::PaintCmd::Text { text, .. } if text == "1:00 / 2:00")));
}

#[test]
fn device_clock_selects_video_frames_without_browser_time_advance() {
    use std::{sync::Arc, time::{Duration, Instant}};
    let mut doc = crate::html::parse_html("<video id=movie src=movie.webm></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    doc.media_apply_video_metadata(id, MediaMetadata {
        presentation_size: None,
        duration: Some(2.0), width: Some(2), height: Some(2), sample_rate: Some(48_000), channels: Some(2),
    });
    assert!(doc.media_play(id));
    let frames = [0.0, 0.5, 1.0].into_iter().enumerate().map(|(index, timestamp)| super::VideoFrame {
        presentation_size: None,
        width: 2, height: 2, timestamp,
        rgba: Arc::new(vec![(index as u8) * 80, 0, 0, 255].repeat(4)),
    }).collect();
    assert!(doc.media_queue_video_frames(id, frames));
    let now = Instant::now();
    doc.tick_media_with_external_clocks(now + Duration::from_secs(10), &[(id, 0.25)]);
    assert_eq!(doc.media_current_time(id), Some(0.25));
    assert_eq!(doc.find_webcore(id).unwrap().image_data.as_ref().unwrap()[0], 0);
    doc.tick_media_with_external_clocks(now + Duration::from_secs(20), &[(id, 0.25)]);
    assert_eq!(doc.media_current_time(id), Some(0.25));
    assert_eq!(doc.find_webcore(id).unwrap().image_data.as_ref().unwrap()[0], 0);
    let (_, presented) = doc.tick_media_with_external_clocks(now + Duration::from_secs(21), &[(id, 0.5)]);
    assert_eq!(presented, vec![id]);
    assert_eq!(doc.media_current_time(id), Some(0.5));
    assert_eq!(doc.find_webcore(id).unwrap().image_data.as_ref().unwrap()[0], 80);
    doc.tick_media_with_external_clocks(now + Duration::from_secs(22), &[(id, 1.5)]);
    assert!(doc.media_states[&id].pending_video_frames.is_empty());
    assert!(!doc.media_states[&id].ended);
    doc.tick_media_with_external_clocks(now + Duration::from_secs(23), &[(id, 2.0)]);
    assert!(doc.media_states[&id].ended);
    assert!(doc.media_states[&id].paused);
}

#[test]
fn mp4_metadata_sizes_video_before_picture_arrives() {
    let mut doc = crate::html::parse_html("<video id=movie src=movie.mp4></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    assert!(doc.media_apply_video_metadata(
        id,
        MediaMetadata {
            presentation_size: None,
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
fn external_video_layer_omits_software_frame_and_keeps_background() {
    let doc = crate::html::parse_html("<video id=movie width=100 height=80></video>");
    let mut engine = crate::frame::EngineFrame::new(doc, 200.0, 150.0);
    let id = engine.doc.get_element_by_id("movie").unwrap();
    engine.update_frame();
    assert!(engine.doc.media_present_video_frame(
        id,
        crate::video::backend::VideoFrame {
            presentation_size: None,
            width: 2,
            height: 2,
            rgba: std::sync::Arc::new(vec![90, 120, 150, 255].repeat(4)),
            timestamp: 0.0,
        },
    ));
    engine.doc.find_webcore_mut(id).unwrap().external_video_overlay = true;
    let list = crate::renderer::display_list_builder::build_display_list(
        &engine.doc.root, 200.0, 150.0,
    );
    assert!(!list.commands.iter().any(|cmd| matches!(
        cmd,
        crate::renderer::display_list::PaintCmd::Image {
            data: crate::renderer::display_list::ImageRef::Shared(_, 2, 2),
            ..
        }
    )));
    assert!(list.commands.iter().any(|cmd| matches!(
        cmd,
        crate::renderer::display_list::PaintCmd::FillRect { radius, .. }
            if *radius == [4.0; 4]
    )));
}

#[test]
fn video_shows_center_play_affordance_only_when_paused() {
    let doc = crate::html::parse_html("<video id=movie width=100 height=80 src=clip.y4m></video>");
    let mut engine = crate::frame::EngineFrame::new(doc, 200.0, 150.0);
    let id = engine.doc.get_element_by_id("movie").unwrap();
    engine.update_frame();
    let paused =
        crate::renderer::display_list_builder::build_display_list(&engine.doc.root, 200.0, 150.0);
    assert!(paused.commands.iter().any(|command| matches!(
        command,
        crate::renderer::display_list::PaintCmd::PushClipPath { points, .. } if points.len() == 3
    )));
    assert!(engine.doc.media_play(id));
    let playing =
        crate::renderer::display_list_builder::build_display_list(&engine.doc.root, 200.0, 150.0);
    assert!(!playing.commands.iter().any(|command| matches!(
        command,
        crate::renderer::display_list::PaintCmd::PushClipPath { points, .. } if points.len() == 3
    )));
}

#[test]
fn video_fallback_markup_is_not_painted_as_page_text() {
    let doc = crate::html::parse_html(
        "<video id=v width=100 height=80><source src=clip.mp4 type='video/mp4; codecs=&quot;hvc1&quot;'>Video not supported.</video>",
    );
    let mut engine = crate::frame::EngineFrame::new(doc, 200.0, 150.0);
    engine.update_frame();
    let list =
        crate::renderer::display_list_builder::build_display_list(&engine.doc.root, 200.0, 150.0);
    assert!(!list.commands.iter().any(|command| matches!(
        command,
        crate::renderer::display_list::PaintCmd::Text { text, .. } if text.contains("Video not supported")
    )));
    let video = engine
        .doc
        .find_webcore(engine.doc.get_element_by_id("v").unwrap())
        .unwrap();
    assert_eq!(video.layout.content_rect.w, 100.0);
    assert_eq!(video.layout.content_rect.h, 80.0);
    assert!(video.children.iter().all(|child| child.layout.content_rect.h == 0.0));
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
fn looping_stream_uses_monotonic_frame_times_but_wrapped_media_time() {
    let mut doc = crate::html::parse_html("<video id=movie src=clip.y4m loop></video>");
    let id = doc.get_element_by_id("movie").unwrap();
    assert!(doc.media_apply_video_metadata(
        id,
        MediaMetadata {
            presentation_size: None,
            duration: Some(0.08),
            width: Some(1),
            height: Some(1),
            sample_rate: None,
            channels: None,
        }
    ));
    for (timestamp, value) in [(0.0, 10), (0.04, 20), (0.08, 30), (0.12, 40)] {
        assert!(doc.media_queue_video_frames(
            id,
            vec![super::VideoFrame {
                presentation_size: None,
                width: 1,
                height: 1,
                rgba: std::sync::Arc::new(vec![value, value, value, 255]),
                timestamp,
            }]
        ));
    }
    assert!(doc.media_play(id));
    let start = std::time::Instant::now();
    doc.media_states.get_mut(&id).unwrap().last_tick = Some(start);
    doc.tick_media(start + std::time::Duration::from_millis(90));
    assert!(!doc.media_paused(id).unwrap());
    assert!(!doc.media_ended(id).unwrap());
    assert!((doc.media_current_time(id).unwrap() - 0.01).abs() < 0.001);
    assert_eq!(
        doc.find_webcore(id).unwrap().image_data.as_ref().unwrap()[0],
        30
    );
    doc.tick_media(start + std::time::Duration::from_millis(130));
    assert!((doc.media_current_time(id).unwrap() - 0.05).abs() < 0.001);
    assert_eq!(
        doc.find_webcore(id).unwrap().image_data.as_ref().unwrap()[0],
        40
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
                presentation_size: None,
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
            presentation_size: None,
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
