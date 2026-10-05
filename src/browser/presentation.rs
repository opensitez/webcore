//! Native media composition shared by the standalone browser.
use super::*;

pub(super) fn intersects(a: Rect, b: Rect) -> bool {
    a.w > 0.0
        && a.h > 0.0
        && b.w > 0.0
        && b.h > 0.0
        && a.x < b.right()
        && a.right() > b.x
        && a.y < b.bottom()
        && a.bottom() > b.y
}

pub(super) fn video_is_unobstructed(node: &WebCore, video_id: u32, video_rect: Rect) -> bool {
    if node.node_id == video_id {
        return true;
    }
    let Some(target_child) = node
        .children
        .iter()
        .find(|child| contains_node(child, video_id))
    else {
        return false;
    };
    if node.style.has_transform() || node.style.opacity < 1.0 {
        return false;
    }
    node.children.iter().all(|child| {
        std::ptr::eq(child, target_child) || !intersects(child.layout.border_rect, video_rect)
    }) && video_is_unobstructed(target_child, video_id, video_rect)
}

pub(super) fn contains_node(node: &WebCore, id: u32) -> bool {
    node.node_id == id || node.children.iter().any(|child| contains_node(child, id))
}

pub(super) fn find_node(node: &WebCore, id: u32) -> Option<&WebCore> {
    if node.node_id == id {
        return Some(node);
    }
    node.children.iter().find_map(|child| find_node(child, id))
}

pub(super) fn find_node_mut(node: &mut WebCore, id: u32) -> Option<&mut WebCore> {
    if node.node_id == id {
        return Some(node);
    }
    node.children
        .iter_mut()
        .find_map(|child| find_node_mut(child, id))
}

pub(super) fn collect_videos<'a>(node: &'a WebCore, videos: &mut Vec<&'a WebCore>) {
    if node.tag == "video" {
        videos.push(node);
    }
    for child in &node.children {
        collect_videos(child, videos);
    }
}

pub(super) fn single_video(node: &WebCore) -> Option<&WebCore> {
    fn visit<'a>(node: &'a WebCore, found: &mut Option<&'a WebCore>) -> bool {
        if node.tag == "video" {
            if found.is_some() { return false; }
            *found = Some(node);
        }
        node.children.iter().all(|child| visit(child, found))
    }
    let mut found = None;
    visit(node, &mut found).then_some(found).flatten()
}

pub(super) fn composited_video_candidate(
    doc: &Document,
    page_w: f32,
    page_h: f32,
    chrome_height: f32,
) -> Option<(u32, VideoLayerFrame)> {
    if doc.scroll_y != 0.0 {
        return None;
    }
    let video = single_video(&doc.root)?;
    let rect = video.layout.content_rect;
    let visible = Rect::new(0.0, 0.0, page_w, page_h);
    if !intersects(rect, visible)
        || rect.x < 0.0
        || rect.y < 0.0
        || rect.right() > page_w
        || rect.bottom() > page_h
        || video.media_paused
        || video.attributes.contains_key("controls")
        || video.style.has_transform()
        || video.style.opacity < 1.0
        || !matches!(video.style.object_fit, ObjectFit::Fill | ObjectFit::Cover)
        || video.style.object_position_x != CssLength::Percent(50.0)
        || video.style.object_position_y != CssLength::Percent(50.0)
    {
        return None;
    }
    let pixels = video.image_data.as_ref()?;
    if video.image_data_width == 0
        || video.image_data_height == 0
        || u64::from(video.image_data_width) * u64::from(video.image_data_height) * 4 != pixels.len() as u64
    {
        return None;
    }
    Some((
        video.node_id,
        VideoLayerFrame {
            rgba: pixels.clone(),
            source_width: video.image_data_width,
            source_height: video.image_data_height,
            presentation_size: Some((video.image_width, video.image_height)),
            x: rect.x,
            y: rect.y + chrome_height,
            width: rect.w,
            height: rect.h,
            cover: video.style.object_fit == ObjectFit::Cover,
            corner_radius: 4.0,
            tint: None,
            foreground: None,
        },
    ))
}

#[cfg(test)]
mod video_discovery_tests {
    use super::{Arc, CssLength, Document, ObjectFit, Rect, WebCore, collect_videos,
        composited_video_candidate, single_video};

    #[test]
    fn actual_document_candidate_updates_after_video_insert_remove_and_controls() {
        let mut doc = Document::new();
        let mut video = WebCore::new("video");
        video.media_paused = false;
        video.layout.content_rect = Rect::new(0.0, 0.0, 320.0, 180.0);
        video.image_width = 4;
        video.image_height = 4;
        video.image_data_width = 4;
        video.image_data_height = 4;
        video.image_data = Some(Arc::new(vec![255; 4 * 4 * 4]));
        let style = Arc::make_mut(&mut video.style);
        style.opacity = 1.0;
        style.object_fit = ObjectFit::Fill;
        style.object_position_x = CssLength::Percent(50.0);
        style.object_position_y = CssLength::Percent(50.0);
        let id = video.node_id;
        doc.root.children.push(video);
        let (candidate_id, frame) = composited_video_candidate(&doc, 320.0, 180.0, 40.0).unwrap();
        assert_eq!(candidate_id, id);
        assert_eq!((frame.x, frame.y, frame.width, frame.height), (0.0, 40.0, 320.0, 180.0));
        assert!(Arc::ptr_eq(&frame.rgba, doc.root.children[0].image_data.as_ref().unwrap()));
        doc.root.children[0].image_width = 8;
        Arc::make_mut(&mut doc.root.children[0].style).object_fit = ObjectFit::Cover;
        let (_, scaled) = composited_video_candidate(&doc, 320.0, 180.0, 40.0).unwrap();
        assert_eq!((scaled.source_width, scaled.source_height), (4, 4));
        assert_eq!(scaled.presentation_size, Some((8, 4)));
        assert!(scaled.is_opaque());
        assert!((scaled.contents_rect().2 - 8.0 / 9.0).abs() < 1e-6);
        doc.root.children.push(WebCore::new("video"));
        assert!(composited_video_candidate(&doc, 320.0, 180.0, 40.0).is_none());
        doc.root.children.pop();
        assert!(composited_video_candidate(&doc, 320.0, 180.0, 40.0).is_some());
        doc.root.children[0].attributes.insert("controls", "");
        assert!(composited_video_candidate(&doc, 320.0, 180.0, 40.0).is_none());
        doc.root.children[0].attributes.remove("controls");
        doc.scroll_y = 1.0;
        assert!(composited_video_candidate(&doc, 320.0, 180.0, 40.0).is_none());
    }

    #[test]
    fn single_video_matches_collector_without_retaining_tree_state() {
        let mut root = WebCore::new("body");
        root.children.push(WebCore::new("audio"));
        root.children.push(WebCore::new("div"));
        for count in 0..4 {
            let mut videos = Vec::new();
            collect_videos(&root, &mut videos);
            let expected = if videos.len() == 1 { Some(videos[0]) } else { None };
            match (single_video(&root), expected) {
                (Some(actual), Some(expected)) => assert!(std::ptr::eq(actual, expected)),
                (None, None) => {}
                _ => panic!("single-video query disagrees with the collector"),
            }
            if count == 0 {
                root.children[1].children.push(WebCore::new("video"));
            } else {
                root.children[1].children[0].children.push(WebCore::new("video"));
            }
        }
        root.children[1].children[0].children.clear();
        assert!(single_video(&root).is_some());
        root.children[1].children.clear();
        assert!(single_video(&root).is_none());
        assert!(single_video(&WebCore::new("video")).is_some());
    }

    #[test]
    #[ignore = "same-process single-video traversal versus allocating collector ABBA benchmark"]
    fn benchmark_single_video_discovery() {
        for positions in [vec![], vec![0], vec![8191], vec![0, 1], vec![0, 8191]] {
            let mut root = WebCore::new("body");
            root.children = (0..8192).map(|index|
                WebCore::new(if positions.contains(&index) { "video" } else { "div" })).collect();
            let mut videos = Vec::new();
            collect_videos(&root, &mut videos);
            assert_eq!(single_video(&root).map(|node| node.node_id),
                (videos.len() == 1).then(|| videos[0].node_id));
            drop(videos);
            for nonalloc in [false, true, true, false] {
                let start = std::time::Instant::now();
                for _ in 0..1000 {
                    let root = std::hint::black_box(&root);
                    if nonalloc {
                        std::hint::black_box(single_video(root));
                    } else {
                        let mut videos = Vec::new();
                        collect_videos(root, &mut videos);
                        std::hint::black_box(videos);
                    }
                }
                eprintln!("video discovery nodes=8193 videos={positions:?} nonalloc={nonalloc} x1000 ms={:.3}",
                    start.elapsed().as_secs_f64() * 1000.0);
            }
        }
    }
}

pub(super) struct NativeVideoComposition {
    pub(super) tab: usize,
    pub(super) video_id: u32,
    pub(super) page_w: f32,
    pub(super) page_h: f32,
    pub(super) scale: f32,
    pub(super) tint: [u8; 4],
    pub(super) foreground: VideoForeground,
}

pub(super) fn build_native_video_composition(
    view: &mut crate::BrowserView,
    tab: usize,
    video_id: u32,
    video_rect: Rect,
    page_w: f32,
    page_h: f32,
    scale: f32,
    chrome_height: f32,
) -> Option<NativeVideoComposition> {
    let (mut doc, renderer) = view.document_and_renderer_current_mut()?;
    let pixels = find_node(&doc.root, video_id)?.image_data.clone()?;
    let was_external = find_node(&doc.root, video_id)?.external_video_overlay;
    if was_external {
        find_node_mut(&mut doc.root, video_id)?.external_video_overlay = false;
    }
    let font_system = Some(&mut renderer.font_system as *mut _);
    let list = build_display_list_full_with_font_system(
        &doc.root,
        page_w,
        page_h,
        doc.scroll_x,
        doc.scroll_y,
        0,
        0,
        &std::collections::HashSet::new(),
        &doc.base_url,
        font_system,
    );
    if was_external {
        find_node_mut(&mut doc.root, video_id)?.external_video_overlay = true;
    }
    let video_index = list.commands.iter().position(|command| {
        matches!(command, PaintCmd::Image { data: ImageRef::Shared(data, ..), .. } if Arc::ptr_eq(data, &pixels))
    })?;
    let mut clips = Vec::new();
    for command in &list.commands[..video_index] {
        match command {
            PaintCmd::PushClip { .. } => clips.push(command.clone()),
            PaintCmd::PopClip => {
                clips.pop()?;
            }
            PaintCmd::PushTransform { .. }
            | PaintCmd::PushBlendMode { .. }
            | PaintCmd::PushFilter { .. }
            | PaintCmd::PushMask { .. } => return None,
            _ => {}
        }
    }
    let tint_index = list.commands[video_index + 1..]
        .iter()
        .take(6)
        .position(|command| matches!(command, PaintCmd::PushBlendMode { mode: 1 }))?
        + video_index
        + 1;
    let PaintCmd::FillRect {
        rect: tint_rect,
        color,
        ..
    } = list.commands.get(tint_index + 1)?
    else {
        return None;
    };
    if !matches!(
        list.commands.get(tint_index + 2),
        Some(PaintCmd::PopBlendMode)
    ) || color.a == 0
        || tint_rect.x > video_rect.x
        || tint_rect.y > video_rect.y
        || tint_rect.right() < video_rect.right()
        || tint_rect.bottom() < video_rect.bottom()
    {
        return None;
    }
    let mut overlay = DisplayList::new();
    overlay.commands.extend(clips);
    overlay
        .commands
        .extend_from_slice(&list.commands[tint_index + 3..]);
    let width = ((page_w * scale).ceil() as u32).max(1);
    let height = ((page_h * scale).ceil() as u32).max(1);
    let mut image = Pixmap::new(width, height)?;
    crate::renderer::display_list_replay::replay_with_text(
        &overlay,
        &mut image,
        scale,
        &mut renderer.font_system,
        &mut renderer.swash_cache,
    );
    Some(NativeVideoComposition {
        tab,
        video_id,
        page_w,
        page_h,
        scale,
        tint: [color.r, color.g, color.b, color.a],
        foreground: VideoForeground {
            rgba: Arc::new(image.data().to_vec()),
            source_width: width,
            source_height: height,
            x: 0.0,
            y: chrome_height,
            width: page_w,
            height: page_h,
        },
    })
}
