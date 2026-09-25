//! Tiled Rendering — only rasterize visible tiles + buffer zone.
//!
//! Instead of rendering the entire page into one surface, we divide it into
//! tiles (512x512 logical pixels). Only tiles intersecting the viewport
//! (+ a buffer zone) are rasterized. Cached tiles are reused during scroll.
//!
//! This makes scroll instant: we just composite pre-rasterized tiles at
//! new offsets instead of re-rendering the entire page.

use crate::types::Rect;
use std::collections::HashMap;

/// Tile size in logical pixels.
pub const TILE_SIZE: f32 = 512.0;

/// A single rasterized tile.
pub struct RasterTile {
    /// Tile grid coordinates.
    pub tile_x: i32,
    pub tile_y: i32,
    /// Rasterized pixel data (RGBA, pre-multiplied alpha).
    pub pixmap: tiny_skia::Pixmap,
    /// Whether this tile needs re-rasterization.
    pub dirty: bool,
}

/// Manages the tile grid — decides which tiles exist, which are dirty,
/// and evicts distant tiles to save memory.
pub struct TileManager {
    /// Active tiles, keyed by (tile_x, tile_y).
    pub tiles: HashMap<(i32, i32), RasterTile>,
    /// Current viewport in document coordinates.
    pub viewport: Rect,
    /// Scale factor (for HiDPI).
    pub scale: f32,
    /// Total document size (for tile range clamping).
    pub doc_width: f32,
    pub doc_height: f32,
    /// Generation counter — incremented on layout change.
    pub generation: u64,
}

impl TileManager {
    pub fn new() -> Self {
        Self {
            tiles: HashMap::new(),
            viewport: Rect::default(),
            scale: 1.0,
            doc_width: 0.0,
            doc_height: 0.0,
            generation: 0,
        }
    }

    /// Update viewport position (called on scroll).
    /// Returns the set of tile coordinates that need rasterization.
    pub fn update_viewport(&mut self, viewport: Rect, scale: f32) -> Vec<(i32, i32)> {
        self.viewport = viewport;
        self.scale = scale;
        self.needed_tiles()
    }

    /// Get tiles intersecting the viewport. Offscreen work must not hold up a
    /// visible frame; prefetch can be scheduled separately when idle.
    fn needed_tiles(&self) -> Vec<(i32, i32)> {
        if self.viewport.w <= 0.0 || self.viewport.h <= 0.0 {
            return Vec::new();
        }
        let min_tx = (self.viewport.x / TILE_SIZE).floor() as i32;
        let max_tx = ((self.viewport.x + self.viewport.w - 0.001) / TILE_SIZE).floor() as i32;
        let min_ty = (self.viewport.y / TILE_SIZE).floor() as i32;
        let max_ty = ((self.viewport.y + self.viewport.h - 0.001) / TILE_SIZE).floor() as i32;

        // Clamp to document bounds
        let doc_max_tx = ((self.doc_width - 0.001).max(0.0) / TILE_SIZE).floor() as i32;
        let doc_max_ty = ((self.doc_height - 0.001).max(0.0) / TILE_SIZE).floor() as i32;

        let mut needed = Vec::new();
        for ty in min_ty.max(0)..=max_ty.min(doc_max_ty) {
            for tx in min_tx.max(0)..=max_tx.min(doc_max_tx) {
                needed.push((tx, ty));
            }
        }
        needed
    }

    /// Ensure a tile exists, creating it if needed. Returns whether it needs rasterization.
    pub fn ensure_tile(&mut self, tx: i32, ty: i32) -> bool {
        let phys_size = (TILE_SIZE * self.scale).ceil() as u32;
        let tile = self.tiles.entry((tx, ty)).or_insert_with(|| {
            RasterTile {
                tile_x: tx,
                tile_y: ty,
                pixmap: tiny_skia::Pixmap::new(phys_size.max(1), phys_size.max(1))
                    .unwrap_or_else(|| tiny_skia::Pixmap::new(1, 1).unwrap()),
                dirty: true,
            }
        });
        if tile.pixmap.width() != phys_size || tile.pixmap.height() != phys_size {
            tile.pixmap = tiny_skia::Pixmap::new(phys_size.max(1), phys_size.max(1))
                .unwrap_or_else(|| tiny_skia::Pixmap::new(1, 1).unwrap());
            tile.dirty = true;
        }
        tile.dirty
    }

    /// Mark a tile as clean after rasterization.
    pub fn mark_clean(&mut self, tx: i32, ty: i32) {
        if let Some(tile) = self.tiles.get_mut(&(tx, ty)) {
            tile.dirty = false;
        }
    }

    /// Mark all tiles as dirty (after layout change).
    pub fn invalidate_all(&mut self) {
        for tile in self.tiles.values_mut() {
            tile.dirty = true;
        }
        self.generation += 1;
    }

    /// Mark tiles that intersect a dirty rect as needing re-rasterization.
    pub fn invalidate_rect(&mut self, rect: &Rect) {
        if rect.w <= 0.0 || rect.h <= 0.0 {
            return;
        }
        let min_tx = (rect.x / TILE_SIZE).floor() as i32;
        let max_tx = (((rect.x + rect.w).ceil() - 1.0) / TILE_SIZE).floor() as i32;
        let min_ty = (rect.y / TILE_SIZE).floor() as i32;
        let max_ty = (((rect.y + rect.h).ceil() - 1.0) / TILE_SIZE).floor() as i32;

        for ty in min_ty..=max_ty {
            for tx in min_tx..=max_tx {
                if let Some(tile) = self.tiles.get_mut(&(tx, ty)) {
                    tile.dirty = true;
                }
            }
        }
    }

    /// Evict tiles that are far from the viewport to save memory.
    pub fn evict_distant(&mut self) {
        let center_x = self.viewport.x + self.viewport.w / 2.0;
        let center_y = self.viewport.y + self.viewport.h / 2.0;
        let max_dist = (self.viewport.w + self.viewport.h) * 2.0; // 2x viewport diagonal

        self.tiles.retain(|&(tx, ty), _| {
            let tile_cx = tx as f32 * TILE_SIZE + TILE_SIZE / 2.0;
            let tile_cy = ty as f32 * TILE_SIZE + TILE_SIZE / 2.0;
            let dist = ((tile_cx - center_x).powi(2) + (tile_cy - center_y).powi(2)).sqrt();
            dist < max_dist
        });
    }

    /// Composite visible tiles onto the output pixmap.
    pub fn composite_over(
        &self,
        output: &mut tiny_skia::Pixmap,
        scroll_x: f32,
        scroll_y: f32,
        scale: f32,
    ) {
        for ((tx, ty), tile) in &self.tiles {
            if tile.dirty {
                continue;
            }
            let x = (*tx as f32 * TILE_SIZE - scroll_x) * scale;
            let y = (*ty as f32 * TILE_SIZE - scroll_y) * scale;
            if x + tile.pixmap.width() as f32 <= 0.0
                || y + tile.pixmap.height() as f32 <= 0.0
                || x >= output.width() as f32
                || y >= output.height() as f32
            {
                continue;
            }
            output.draw_pixmap(
                0,
                0,
                tile.pixmap.as_ref(),
                &tiny_skia::PixmapPaint::default(),
                tiny_skia::Transform::from_translate(x.round(), y.round()),
                None,
            );
        }
    }

    /// Composite visible tiles onto the output pixmap.
    /// This is the fast path — just copy pre-rasterized tiles at the right offset.
    pub fn composite_to(
        &self,
        output: &mut tiny_skia::Pixmap,
        scroll_x: f32,
        scroll_y: f32,
        scale: f32,
    ) {
        let out_w = output.width() as i32;
        let out_h = output.height() as i32;
        let phys_tile = (TILE_SIZE * scale).ceil() as i32;

        for ((tx, ty), tile) in &self.tiles {
            if tile.dirty {
                continue;
            } // skip unrasterized tiles

            // Tile position in physical pixels, adjusted for scroll
            let px = (*tx as f32 * TILE_SIZE - scroll_x) * scale;
            let py = (*ty as f32 * TILE_SIZE - scroll_y) * scale;
            let ipx = px.round() as i32;
            let ipy = py.round() as i32;

            // Skip if entirely off-screen
            if ipx + phys_tile <= 0 || ipy + phys_tile <= 0 || ipx >= out_w || ipy >= out_h {
                continue;
            }

            // Copy tile pixels to output
            let tile_w = tile.pixmap.width() as i32;
            let tile_h = tile.pixmap.height() as i32;
            let src_data = tile.pixmap.data();
            let dst_data = output.data_mut();

            let src_x0 = if ipx < 0 { -ipx } else { 0 };
            let src_y0 = if ipy < 0 { -ipy } else { 0 };
            let dst_x0 = ipx.max(0);
            let dst_y0 = ipy.max(0);
            let copy_w = (tile_w - src_x0).min(out_w - dst_x0);
            let copy_h = (tile_h - src_y0).min(out_h - dst_y0);

            if copy_w <= 0 || copy_h <= 0 {
                continue;
            }

            for row in 0..copy_h {
                let src_offset = ((src_y0 + row) * tile_w + src_x0) as usize * 4;
                let dst_offset = ((dst_y0 + row) * out_w as i32 + dst_x0) as usize * 4;
                let len = copy_w as usize * 4;
                if src_offset + len <= src_data.len() && dst_offset + len <= dst_data.len() {
                    dst_data[dst_offset..dst_offset + len]
                        .copy_from_slice(&src_data[src_offset..src_offset + len]);
                }
            }
        }
    }

    /// Number of cached tiles.
    pub fn tile_count(&self) -> usize {
        self.tiles.len()
    }

    /// Number of dirty tiles.
    pub fn dirty_count(&self) -> usize {
        self.tiles.values().filter(|t| t.dirty).count()
    }
}

impl Default for TileManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_manager_basic() {
        let mut tm = TileManager::new();
        tm.doc_width = 2000.0;
        tm.doc_height = 5000.0;
        let needed = tm.update_viewport(Rect::new(0.0, 0.0, 800.0, 600.0), 1.0);
        assert_eq!(needed, vec![(0, 0), (1, 0), (0, 1), (1, 1)]);
    }

    #[test]
    fn viewport_edge_does_not_rasterize_an_offscreen_tile() {
        let mut tm = TileManager::new();
        tm.doc_width = 2048.0;
        tm.doc_height = 2048.0;
        assert_eq!(
            tm.update_viewport(Rect::new(0.0, 0.0, 512.0, 512.0), 1.0),
            vec![(0, 0)]
        );
        assert_eq!(
            tm.update_viewport(Rect::new(0.0, 500.0, 512.0, 512.0), 1.0),
            vec![(0, 0), (0, 1)]
        );
    }

    #[test]
    fn tile_invalidation() {
        let mut tm = TileManager::new();
        tm.doc_width = 2000.0;
        tm.doc_height = 2000.0;
        tm.ensure_tile(0, 0);
        tm.mark_clean(0, 0);
        assert!(!tm.tiles[&(0, 0)].dirty);

        tm.invalidate_rect(&Rect::new(100.0, 100.0, 50.0, 50.0));
        assert!(tm.tiles[&(0, 0)].dirty);
    }

    #[test]
    fn tile_eviction() {
        let mut tm = TileManager::new();
        tm.doc_width = 10000.0;
        tm.doc_height = 10000.0;
        tm.viewport = Rect::new(0.0, 0.0, 800.0, 600.0);

        // Create tiles far away
        tm.ensure_tile(0, 0);
        tm.ensure_tile(15, 15); // very far from viewport
        assert_eq!(tm.tile_count(), 2);

        tm.evict_distant();
        assert!(tm.tile_count() < 2, "distant tile should be evicted");
    }

    #[test]
    fn cached_tile_reallocates_when_scale_changes() {
        let mut tiles = TileManager::new();
        tiles.update_viewport(Rect::new(0.0, 0.0, 512.0, 512.0), 1.0);
        assert!(tiles.ensure_tile(0, 0));
        tiles.mark_clean(0, 0);
        assert!(!tiles.ensure_tile(0, 0));

        tiles.update_viewport(Rect::new(0.0, 0.0, 512.0, 512.0), 2.0);
        assert!(tiles.ensure_tile(0, 0));
        assert_eq!(tiles.tiles[&(0, 0)].pixmap.width(), 1024);
    }

    #[test]
    fn paint_invalidation_only_dirties_intersecting_tiles() {
        let mut tiles = TileManager::new();
        tiles.update_viewport(Rect::new(0.0, 0.0, 1024.0, 512.0), 1.0);
        tiles.ensure_tile(0, 0);
        tiles.ensure_tile(1, 0);
        tiles.mark_clean(0, 0);
        tiles.mark_clean(1, 0);
        tiles.invalidate_rect(&Rect::new(20.0, 20.0, 30.0, 30.0));
        assert!(tiles.tiles[&(0, 0)].dirty);
        assert!(!tiles.tiles[&(1, 0)].dirty);
    }
}
