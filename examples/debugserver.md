# Phoenix Browser — Debug & Inspector Guide

## Quick Start

```bash
# GUI browser with debug port
cargo run --release --example browser -- --debug-port 9222 https://example.com

# Headless mode (no window, for CI/scripting)
cargo run --release --example browser -- --headless https://example.com

# With Chrome comparison
cargo run --release --example browser -- --headless --chrome https://example.com
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
  --cached           Cache fetched resources to snapshot_cache/
  --cache-dir <dir>  Custom cache directory
  --no-images        Skip image loading
```

## Command Reference

All commands are JSON: `{"cmd":"name", ...}`. Responses include `"cmd_ms"` timing.

### Navigation
| Command | Example |
|---------|---------|
| `screenshot` | `{"cmd":"screenshot","out":"/tmp/page.png"}`; add `"scale":2` to exercise the same HiDPI raster path as a Retina GUI window |
| `navigate` | `{"cmd":"navigate","url":"https://example.com"}` |
| `scroll` | `{"cmd":"scroll","dy":200}` |
| `resize` | `{"cmd":"resize","width":800,"height":600}` |
| `viewport` | `{"cmd":"viewport"}` — returns width, height, scroll, doc_height |

### Finding & Querying
| Command | Example |
|---------|---------|
| `find` | `{"cmd":"find","selector":"h1"}` |
| `text` | `{"cmd":"text","selector":"h1"}` |
| `attr` | `{"cmd":"attr","selector":"a","name":"href"}` |
| `search` | `{"cmd":"search","query":"hello"}` — search by text content |
| `hit` | `{"cmd":"hit","x":640,"y":200}` — hit test at coordinates |
| `dom-path` | `{"cmd":"dom-path","selector":"h1"}` — CSS selector path |
| `parent` | `{"cmd":"parent","selector":"td"}` — ancestor chain |

### Inspection
| Command | Example |
|---------|---------|
| `inspect` | `{"cmd":"inspect","selector":".sidebar"}` |
| `inspect-node` | `{"cmd":"inspect-node","nid":42}` — by node_id |
| `deep` | `{"cmd":"deep","selector":"td"}` — full dump |
| `computed` | `{"cmd":"computed","selector":"h1"}` |
| `css` | `{"cmd":"css","selector":"td","props":"display,width"}` |
| `rules` | `{"cmd":"rules","selector":"h1"}` — matched CSS rules |
| `inspect-mode` | `{"cmd":"inspect-mode","on":"true"}` — recascade with matched-rule capture enabled for scripted `rules` inspection; GUI mode preserves the pre-panel page viewport so responsive media queries do not change while the inspector opens |
| `rule-search` | `{"cmd":"rule-search","query":"lg\\:flex","limit":10}` — search loaded stylesheet selectors while debugging cascade misses |
| `paint-dump` | `{"cmd":"paint-dump","x":0,"y":0,"w":400,"h":200,"limit":80}` — display-list commands in a viewport rectangle; text entries include font metrics and decoration flags, fill rectangles include color/radius, and CSS mask entries include mask image dimensions |
| `box-model` | `{"cmd":"box-model","selector":"div"}` — Chrome-style |
| `highlight` | `{"cmd":"highlight","selector":"h1","out":"/tmp/hl.png"}`; accepts `"scale":2` for HiDPI output |
| `dom-tree` | `{"cmd":"dom-tree","depth":2}` — structured JSON tree |
| `a11y` | `{"cmd":"a11y"}` — accessibility tree |

### Interaction
| Command | Example |
|---------|---------|
| `click` | `{"cmd":"click","selector":"#btn"}` or `{"cmd":"click","x":100,"y":200}` |
| `hover` | `{"cmd":"hover","selector":".menu"}` |
| `type` | `{"cmd":"type","text":"hello"}` |
| `key` | `{"cmd":"key","key":"Enter"}` |
| `force-state` | `{"cmd":"force-state","selector":".item","state":"hover"}` |

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
| `perf` | `{"cmd":"perf"}` — load timing breakdown |
| `bench` | `{"cmd":"bench","n":5}` — cascade/layout benchmark |
| `bench-progressive` | `{"cmd":"bench-progressive"}` — above-fold vs full |
| `network` | `{"cmd":"network"}` — resource count |
| `measure` | `{"cmd":"measure","from":"#a","to":"#b"}` — distance |

### Chrome Comparison (requires `--chrome`)
| Command | Example |
|---------|---------|
| `chrome-screenshot` | `{"cmd":"chrome-screenshot","out":"/tmp/chrome.png"}` |
| `chrome-sync` | `{"cmd":"chrome-sync"}` — sync scroll to Chrome |

### Browser Tabs (GUI mode only)
| Command | Example |
|---------|---------|
| `tabs` | `{"cmd":"tabs"}` — list open tabs |
| `switch-tab` | `{"cmd":"switch-tab","index":1}` |

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
