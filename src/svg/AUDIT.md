# SVG Implementation Audit

Status: in progress, 2026-10-05. This is not a full-conformance claim.
Reference material: `data/ecma/svgwg/master`, `data/ecma/csswg-drafts`,
and `data/wpt/svg`. Review normative requirements and real browser paths,
not merely element recognition or an empty unsupported-feature report.

## Verified In This Pass

- Linebender's Vello sparkline renders green rather than a black rectangle
  in the release browser. CSS mask declarations, inherited gradient variables,
  and computed fill paint-server references are preserved.
- Rasterization transfers its owned pixel buffer instead of copying it.
- Slashdot's social-icon backing offset was an inline-layout/font-metric
  issue, not an SVG fill error. Non-replaced inline owners now use their own
  font content edges around the baseline, rather than descendant bounds.
  Font metrics query the requested face directly instead of shaping a Latin
  probe that icon fonts lack. Release-browser before/after captures remove
  Facebook's extra white strip below and Mastodon's strip above, while
  preserving the site's intentional white backing. Chrome measurements and
  focused inline-owner/icon-font regressions validate the geometry.
- The broader SVG run passed 287 tests, with two manual benchmarks ignored,
  including CSS, DOM event controls, streaming, display lists and retained
  rasters. The macOS capture regression also passed in this pass.
- Path text now uses the shared Canvas affine glyph renderer: rotation,
  shear, reflection and nonuniform scaling transform glyph outlines rather
  than only their origins. Positive uniform-scale bitmap text retains its
  fast path. Shared Canvas passed 130 tests, including affine fill/stroke,
  alignment, clipping and effects; an SVG tangent regression covers both
  fill and stroke. This does not replace per-character SVG shaping.
- Streaming HTML acknowledges self-closing SVG syntax without treating
  ordinary HTML slashes as self-closing. Unquoted attribute slashes remain
  value data, and SVG integration points return to HTML handling. Tokenizer
  and streaming gates passed 31 tests, display lists 286 and tree construction
  five. The rebuilt release browser shows sharp tangential text and matching
  reference shapes with the original self-closing SVG fixture; the misplaced
  nesting previously leaked a gray stroke into the following text.
- Ordered direct text runs preserve mixed text around element children without
  changing element-only reference indices. Native parsing, inline DOM projection,
  Unicode/entity/CDATA boundaries, text-path collection and foreignObject markup
  share that order. Cache keys include run placement. Nested position counters
  span descendants; direct split runs share their existing textLength adjustment,
  and unpositioned mixed chunks anchor once, including computed DOM alignment.
  The release-browser mixed ABCD fixture matches the uninterrupted reference.
  Current gates passed 287 display-list, 115 render, seven tree-construction,
  17 SVG parser, four SVG tree and 29 streaming tests. Full cluster shaping,
  descendant-wide textLength and all positioned chunk anchoring remain separate
  work. Native callers directly mutating public text/children must maintain or
  clear run metadata; replace_text provides explicit invalidation.
- Element parsing is iterative and depth-limited, including 20,000-level
  closed and incomplete hostile-input regressions. A recursive version of
  the limit still overflowed the test-thread stack and was replaced.
- Painting shares a 65,536-node work budget through cloned state and masks,
  with 64-reference and 256-ancestor depth limits. Exhaustion deterministically
  omits remaining work. Deep acyclic use chains, binary reference expansion,
  mask budget sharing and independent raster resets are tested. Gradient
  attribute lookup is iterative, cycle-aware and bounded; stop-reference
  recursion is depth-limited. These are selected reference-traversal protections,
  not a complete resource-exhaustion guarantee.
- Root attributes escape markup correctly on DOM-to-SVG serialization.
- Scientific notation and absolute lengths reuse the existing CSS parser.
- Motion animation preserves the ordinary transform and supports coordinate
  from/to/by forms, with normal-suite regressions.
- SourceAlpha RGB is black, and morphology defaults to erode.
- Missing/wrong-kind paint-server references without an explicit fallback
  render no paint instead of black. Fill and stroke CSS paint-server values
  survive computed-style serialization and inheritance. Live gradient,
  variable and stroke mutations refresh cached pixels.
- Text and motion share a measured path index that preserves moveto breaks,
  avoids per-curve temporary vectors and uses cumulative-distance search.
- Open text paths omit glyphs whose midpoints fall outside the path rather
  than painting them at a clamped endpoint. Raster regressions cover both
  offset directions and partial overflow. Single closed subpaths now wrap
  only within the anchor/direction-dependent one-circuit visibility window.
  Mixed subpaths do not wrap, and shared motion sampling is unchanged.
- Animation accumulation adds the simple-duration endpoint rather than the
  endpoint-minus-start delta. Nonzero starts, indefinite repeats, repeat/freeze
  boundaries, transform parameters, to/by-only behavior and restart are covered.
- Morphology uses separable monotonic-queue extrema for larger windows and a
  direct scan for tiny windows. Exact-output checks cover valid premultiplied
  channels, clipped edges, asymmetric radii and singleton dimensions. Either
  nonpositive parsed radius disables the entire primitive, as specified.
  Alternating-order optimized 512x512 kernel benchmarks observed roughly
  2-17x lower CPU time at radii 4-12; radius 1 retains the direct scan. These
  are allocation-inclusive kernel comparisons, not browser throughput claims.
- Component-transfer tables preserve scalar output across all byte/alpha
  values and the small-image threshold. A warmed release ABBA benchmark
  passed exact-output checks. At 1024x1024, median whole-filter CPU time was
  21.434 vs 95.841 ms for gamma, 19.924 vs 46.051 ms for linear, and
  19.408 vs 41.834 ms for identity (table vs scalar). These are filter
  measurements, not browser frame-rate claims.

## Correctness And Integration Backlog

| Area | Source evidence / missing behavior | Required regression |
| --- | --- | --- |
| Parser safety | Iterative element parsing rejects nesting beyond 256 elements. Paint/reference traversal is bounded, including gradient inheritance. Parsing breadth, text-specific traversal, retained named filter buffers and foreignObject allocation remain unbounded. | Existing depth/reference gates pass; add allocation budgets, breadth limits and remaining downstream traversal coverage. |
| XML/HTML distinction | Standalone parser tolerates incomplete XML and ignores trailing content; namespace prefixes are not fully resolved. | Strict image-document validation separately from tolerant inline-HTML parsing; prefixed SVG namespace. |
| DOM rebuild | Root escaping and live paint-server/variable mutations are covered; broader mutations remain unverified. | Structural mutation, reference replacement and cache invalidation. |
| Mixed text | Ordered runs now preserve source order and element-only indices through parsing, projection, painting and cache keys. Native public mutation requires metadata maintenance; full whitespace/addressable-character shaping and positioned chunk anchoring remain incomplete. | Existing Unicode/CDATA/order/position/cache regressions pass; expand live structural mutation and shaped-cluster cases. |
| Geometry | Exponents/absolute lengths are covered; CSS geometry and viewport units still need integration. | CSS and attribute geometry, units, percentage viewport changes and DOM mutation. |
| Path metrics | Disconnected subpaths, closure and zero-length segments are covered by the shared measured index. Curve flattening remains fixed-resolution. Stroke dashing ignores pathLength calibration; percentage distances need separate treatment. | Adaptive error bounds, pathLength dash/offset parity and public DOM measurement APIs. |
| Text shaping | Positioned/path text uses individual characters rather than shaped clusters. | Combining marks, ligatures, RTL/contextual scripts and inherited positioning lists. |
| Inline text presentation attributes | Native font attributes are applied, then overwritten by DOM computed fonts because common presentation hints omit SVG font-size/family/weight/style. The live 26-unit fixture renders at the inherited default. | Shared cascade mapping with specificity-zero ordering, CSS overrides, relative inheritance, recascade and HTML/foreignObject namespace boundaries. |
| Text length | `spacingAndGlyphs` changes spacing rather than glyph geometry. | WPT textLength cases, positioned text and shaped runs. |
| Text paths | Single closed-path wrapping and open-path omission are covered. Descendant styles, referenced transforms and side=right path reversal remain incomplete. | Nested tspan styles, transformed references, right-side direction and path mutation. |
| foreignObject | Detached raster HTML loses live DOM/layout interaction and outer CSS. | Cascading, inherited styles, focus, controls, events and DOM mutation. |
| Motion animation | Supplemental transforms and unitless/px coordinate endpoint forms are covered. Relative units and broader DOM integration remain. | Percentage/font-relative coordinates, referenced path transforms and live mutation. |
| Animation composition | Endpoint accumulation and indefinite-repeat counting are covered. Computed underlying values, path morphing and mixed units remain incomplete. | CSS-based underlying values, compatible path commands and unit interpolation. |
| Animation DOM | SVG-root timeline controls and geometry DOM bindings remain unverified. | Public browser bindings, pause/seek/resume and measurement methods. |
| Filters | SourceAlpha/default morphology are covered; linearRGB selection/conversion is absent. | Color-space pixel oracles and multi-primitive color-space changes. |
| Filter completeness | Turbulence and lighting primitives are not represented in the current native tree. | Normative primitive coverage and effect-chain pixel references. |
| Diagnostics | Known ignored attributes are mostly absent from unsupported summaries. | Report unsupported behavior without claiming all recognized elements are complete. |

## Performance Backlog

Changes require exact-output parity and repeatable timings at matched sizes.
Do not infer throughput from less work in a narrow kernel alone.

1. Replace filter-graph eager pixmap clones with shared immutable results,
   lazy SourceAlpha, reusable intermediate buffers and bounded effect regions.
2. Morphology sliding extrema and the tiny-window direct path are implemented.
   Preserve exact output and expand benchmark coverage beyond the current
   randomized 512x512 fixture before broader throughput claims.
3. Component-transfer tables are implemented and measured. Preserve exact
   scalar rounding before considering SIMD lookup/premultiplication.
4. Add mask identity/zero fast paths and bulk kernels with scalar parity,
   including unaligned tails, transparent pixels and rounding boundaries.
5. Analyze static SVGs and document-ID subtrees once per rebuild rather than
   repeatedly traversing overlapping subtrees. Mutable-tree invalidation is
   mandatory; pointer identity alone is not a valid cross-frame cache key.
6. Replace computed-style Debug formatting in cache fingerprints with explicit
   hashing while retaining every pixel-affecting input.
7. Text/motion now share cumulative-distance indexes without curve temporary
   vectors or fictitious subpath links. Parsed geometry still needs reusable,
   mutation-safe caching and adaptive error bounds.
8. Exercise both software and accelerated presentation, 1x/2x device scale,
   transformed/offscreen SVGs and dynamic mutations. Cache hits must remain
   correct after CSS, DOM, references, viewport and animation changes.

## Completion Gate

Each area needs a specification-linked inventory, implementation evidence,
automated positive/negative cases, and actual browser integration coverage.
Run SVG/CSS/DOM/animation WPT slices against references, plus the existing unit
and integration suites. Test hostile input and resource limits separately.
Until these gates cover the inventory, full browser SVG support is unproven.
