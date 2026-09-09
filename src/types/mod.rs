//! The data model: CSS values and computed style, the render-tree node,
//! the layout geometry, the `Document` and the input handling over it.
//!
//! ⛔ DECLARES and RE-EXPORTS. This was one 6,584-line file named for
//! none of the several concerns inside it. Call sites say
//! `crate::types::X`, so the glob re-exports keep one path to each item.

use crate::dom::arena::DomArena;
use std::sync::Arc;

pub mod animation_helpers;
pub mod animation_runtime;
pub mod animation_types;
pub mod announcement;
pub mod bidi;
pub mod canvas_ctx;
pub mod color;
pub mod components;
pub mod computed_style;
pub mod css_enums;
pub mod css_types_extra;
pub mod css_value;
pub mod document;
pub mod document_core;
pub mod document_parts;
pub mod document_style;
pub mod filter;
pub mod flex;
pub mod focus;
pub mod focus_nodes;
pub mod form_runtime;
pub mod geometry;
pub mod grid;
pub mod inline_run;
pub mod input_key;
pub mod input_mouse;
pub mod input_scroll;
pub mod input_state;
pub mod layout_box;
pub mod layout_line;
pub mod length;
pub mod live_regions;
pub mod live_text;
pub mod logical;
pub mod scrollbar_hit;
pub mod slots;
pub mod transform;
pub mod webcore_node;
pub mod wheel_scroll;

pub use crate::video::*;
pub use animation_helpers::*;
pub use animation_types::*;
pub use announcement::*;
pub use bidi::*;
pub use canvas_ctx::*;
pub use color::*;
pub use components::*;
pub use computed_style::*;
pub use css_enums::*;
pub use css_types_extra::*;
pub use css_value::*;
pub use document::*;
pub use document_parts::*;
pub use filter::*;
pub use flex::*;
pub use focus_nodes::*;
pub use form_runtime::*;
pub use geometry::*;
pub use grid::*;
pub use inline_run::*;
pub use input_state::*;
pub use layout_box::*;
pub use layout_line::*;
pub use length::*;
pub use live_text::*;
pub use logical::*;
pub use scrollbar_hit::*;
pub use slots::*;
pub use transform::*;
pub use webcore_node::*;
pub use wheel_scroll::*;
