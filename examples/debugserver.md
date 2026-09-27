# Phoenix Browser — Debug & Inspector Guide

## Quick Start

```bash
# GUI browser with debug port
cargo run --release --example browser -- --debug-port 9222 https://example.com

# Headless mode (no window, for CI/scripting)
cargo run --release --example browser -- --headless https://example.com

# With Chrome comparison (JavaScript disabled in the separate Chrome instance)
cargo run --release --example browser -- --debug-port 9222 --chrome https://example.com

# Opt-in loading and rendering profile in the GUI
cargo run --release --example browser -- --cached --profile --debug-port 9222 https://example.com
```

## Three Ways to Inspect

### 1. Web Inspector (open in Chrome)
Open `http://127.0.0.1:9222/` — visual dark-themed inspector with:
- **DOM tree** — auto-loaded, expand/collapse nodes by clicking ▶
- **6 tabs** when you click an element: Box Model, Computed, DOM, Layout, Attrs, Styles
- **Console** — type raw JSON commands at the bottom
- **Find** — CSS selector search
- **Screenshot** button

### 2. F12 Built-in Inspector (GUI mode only)
Press F12 in the browser window:
- **Styles** — matched CSS rules with specificity, overridden properties struck through
- **Computed** — all computed CSS values + children summary
- **Box Model** — Chrome-style nested margin/border/padding/content
- **DOM** — ancestor chain + children tree
- **Layout** — geometry rects, resolved values, scroll, line cache, dirty flags
- **Attrs** — all HTML attributes, custom data, inline style

Click elements to inspect. Drag the panel divider to resize.

### 3. Python Client (scripting & automation)
```bash
# Interactive REPL
python3 examples/debugclient.py 9222

# One-shot command
python3 examples/debugclient.py send 9222 '{"cmd":"find","selector":"h1"}'
```

Library usage:
```python
from debugclient import DebugClient
c = DebugClient(9222)
c.find('h1')
c.screenshot('/tmp/page.png')
c.computed('.sidebar')
c.close()
```

## CLI Options

```
cargo run --release --example browser -- [OPTIONS] [URL]

  --debug-port <n>   Enable debug server (default: 9222 in headless)
  --headless         No window — debug server only
  --chrome           Launch Chrome side-by-side for comparison
  --chrome-port <n>  Chrome CDP port (default: 9223)
  --width <px>       Viewport width (default: 1280)
  --height <px>      Viewport height (default: 900)
  --cached           Use snapshot_cache/ for fetched resources (default)
  --cache-dir <dir>  Custom cache directory
  --no-images        Skip image loading
  --profile          Collect phase/resource timings and print a summary every 5s
```

## Command Reference

All commands are JSON: `{"cmd":"name", ...}`. Responses include `"cmd_ms"` timing.

### Navigation
| Command | Example |
|---------|---------|
| `screenshot` | `{"cmd":"screenshot","out":"/tmp/page.png"}` repaints the browser content; add `"scale":2` for HiDPI. In GUI mode, `"presented":true` saves the last presented full-window frame without repainting, useful for transient stale-surface bugs. |
| `navigate` | `{"cmd":"navigate","url":"https://example.com"}` |
| `tabs` | `{"cmd":"tabs"}` — list open demo-browser tabs |
| `switch-tab` | `{"cmd":"switch-tab","index":0}` — switch the active demo-browser tab |
| `scroll` | `{"cmd":"scroll","dy":200}` |
| `resize` | `{"cmd":"resize","width":800,"height":600}` |
| `viewport` | `{"cmd":"viewport"}` — returns width, height, scroll, doc_height |
| `quit` | `{"cmd":"quit"}` — stop the debugged browser process |

`computed` element records also expose `scroll.left`, `scroll.top`,
`scroll.width` and `scroll.height` in CSS pixels. For an overflow container,
use `hover` to place the pointer over its content, then `scroll` with `dy` to
exercise wheel routing into that container; `viewport.scroll_y` can remain zero
while the element's `scroll.top` changes. The `scroll_paint` profiler includes
these nested wheel updates.

### Finding & Querying
| Command | Example |
|---------|---------|
| `find` | `{"cmd":"find","selector":"h1"}` — returns `node_id` plus geometry so follow-up commands can target the same node |
| `text` | `{"cmd":"text","selector":"h1"}` |
| `attr` | `{"cmd":"attr","selector":"a","name":"href"}` |
| `html` | `{"cmd":"html","selector":"main"}` — serialized HTML for a matched element |
| `dom-html` | `{"cmd":"dom-html"}` — serialized current document HTML |
| `search` | `{"cmd":"search","query":"hello"}` — search by text content |
| `hit` | `{"cmd":"hit","x":640,"y":200}` — hit test at coordinates |
| `dom-path` | `{"cmd":"dom-path","selector":"h1"}` — CSS selector path |
| `parent` | `{"cmd":"parent","selector":"td"}` — ancestor chain |

### Inspection
| Command | Example |
|---------|---------|
| `inspect` | `{"cmd":"inspect","selector":".sidebar"}` |
| `inspect-node` | `{"cmd":"inspect-node","nid":42}` — by node_id; image nodes include decoded `image` metadata (`src`, natural width/height, byte count) when available |
| `deep` | `{"cmd":"deep","selector":"td"}` — full dump |
| `computed` | `{"cmd":"computed","selector":"h1"}` — includes `node_id`, box geometry, display/flex fields, grid line placement and order, `direction`, `writing_mode`, resolved padding/margins |
| `css` | `{"cmd":"css","selector":"td","props":"display,width"}` — also supports debug fields such as `svg-path`, `svg-fill-stored`, `svg-stroke-stored`, `svg-paint-flags`, and `matched-rule-count` |
| `resolve-css` | `{"cmd":"resolve-css","value":"var(--brand)"}` — resolve CSS variable references against the active stylesheet |
| `stylesheet-vars` | `{"cmd":"stylesheet-vars","query":"--brand","limit":20}` — list active stylesheet custom properties after viewport-aware resolution |
| `rules` | `{"cmd":"rules","selector":"h1"}` — matched CSS rules |
| `inspect-mode` | `{"cmd":"inspect-mode","on":"true"}` — recascade with matched-rule capture enabled for scripted `rules` inspection; GUI mode preserves the pre-panel page viewport so responsive media queries do not change while the inspector opens |
| `rule-search` | `{"cmd":"rule-search","query":"lg\\:flex","limit":10}` — search loaded stylesheet selectors while debugging cascade misses |
| `style-debug` | `{"cmd":"style-debug","selector":".box","query":"box"}` — returns `node_id`, selector-index candidates, and filtered candidates for cascade debugging |
| `match-debug` | `{"cmd":"match-debug","selector":".box","limit":3}` — runs the real cascade matcher against live nodes and reports candidate/matched rule indices and selectors |
| `node-id-stats` | `{"cmd":"node-id-stats"}` — counts duplicate `node_id` values in the current DOM/render tree |
| `keyframes` | `{"cmd":"keyframes","query":"ticker","limit":10}` — inspect parsed `@keyframes` stops and properties for animation debugging |
| `lines` | `{"cmd":"lines","selector":"p"}` — line-cache geometry for matched elements, including bidi visual segments (`x`, `w`, `level`) for RTL/LTR paint debugging |
| `paint-dump` | `{"cmd":"paint-dump","x":0,"y":0,"w":400,"h":200,"limit":80}` — display-list commands in a viewport rectangle; text entries include font metrics and decoration flags, fill rectangles include color/radius, borders include per-side widths/colors/styles, image entries include intrinsic size, byte count, nontransparent pixel count, content bounds, and average RGB, and CSS mask entries include mask image dimensions |
| `display-list-stats` | `{"cmd":"display-list-stats"}` — command counts for the current viewport paint-band display list, including text, image, clip, transform, layer, and mask commands plus `paint_top`/`paint_bottom` |
| `image-states` | `{"cmd":"image-states","limit":80}` — DOM image/background/mask state, including source URLs, `srcset`, node IDs, element/background/mask decoded state, natural sizes, byte counts, layout rects, pending-channel/in-flight status, and load errors |
| `resource-states` | `{"cmd":"resource-states"}` — loading flag plus pending CSS/image/font resource state, stylesheet counts, and document height |
| `font-faces` | `{"cmd":"font-faces"}` — parsed `@font-face` families and sources, with exact-family availability in the renderer's font database |
| `stylesheet-slots` | `{"cmd":"stylesheet-slots"}` — document stylesheet order, linked/inline slots, resolved stylesheet URLs, and per-slot loaded rule counts for debugging cascade/resource ordering |
| `memory-stats` | `{"cmd":"memory-stats"}` — browser-owned memory accounting plus OS process RSS/VSZ; includes retained viewport/content surfaces, tile surfaces, display-list command count with inline/heap/text/image/vector byte estimates, raw resource cache, parsed CSS cache, decoded image cache, DOM node/image/style counts, estimated DOM/layout/computed-style/line-cache/matched-rule/stylesheet bytes, and unique decoded DOM image buffers |
| `animated-images` | `{"cmd":"animated-images"}` — list animated image nodes, frame counts, layout rects, clip band, and whether the engine currently considers them visible |
| `animations` | `{"cmd":"animations"}` — list active CSS keyframe animations, their target node IDs, duration, iteration count, and animated property names |
| `svg-metrics` | `{"cmd":"svg-metrics"}` — counts parsed SVG documents plus unsupported native SVG elements/attributes seen on the current page |
| `box-model` | `{"cmd":"box-model","selector":"div"}` — Chrome-style |
| `highlight` | `{"cmd":"highlight","selector":"h1","out":"/tmp/hl.png"}`; accepts `"scale":2` for HiDPI output |
| `dom-tree` | `{"cmd":"dom-tree","depth":2}` — structured JSON tree |
| `tree` | `{"cmd":"tree","depth":2}` — compact DOM/layout tree alias used by quick probes |
| `height-dump` | `{"cmd":"height-dump","limit":40}` — largest layout boxes by height for scroll/document-height debugging |
| `a11y` | `{"cmd":"a11y"}` — accessibility tree |

### Interaction
| Command | Example |
|---------|---------|
| `click` | `{"cmd":"click","selector":"#btn"}` or `{"cmd":"click","x":100,"y":200}` |
| `hover` | `{"cmd":"hover","selector":".menu"}` |
| `type` | `{"cmd":"type","text":"hello"}` |
| `key` | `{"cmd":"key","key":"Enter"}` |
| `force-state` | `{"cmd":"force-state","selector":".item","state":"hover"}` |
| `event-listeners` | `{"cmd":"event-listeners","selector":"button"}` — event listener summary for the matched node |
| `media` | `{"cmd":"media","selector":"video","action":"state"}`; actions: `play`, `pause`, `toggle`, `load`, `seek` with `"time":12.5`, `volume`/`muted`/`rate` with `"value"` |

### Mutation
| Command | Example |
|---------|---------|
| `setstyle` | `{"cmd":"setstyle","selector":"h1","prop":"color","value":"red"}` |
| `setattr` | `{"cmd":"setattr","selector":"img","name":"src","value":"new.png"}` |
| `set-text` | `{"cmd":"set-text","selector":"h1","text":"New Title"}` |
| `add-class` | `{"cmd":"add-class","selector":"body","class":"dark"}` |
| `remove-class` | `{"cmd":"remove-class","selector":"body","class":"dark"}` |
| `toggle-class` | `{"cmd":"toggle-class","selector":"body","class":"dark"}` |

### Performance
| Command | Example |
|---------|---------|
| `perf` | `{"cmd":"perf"}` — current browser-view load state for the active tab |
| `profile` | `{"cmd":"profile","limit":20}` — cumulative phase count/total/max timings and slowest resources; add `"reset":true` to start a new measurement window |
| `bench` | `{"cmd":"bench","n":5}` — cascade/layout benchmark |
| `bench-progressive` | `{"cmd":"bench-progressive"}` — above-fold vs full |
| `bench-render` | `{"cmd":"bench-render","dy":500}` — live BrowserView paint/scroll/cached-paint benchmark in the GUI path |
| `network` | `{"cmd":"network"}` — resource count |
| `measure` | `{"cmd":"measure","from":"#a","to":"#b"}` — distance |
| `step` / `tick` | `{"cmd":"tick","ms":100}` — advance debug timing/state in scripted sessions |

Environment flags for frame diagnostics:

```bash
WEBCORE_TRACE_IDLE=1 cargo run --release --example browser -- --cached https://example.com
WEBCORE_TRACE_RENDER=1 cargo run --release --example browser -- --cached https://example.com
```

`WEBCORE_TRACE_IDLE` reports why the engine requested work (`stylesheet`, `image-layout`, `image-paint`, `font`, `css-animation-paint`, `css-animation-layout`, `animated-image`, etc.) and whether more timed work is pending. `WEBCORE_TRACE_RENDER` splits each render into display-list build and replay time, reports whether the renderer-owned backing surface was reused, and counts paint segments retained across a display-list rebuild.

`--profile` is disabled by default. It records timing across loader and rendering threads, prints a cumulative console summary every five seconds while the GUI draws, and enables the `profile` debug command:

```bash
cargo run --release --example browser -- --cached --profile --debug-port 9222 http://localhost/websites/foxnews.html
python3 examples/debugclient.py send 9222 '{"cmd":"profile","limit":10}'
python3 examples/debugclient.py send 9222 '{"cmd":"profile","reset":true}'
```

The profile reports HTML load and streamed parse, CSS load and parse, image load and decode, resource polling, cascade, geometry, frame updates, display-list recording, tile raster/composite, direct replay, full render, demo-browser draw, and `scroll_paint`. Each phase has `count`, cumulative `total_ms`, and slowest `max_ms`. `scroll_paint` measures from the first wheel, scrollbar, or programmatic scroll change awaiting a frame through completion of the next viewport paint; it includes scheduling delay but not OS compositor presentation. Divide `total_ms` by `count` for the mean scroll latency, and use `max_ms` to spot freezes. Resource entries are the 128 slowest observed document, stylesheet, and image loads, with URL, bytes where known, source, and elapsed time. `network` means an actual HTTP request; `load` includes cache/local/document delivery; `network+consume` includes streaming CSS callbacks and parsing. Phase timers are inclusive and can overlap, so do not add their totals to infer page wall time. The profiler resets on navigation; `"reset":true` also clears the current window without navigating. Use a release GUI run for realistic scroll timings.

`image_decode` also includes background expansion of animated GIF/WebP frames,
not just the initial preview. That work runs on image workers; its elapsed time
must not be interpreted as UI blocking time. Compare it with `frame_update`
and the responsiveness of the presented GUI. For HiDPI paint bugs, capture
`screenshot` with `presented:true`: the default 1x debug repaint can hide a
device-scale clipping error that is visible in the actual window.

Viewport rendering uses cached raster tiles by default. Set `WEBCORE_DISABLE_TILES=1` when comparing against the full-display-list replay path during scroll debugging.
The `fixed_replay` and `content_cache` phases break out fixed-position layer painting and retained-surface copying from `render`.
`raster_image`, `raster_text`, `raster_shadow`, and `raster_layer` break down
executed paint commands inside tile/direct replay. Culled commands are excluded;
`raster_layer` measures opacity/filter/blend/mask allocation and compositing;
`raster_clip` measures clip-mask construction, intersection and stack cleanup.
These nested timings must not be added to their enclosing raster/render totals.
Command timing is disabled unless `--profile` is enabled.
When comparing performance runs, also record `viewport`: its `scale` is page
zoom, while GUI `device_scale` is the display's physical-pixel scale. A 2x
display paints four times as many pixels at the same CSS viewport size.

### Chrome Comparison (requires `--chrome`)

#### Whole-page or subtree value comparison

Run one GUI pair on the page, then compare their current live state:

```sh
./target/release/examples/browser --cached --chrome --debug-port 9222 --chrome-port 9223 http://localhost/websites/telquel.html
python3 examples/debugclient.py compare 9222 --out /tmp/page-differences.json
python3 examples/debugclient.py compare 9222 --selector '.article-important' --out /tmp/section-differences.json
python3 examples/debugclient.py compare 9222 --only missing --limit 10 --out /tmp/missing-elements.json
python3 examples/debugclient.py compare 9222 --only visibility
python3 examples/debugclient.py compare 9222 --sync-viewport --out /tmp/aligned-differences.json
```

The client compares the **whole light DOM by default**, including offscreen
elements. A selector includes every matching element and its descendants.
`--tolerance 1` sets geometry tolerance in CSS pixels; `--limit 20` only limits
console output, never the complete JSON file. Repeat after hover or scroll-back
to compare those states. The command is read-only: it does not navigate, change
styles, synchronize scroll, or wait for a presumed final rendering state.
The explicit `--sync-viewport` option is the exception: it sets Chrome's emulated
CSS viewport to Webcore's width/height (desktop, DPR 1) and synchronizes scroll
before capturing Chrome. It does not reload or modify the page DOM. This avoids
browser-toolbar height differences producing responsive-layout noise.

Reports include both property values, Webcore node IDs, full element paths,
matched/unmatched counts, property frequency counts, and environment warnings.
Position differences with the same displacement as a matched parent are annotated
`same_offset_as_parent` and excluded from the independent-difference count when
they are the element's only differences. They remain in the full report.
Matching reserves unique IDs first, then unique tag/direct-text/key-attribute
signatures, globally unique class/container signatures, paths relative to
unique-ID ancestors, paths below already matched parents, and DOM paths (not anonymous
layout wrappers). Content signatures tolerate inserted siblings; ambiguous
repeated content is not guessed. Class token order does not affect identity.
Unmatched means **no confident match**, not proof that the parser dropped a node.
Parser differences, different resources or DOM mutations can also cause it.
Ancestors are listed before descendants to
help locate upstream causes. `style-or-cascade` and `layout-or-resource` are
triage hints, not proven root causes. Follow up using `inspect-node`, `rules`,
`css`, and `chrome-eval` on the reported element.

The `triage` list groups unmatched descendants under their first unmatched
ancestor (with a descendant count), suppresses pure inherited offsets, and
prioritizes unmatched/visibility issues before style and layout differences.
Non-rendering metadata elements are lower priority in that list.
The complete `differences` list remains intact. `--only missing|visibility|styles|layout`
filters console output, not the saved file. Element descriptions include tag,
ID/classes, direct-text excerpts (160 Unicode characters), and selected attributes
(`href`, `src`, `alt`, `aria-label`, `role`, `name`, `type`). Reports can contain
page text and URLs; treat them like other page-debugging artifacts.

`visibility-mismatch` means matched elements differ in suppression by
`display:none`, zero opacity on an ancestor, or computed visibility; it is not
a hit-test, clipping or pixel-visibility verdict. Image decode differences carry
both resource URLs/dimensions so a failed reference image is distinguishable
from a Webcore decoder failure. Changes to text and selected attributes on
confidently matched nodes are reported separately from geometry.
Webcore currently exposes visibility as a boolean, losing the distinction between
`hidden` and `collapse`. Comparison checks visible/non-visible and emits a coverage
warning for Chrome `collapse` elements, rather than falsely claiming to verify
their collapse-specific layout behavior.

Compared values include border-box geometry, resolved margins/padding, display,
position, float, font size/weight/family, direction, writing mode, text alignment,
overflow, flex alignment, opacity, colors and image decode availability. Geometry
is omitted for Chrome elements without boxes or under transforms because Chrome
client rectangles are transformed while Webcore layout rectangles are not.
Authored auto/percentage sizes are not compared directly against Chrome's used
pixel sizes. Viewport/scroll differences, loading state, font readiness, empty
selections and different URLs are explicitly reported. A snapshot is diagnostic,
not an atomic cross-process capture or a conformance verdict.

Current limitations: no shadow-tree, pseudo-element, text-run/line-wrap, clipping,
or pixel comparison; property coverage is the explicit set above, not every CSS
property. Missing resources in Chrome can affect results. Resource/style changes
between the two snapshots can also produce transient differences. The existing
screenshot comparison remains separate.

`compare-snapshot` is the GUI server's bulk Webcore snapshot endpoint:
`{"cmd":"compare-snapshot"}` or
`{"cmd":"compare-snapshot","selector":"main"}`. The Python client performs
matching and diffing outside the browser render loop; instrumentation has no
normal-frame cost when it is not requested.

| Command | Example |
|---------|---------|
| `chrome-screenshot` | `{"cmd":"chrome-screenshot","out":"/tmp/chrome.png"}` |
| `chrome-sync` | `{"cmd":"chrome-sync"}` — navigate Chrome to the active webcore URL for side-by-side inspection |
| `chrome-eval` | `{"cmd":"chrome-eval","expression":"getComputedStyle(document.querySelector('input')).color"}` — inspect Chrome's live page in GUI comparison mode |
| `chrome-type` | `{"cmd":"chrome-type","text":"hello"}` — send native text input to Chrome's focused control |
| `compare` | `{"cmd":"compare","out":"/tmp/compare.png"}` — capture webcore and Chrome screenshots and report visual diff metadata |

## Python REPL Shortcuts

```
python3 examples/debugclient.py 9222

dbg> ss                  screenshot
dbg> f h1                find elements
dbg> i .sidebar          inspect
dbg> deep .card          full inspection
dbg> computed h1         computed styles
dbg> rules h1            matched CSS rules
dbg> css h1 display,width  query specific props
dbg> bm .container       box model
dbg> path h1             CSS selector path
dbg> parent td           ancestor chain
dbg> dt                  DOM tree (JSON)
dbg> dt 42               subtree from node 42
dbg> tx h1               text content
dbg> a img src           get attribute
dbg> search hello        search by text
dbg> hit 640 200         hit test
dbg> a11y                accessibility tree
dbg> hl .card            highlight overlay
dbg> c #btn              click
dbg> c 100 200           click at coordinates
dbg> h .menu             hover
dbg> k Enter             send key
dbg> ty hello            type text
dbg> style h1 color red  set style
dbg> cls+ body dark      add class
dbg> cls- body dark      remove class
dbg> cls~ body dark      toggle class
dbg> force .item hover   force state
dbg> media video toggle  inspect/control media; seek with `media video seek 12.5`; set state with `media video volume 0.5`, `media video muted true`, `media video rate 1.25`
dbg> nav https://...     navigate
dbg> r 800               resize width
dbg> sc 200              scroll down
dbg> vp                  viewport info
dbg> net                 network info
dbg> perf                load timing
dbg> bench 3             benchmark
dbg> benchp              progressive benchmark
dbg> t                   text tree dump
dbg> q                   quit
dbg> {"cmd":"..."}       raw JSON command
```

## Typical Workflows

### Debug a layout issue
```bash
cargo run --release --example browser -- --headless --debug-port 9222 file:///path/to/page.html
python3 examples/debugclient.py 9222
```
```
dbg> f .broken-element
dbg> bm .broken-element
dbg> path .broken-element
dbg> computed .broken-element
dbg> rules .broken-element
dbg> style .broken-element display flex
dbg> ss
```

### Compare with Chrome
```bash
cargo run --release --example browser -- --headless --chrome https://example.com
python3 examples/debugclient.py 9222
```
```
dbg> {"cmd":"compare","selector":"[id]"}     geometry diff, both engines
dbg> {"cmd":"compare","selector":".card"}    narrow it to one component
dbg> ss
dbg> {"cmd":"chrome-screenshot","out":"/tmp/chrome.png"}
```

`compare` is the first thing to reach for on a layout difference: it says
WHICH elements disagree and by how much, before any screenshot is opened.

### Performance profiling
```
dbg> perf
dbg> bench 5
dbg> benchp
```
