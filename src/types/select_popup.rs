//! Shared native select popup geometry and interaction.

use super::{ComputedStyle, Document, Rect, WebCore};

pub(crate) const SELECT_POPUP_PADDING: f32 = 4.0;
const SELECT_POPUP_MIN_WIDTH: f32 = 150.0;
const SELECT_OPTION_LINE_FACTOR: f32 = 1.8;
const SELECT_GROUP_LINE_FACTOR: f32 = 1.5;

pub(crate) struct SelectPopupRow<'a> {
    pub node: &'a WebCore,
    pub index: Option<usize>,
    pub disabled: bool,
    pub top: f32,
    pub height: f32,
}

pub(crate) struct SelectPopup<'a> {
    pub rect: Rect,
    pub font_px: f32,
    pub scroll: f32,
    pub max_scroll: f32,
    pub rows: Vec<SelectPopupRow<'a>>,
}

impl<'a> SelectPopup<'a> {
    pub fn new(node: &'a WebCore, root_font: f32, viewport: Rect, scroll: f32) -> Self {
        let font_px = node
            .style
            .font_size_px(ComputedStyle::INITIAL_FONT_SIZE_PX, root_font);
        let mut rows = Vec::new();
        let mut index = 0;
        let mut top = 0.0;
        fn walk<'a>(
            node: &'a WebCore,
            in_group: bool,
            disabled: bool,
            font: f32,
            index: &mut usize,
            top: &mut f32,
            rows: &mut Vec<SelectPopupRow<'a>>,
        ) {
            for child in &node.children {
                match child.tag.as_str() {
                    "option" => {
                        let height = font * SELECT_OPTION_LINE_FACTOR;
                        rows.push(SelectPopupRow {
                            node: child,
                            index: Some(*index),
                            disabled: crate::html::forms::option_is_disabled(child, disabled),
                            top: *top,
                            height,
                        });
                        *index += 1;
                        *top += height;
                    }
                    "select" | "hr" | "datalist" => {}
                    "optgroup" if !in_group => {
                        let height = font * SELECT_GROUP_LINE_FACTOR;
                        let disabled = child.attributes.contains_key("disabled");
                        rows.push(SelectPopupRow {
                            node: child,
                            index: None,
                            disabled,
                            top: *top,
                            height,
                        });
                        *top += height;
                        walk(child, true, disabled, font, index, top, rows);
                    }
                    "optgroup" => {}
                    _ => walk(child, in_group, disabled, font, index, top, rows),
                }
            }
        }
        walk(node, false, false, font_px, &mut index, &mut top, &mut rows);
        let content_height = top + SELECT_POPUP_PADDING * 2.0;
        let anchor = node.layout.border_rect;
        let below = (viewport.bottom() - anchor.bottom()).max(0.0);
        let above = (anchor.y - viewport.y).max(0.0);
        let open_above = content_height > below && above > below;
        let height = content_height.min(if open_above { above } else { below });
        let width = anchor
            .w
            .max(SELECT_POPUP_MIN_WIDTH)
            .min(viewport.w.max(0.0));
        let x = anchor
            .x
            .clamp(viewport.x, (viewport.right() - width).max(viewport.x));
        let y = if open_above {
            anchor.y - height
        } else {
            anchor.bottom()
        };
        let max_scroll = (content_height - height).max(0.0);
        Self {
            rect: Rect::new(x, y, width, height),
            font_px,
            rows,
            max_scroll,
            scroll: scroll.clamp(0.0, max_scroll),
        }
    }

    pub fn contains(&self, point: (f32, f32)) -> bool {
        point.0 >= self.rect.x
            && point.0 < self.rect.right()
            && point.1 >= self.rect.y
            && point.1 < self.rect.bottom()
    }

    pub fn option_at(&self, point: (f32, f32)) -> Option<&SelectPopupRow<'a>> {
        if !self.contains(point) {
            return None;
        }
        let y = point.1 - self.rect.y - SELECT_POPUP_PADDING + self.scroll;
        self.rows.iter().find(|row| {
            row.index.is_some() && !row.disabled && y >= row.top && y < row.top + row.height
        })
    }
}

impl Document {
    pub(crate) fn reveal_list_box_option(&mut self, select_id: u32, option_id: u32) {
        let Some(select) = self.get_node(select_id) else {
            return;
        };
        let Some(index) = crate::html::forms::option_ids(select)
            .iter()
            .position(|&id| id == option_id)
        else {
            return;
        };
        let initial = crate::types::ComputedStyle::INITIAL_FONT_SIZE_PX;
        let root_font = self.root.style.font_size_px(initial, initial);
        let font_px = select.style.font_size_px(root_font, root_font);
        let row_height = crate::html::forms::list_box_row_height(font_px);
        let top = if index == 0 {
            0.0
        } else {
            crate::html::forms::LIST_BOX_PADDING + index as f32 * row_height
        };
        let old = select.layout.scroll_top;
        let height = select.layout.content_rect.h;
        let next = if top < old {
            top
        } else if top + row_height > old + height {
            top + row_height - height
        } else {
            old
        }
        .clamp(0.0, (select.layout.scroll_height - height).max(0.0));
        if (next - old).abs() > f32::EPSILON {
            self.get_box_by_id_mut(select_id).unwrap().layout.scroll_top = next;
            self.note_scroll_action(select_id);
            let mut event = crate::dom::events::DomEvent::new("scroll", select_id);
            self.dispatch_dom_event(&mut event);
        }
    }

    pub(crate) fn reveal_select_popup_selection(&mut self) {
        let Some(popup) = self.select_popup() else {
            return;
        };
        let selected = self
            .get_node(self.open_select)
            .map(crate::html::forms::selected_index)
            .unwrap_or(-1);
        let Some(row) = popup
            .rows
            .iter()
            .find(|row| row.index.is_some_and(|index| index as i32 == selected))
        else {
            return;
        };
        let visible = (popup.rect.h - SELECT_POPUP_PADDING * 2.0).max(0.0);
        let next = if row.top < popup.scroll {
            row.top
        } else if row.top + row.height > popup.scroll + visible {
            row.top + row.height - visible
        } else {
            popup.scroll
        };
        self.dropdown_scroll = next.clamp(0.0, popup.max_scroll);
    }

    pub(crate) fn select_popup(&self) -> Option<SelectPopup<'_>> {
        if self.open_select == 0 {
            return None;
        }
        let node = self.get_node(self.open_select)?;
        if node.tag != "select" {
            return None;
        }
        let width = if self.viewport_w > 0.0 {
            self.viewport_w
        } else {
            self.root.layout.border_rect.w
        };
        let height = if self.viewport_h > 0.0 {
            self.viewport_h
        } else {
            f32::INFINITY
        };
        Some(SelectPopup::new(
            node,
            self.root_font_px(),
            Rect::new(self.scroll_x, self.scroll_y, width, height),
            self.dropdown_scroll,
        ))
    }

    pub(crate) fn select_popup_pointer(
        &mut self,
        event: crate::dom::HtmlEventType,
        point: (f32, f32),
    ) -> Option<bool> {
        use crate::dom::HtmlEventType;
        if self.open_select == 0 {
            return None;
        }
        let Some(popup) = self.select_popup() else {
            self.open_select = 0;
            return Some(true);
        };
        let hit = popup
            .option_at(point)
            .map(|row| (row.index.unwrap() as i32, row.node.node_id));
        match event {
            HtmlEventType::MouseMove => {
                let next = hit.map_or(-1, |(index, _)| index);
                let changed = next != self.dropdown_hover_idx;
                self.dropdown_hover_idx = next;
                Some(changed)
            }
            HtmlEventType::MouseDown => Some(false),
            HtmlEventType::MouseUp => {
                let id = self.open_select;
                if let Some((_, option)) = hit
                    && let Some(select) = self.find_webcore_mut(id)
                {
                    let changed = crate::html::forms::pick_option(select, option);
                    if changed {
                        select.layout.layout_dirty = true;
                        self.style_dirty = true;
                        self.send_select_update_notifications(id);
                    }
                }
                self.open_select = 0;
                self.dropdown_hover_idx = -1;
                self.dropdown_scroll = 0.0;
                Some(true)
            }
            _ => None,
        }
    }
}
