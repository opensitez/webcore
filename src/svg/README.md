# webcore SVG

SVG support is owned by this module.

The active rendering path is:

1. HTML or image loading supplies SVG source text.
2. `parser.rs` parses that source into `SvgDocument` / `SvgNode`.
3. `geometry.rs`, `source.rs`, and `path.rs` normalize SVG-specific syntax.
4. `paint.rs` rasterizes the parsed native tree through webcore canvas/text
   primitives.

`WebCore` nodes store parsed `SvgDocument` values for SVG content. They do not
store raw SVG markup for render-time fallback.

New SVG behavior should be added here and should reuse existing webcore CSS,
font, image, filter, mask, and canvas primitives where the specs overlap.
