//! Layout Constraints — the core data types for the FormattingContext architecture.
//!
//! `Constraints` bundles the parameters that flow DOWN from parent to child during layout.
//! `IntrinsicSizes` bundles the size information that flows UP from child to parent.
//! `FormattingContext` is the trait every layout mode implements.

use super::{LayoutEngine, ResolvedBox};

/// Size constraints flowing DOWN the tree (parent → child).
///
/// Replaces the previous 5+ parameter signatures on layout functions.
/// In the current engine this carries position (x, y) alongside true constraints;
/// a future refactor will separate positioning from constraint propagation.
#[derive(Clone, Copy, Debug)]
pub struct Constraints {
    /// Available width from containing block
    pub available_width: f32,
    /// Available height from containing block (None = auto).
    /// Used for percentage height resolution (CSS 2.1 §10.5).
    pub available_height: Option<f32>,
    /// Position X in parent's coordinate space
    pub x: f32,
    /// Position Y in parent's coordinate space
    pub y: f32,
    /// Parent's computed font-size in px (for em/ex units)
    pub parent_font_px: f32,
    /// Root element's computed font-size in px (for rem units)
    pub root_font_px: f32,
    /// Forced content width (overrides style.width in resolve_box_vp).
    /// Used by flex layout to set the resolved main/cross size WITHOUT
    /// mutating the DOM style. None = use style.width as normal.
    pub forced_width: Option<f32>,
    /// Forced content height (overrides style.height in resolve_box_vp).
    pub forced_height: Option<f32>,
    /// The caller requires the child to establish an independent formatting
    /// context. Flex items do this, which means their internal floats contribute
    /// to the item's auto cross-size instead of escaping and collapsing it.
    pub force_independent_formatting_context: bool,
}

impl Constraints {
    #[inline]
    pub fn new(
        available_width: f32,
        x: f32,
        y: f32,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> Self {
        Self {
            available_width,
            available_height: None,
            x,
            y,
            parent_font_px,
            root_font_px,
            forced_width: None,
            forced_height: None,
            force_independent_formatting_context: false,
        }
    }

    /// Create constraints with an explicit available height.
    #[inline]
    pub fn with_height(
        available_width: f32,
        available_height: f32,
        x: f32,
        y: f32,
        parent_font_px: f32,
        root_font_px: f32,
    ) -> Self {
        Self {
            available_width,
            available_height: Some(available_height),
            x,
            y,
            parent_font_px,
            root_font_px,
            forced_width: None,
            forced_height: None,
            force_independent_formatting_context: false,
        }
    }

    /// Create constraints with forced content dimensions (for flex items).
    #[inline]
    pub fn with_forced(
        available_width: f32,
        x: f32,
        y: f32,
        parent_font_px: f32,
        root_font_px: f32,
        forced_width: Option<f32>,
        forced_height: Option<f32>,
    ) -> Self {
        Self {
            available_width,
            available_height: None,
            x,
            y,
            parent_font_px,
            root_font_px,
            forced_width,
            forced_height,
            force_independent_formatting_context: false,
        }
    }
}

/// Intrinsic size information flowing UP the tree (child → parent).
///
/// Used by flex, grid, table, and float sizing to query children's natural sizes
/// without performing full layout.
#[derive(Clone, Copy, Debug, Default)]
pub struct IntrinsicSizes {
    /// Minimum content width — the narrowest the element can be without overflow.
    pub min_content: f32,
    /// Maximum content width — the preferred width with no wrapping.
    pub max_content: f32,
}

/// The layout algorithm trait. Every formatting context (BFC, IFC, FFC, GFC, TFC)
/// implements this. Custom components also implement this for app engine mode.
///
/// In Step 1, this trait is defined but not yet used for dispatch — existing
/// free functions (layout_flex, layout_grid, etc.) are the implementations.
/// Step 2+ will migrate to trait-based dispatch.
pub trait FormattingContext {
    /// Compute intrinsic sizes (for parent's sizing algorithm).
    /// MUST be cheap — cached after first call, invalidated on content change.
    fn intrinsic_sizes(&self, engine: &LayoutEngine) -> IntrinsicSizes;

    /// Perform full layout given constraints from parent.
    /// Returns the element's margin-box height.
    fn layout(
        &mut self,
        engine: &LayoutEngine,
        constraints: &Constraints,
        rbox: &ResolvedBox,
    ) -> f32;
}
