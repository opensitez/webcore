#!/usr/bin/env python3
"""
debugclient.py — Python client for Phoenix Browser debug server.

Usage as CLI:
    python3 examples/debugclient.py [port]              # interactive REPL
    python3 examples/debugclient.py send [port] '...'   # one-shot

Usage as library (from Claude Code bash):
    python3 -c "
    from examples.debugclient import DebugClient
    c = DebugClient()
    print(c.find('h1'))
    print(c.computed('h1'))
    c.screenshot('/tmp/out.png')
    "
"""

import socket
import json
import sys
import os

class DebugClient:
    """Persistent TCP connection to browser debug server with typed methods."""

    def __init__(self, port=9222, host='127.0.0.1'):
        self.host = host
        self.port = port
        self.sock = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        self.sock.settimeout(30)
        self.sock.connect((host, port))
        self._buf = b''

    def close(self):
        try: self.sock.close()
        except: pass

    def __enter__(self):
        return self

    def __exit__(self, *args):
        self.close()

    def _send(self, cmd_dict):
        """Send a command dict and return parsed JSON response."""
        line = json.dumps(cmd_dict, separators=(',', ':'))
        self.sock.sendall((line + '\n').encode())
        # Read until newline
        while b'\n' not in self._buf:
            chunk = self.sock.recv(1 << 20)  # 1MB chunks
            if not chunk:
                raise ConnectionError("Server disconnected")
            self._buf += chunk
        line_bytes, self._buf = self._buf.split(b'\n', 1)
        return json.loads(line_bytes.decode())

    def raw(self, cmd_dict):
        """Send raw command dict, return raw response dict."""
        return self._send(cmd_dict)

    # ── Navigation & Rendering ───────────────────────────────────────────

    def screenshot(self, out=None):
        """Take a screenshot. Returns response with path and timing."""
        cmd = {"cmd": "screenshot"}
        if out: cmd["out"] = out
        return self._send(cmd)

    def navigate(self, url):
        """Load a new URL."""
        return self._send({"cmd": "navigate", "url": url})

    def resize(self, width=None, height=None):
        """Resize viewport and re-layout."""
        cmd = {"cmd": "resize"}
        if width is not None: cmd["width"] = width
        if height is not None: cmd["height"] = height
        return self._send(cmd)

    def scroll(self, dy=None, y=None, selector=None):
        """Scroll viewport. dy=relative, y=absolute, selector=scroll to element."""
        if selector:
            return self._send({"cmd": "scroll", "selector": selector})
        if y is not None:
            return self._send({"cmd": "scroll", "y": y})
        return self._send({"cmd": "scroll", "dy": dy or 0})

    def visible(self):
        """Get all block elements currently visible in the viewport."""
        r = self._send({"cmd": "visible"})
        return r

    # ── Finding Elements ─────────────────────────────────────────────────

    def find(self, selector):
        """Find elements matching selector. Returns list of elements."""
        r = self._send({"cmd": "find", "selector": selector})
        return r.get("elements", [])

    def text(self, selector):
        """Get text content of matching elements."""
        r = self._send({"cmd": "text", "selector": selector})
        return r.get("texts", [])

    def attr(self, selector, name):
        """Get attribute value from matching elements."""
        r = self._send({"cmd": "attr", "selector": selector, "name": name})
        return r.get("values", [])

    def html(self, selector):
        """Get serialized HTML of matching elements."""
        r = self._send({"cmd": "html", "selector": selector})
        return r.get("html", [])

    def path(self, selector):
        """Get full DOM path for matching elements."""
        r = self._send({"cmd": "dom-path", "selector": selector})
        return r.get("paths", [])

    def parent(self, selector):
        """Get ancestor chains for matching elements."""
        r = self._send({"cmd": "parent", "selector": selector})
        return r.get("chains", [])

    # ── Inspection ───────────────────────────────────────────────────────

    def inspect(self, selector):
        """Inspect elements — box model + key styles."""
        r = self._send({"cmd": "inspect", "selector": selector})
        return r.get("elements", [])

    def deep(self, selector):
        """Deep inspect — all attrs, rects, CSS values, line cache, children."""
        r = self._send({"cmd": "deep", "selector": selector})
        return r.get("elements", [])

    def computed(self, selector):
        """Get all computed CSS properties."""
        r = self._send({"cmd": "computed", "selector": selector})
        return r.get("elements", [])

    def css(self, selector, *props):
        """Query specific CSS properties.
        Usage: c.css('td', 'vertical-align', 'padding-top')
        """
        r = self._send({"cmd": "css", "selector": selector, "props": ",".join(props)})
        return r.get("elements", [])

    def rules(self, selector):
        """Get matched CSS rules with specificity and source."""
        r = self._send({"cmd": "matched-rules", "selector": selector})
        return r.get("elements", [])

    def highlight(self, selector, out=None):
        """Screenshot with box model overlay on matching elements."""
        cmd = {"cmd": "highlight", "selector": selector}
        if out: cmd["out"] = out
        return self._send(cmd)

    # ── Interaction ──────────────────────────────────────────────────────

    def click(self, selector=None, x=None, y=None):
        """Click by selector (center) or coordinates."""
        if selector:
            return self._send({"cmd": "click", "selector": selector})
        return self._send({"cmd": "click", "x": x, "y": y})

    def hover(self, selector=None, x=None, y=None):
        """Hover by selector (center) or coordinates."""
        if selector:
            return self._send({"cmd": "hover", "selector": selector})
        return self._send({"cmd": "hover", "x": x, "y": y})

    def type_text(self, text):
        """Type text into focused element."""
        return self._send({"cmd": "type", "text": text})

    def key(self, key_name):
        """Send a key press (Enter, Tab, Backspace, etc.)."""
        return self._send({"cmd": "key", "key": key_name})

    # ── Mutation ─────────────────────────────────────────────────────────

    def setstyle(self, selector, prop, value):
        """Modify a CSS property and re-layout."""
        return self._send({"cmd": "setstyle", "selector": selector, "prop": prop, "value": value})

    def setattr(self, selector, name, value):
        """Modify an HTML attribute."""
        return self._send({"cmd": "setattr", "selector": selector, "name": name, "value": value})

    # ── Debug ────────────────────────────────────────────────────────────

    def tree(self, selector=None):
        """Dump box tree (optionally filtered by selector)."""
        cmd = {"cmd": "tree"}
        if selector: cmd["selector"] = selector
        r = self._send(cmd)
        return r.get("tree", "")

    def perf(self):
        """Get load timing breakdown."""
        return self._send({"cmd": "perf"})

    def bench(self, n=5):
        """Benchmark cascade/layout/render n times."""
        return self._send({"cmd": "bench", "n": n})

    def subtree(self, selector):
        """Get container + all descendants with full computed layout."""
        r = self._send({"cmd": "subtree", "selector": selector})
        return r.get("elements", [])

    # ── Chrome comparison ────────────────────────────────────────────────

    def sync(self, chrome_port=9223):
        """Sync scroll position to Chrome reference window via CDP."""
        scroll_y = self._send({"cmd": "scroll", "dy": 0}).get('scroll_y', 0)
        return _cdp_eval(chrome_port, f"window.scrollTo(0, {scroll_y})")

    def chrome_label(self, chrome_port=9223):
        """Set Chrome window title to '[Chrome Reference]' for easy identification."""
        return _cdp_eval(chrome_port,
            "document.title = '[Chrome] ' + document.title.replace('[Chrome] ', '')")

    def chrome_screenshot(self, out='chrome_screenshot.png', chrome_port=9223):
        """Take a screenshot from the Chrome reference window via CDP."""
        return _cdp_screenshot(chrome_port, out)

    # ── High-level helpers ───────────────────────────────────────────────

    def overflow(self, selector):
        """Check a container for any descendants that overflow its bounds.
        Returns a list of overflow violations with element details."""
        results = self.subtree(selector)
        violations = []
        for result in results:
            container = result['container']
            cx, cy = container['x'], container['y']
            cr, cb = container['right'], container['bottom']
            cw, ch = container['w'], container['h']

            for node in result['descendants']:
                if node['depth'] == 0:
                    continue  # skip the container itself
                tag = node['tag']
                if tag == '#text' and node['content'][2] == 0:
                    continue  # skip zero-width text nodes

                nx, ny, nw, nh = node['content']
                nr = node['right']
                nb = node['bottom']

                issues = []
                if nw > 0 and nr > cr + 1:
                    issues.append(f"right overflow +{nr - cr:.0f}px")
                if nh > 0 and nb > cb + 1:
                    issues.append(f"bottom overflow +{nb - cb:.0f}px")
                if nw > 0 and nx < cx - 1:
                    issues.append(f"left overflow -{cx - nx:.0f}px")

                # Check line_cache too
                for i, line in enumerate(node.get('lines', [])):
                    lx, ly, lw, lh = line
                    lr = lx + lw
                    if lr > cr + 1:
                        issues.append(f"line[{i}] right overflow +{lr - cr:.0f}px (w={lw:.0f})")

                if issues:
                    label = tag
                    nid = node.get('id', '')
                    ncls = node.get('class', '')
                    if nid: label += f'#{nid}'
                    if ncls: label += f'.{ncls}'
                    text = node.get('text', '')
                    violations.append({
                        'element': label,
                        'depth': node['depth'],
                        'rect': [nx, ny, nw, nh],
                        'issues': issues,
                        'display': node.get('display', ''),
                        'text': text[:60] if text else '',
                        'container_w': cw,
                    })

        if not violations:
            print(f"No overflow in {selector} (container {cw:.0f}x{ch:.0f})")
        else:
            print(f"=== {len(violations)} overflow(s) in {selector} (container {cw:.0f}x{ch:.0f}) ===")
            for v in violations:
                indent = '  ' * v['depth']
                print(f"{indent}<{v['element']}> [{v['display']}] {v['rect'][2]:.0f}x{v['rect'][3]:.0f} @ ({v['rect'][0]:.0f},{v['rect'][1]:.0f})")
                for issue in v['issues']:
                    print(f"{indent}  -> {issue}")
                if v['text']:
                    print(f"{indent}  text: {v['text']!r}")
        return violations

    def dump(self, selector):
        """Print a human-readable summary of an element for debugging."""
        elements = self.deep(selector)
        for el in elements:
            tag = el.get('tag', '?')
            eid = el.get('id', '')
            ecls = el.get('class', '')
            label = tag
            if eid: label += f'#{eid}'
            if ecls: label += f'.{ecls.replace(" ", ".")}'

            cr = el.get('content', {})
            pr = el.get('padding', {})
            mr = el.get('margin', {})

            print(f"\n{'='*60}")
            print(f"  <{label}>")
            print(f"  content:  ({cr.get('x',0):.0f}, {cr.get('y',0):.0f}) {cr.get('w',0):.0f} x {cr.get('h',0):.0f}")
            print(f"  padding:  ({pr.get('x',0):.0f}, {pr.get('y',0):.0f}) {pr.get('w',0):.0f} x {pr.get('h',0):.0f}")
            print(f"  margin:   ({mr.get('x',0):.0f}, {mr.get('y',0):.0f}) {mr.get('w',0):.0f} x {mr.get('h',0):.0f}")

            rp = el.get('resolved_padding', [0,0,0,0])
            rm = el.get('resolved_margin', [0,0,0,0])
            rb = el.get('resolved_border', [0,0,0,0])
            print(f"  padding:  {rp[0]:.0f} {rp[1]:.0f} {rp[2]:.0f} {rp[3]:.0f} (T R B L)")
            print(f"  margin:   {rm[0]:.0f} {rm[1]:.0f} {rm[2]:.0f} {rm[3]:.0f}")
            print(f"  border:   {rb[0]:.0f} {rb[1]:.0f} {rb[2]:.0f} {rb[3]:.0f}")

            print(f"  display:      {el.get('display','')}")
            print(f"  position:     {el.get('position','')}")
            print(f"  vert-align:   {el.get('vertical_align','')}")
            print(f"  text-align:   {el.get('text_align','')}")

            attrs = el.get('attrs', {})
            if attrs:
                print(f"  attrs:        {attrs}")

            children = el.get('children', [])
            if children:
                print(f"  children ({len(children)}):")
                for ch in children[:10]:
                    ct = ch.get('tag','')
                    ci = ch.get('id','')
                    cc = ch.get('class','')
                    cx = ch.get('c', [0,0,0,0])
                    chl = ct
                    if ci: chl += f'#{ci}'
                    if cc: chl += f'.{cc}'
                    print(f"    <{chl}> {ch.get('display','')} ({cx[0]:.0f},{cx[1]:.0f} {cx[2]:.0f}x{cx[3]:.0f})")

            text_nodes = el.get('text_nodes', [])
            if text_nodes:
                for tn in text_nodes:
                    print(f"  text: {tn.get('text','')!r}")

    def compare(self, selector, prop, value, out_before='/tmp/before.png', out_after='/tmp/after.png'):
        """Screenshot before/after a style change."""
        self.screenshot(out_before)
        self.setstyle(selector, prop, value)
        self.screenshot(out_after)
        print(f"Before: {out_before}")
        print(f"After:  {out_after}")

    def find_at(self, x, y):
        """Click at coordinates and return what was hit."""
        return self._send({"cmd": "click", "x": x, "y": y})

    def profile(self):
        """Print a human-readable performance summary."""
        p = self.perf()
        lt = p.get('load_timing', {})
        print(f"\nPage load: {lt.get('total_ms', 0):.1f}ms")
        for key in ['fetch_html', 'parse_html', 'fetch_css', 'cascade', 'layout', 'fetch_images']:
            v = lt.get(key, 0)
            if isinstance(v, (int, float)) and v > 0:
                bar = '#' * max(1, int(v / lt.get('total_ms', 1) * 40))
                print(f"  {key:15s} {v:8.1f}ms  {bar}")
        for key in ['nodes', 'text_nodes', 'css_rules']:
            v = lt.get(key, 0)
            if v: print(f"  {key:15s} {int(v):>8d}")

        b = self.bench(3)
        print(f"\nBenchmark (3 iterations):")
        for stage in ['cascade_ms', 'layout_ms', 'render_ms']:
            s = b.get(stage, {})
            name = stage.replace('_ms', '')
            print(f"  {name:10s}  avg={s.get('avg',0):.1f}ms  min={s.get('min',0):.1f}ms  max={s.get('max',0):.1f}ms")
        print(f"  {'total':10s}  avg={b.get('total_avg_ms',0):.1f}ms")


# ── Chrome CDP helpers ────────────────────────────────────────────────────────

def _cdp_ws_url(chrome_port):
    """Get the WebSocket debugger URL for the first page tab."""
    import urllib.request
    resp = urllib.request.urlopen(f'http://127.0.0.1:{chrome_port}/json', timeout=2)
    targets = json.loads(resp.read().decode())
    for t in targets:
        if t.get('type') == 'page':
            return t.get('webSocketDebuggerUrl')
    return None

def _cdp_send_ws(ws_url, method, params=None):
    """Send a CDP command via WebSocket and return the result."""
    import struct, random, hashlib, base64
    # Parse ws://host:port/path
    url = ws_url.replace('ws://', '')
    hp, path = url.split('/', 1) if '/' in url else (url, '')
    host, port_str = hp.split(':')
    port = int(port_str)
    path = '/' + path

    s = socket.socket()
    s.settimeout(5)
    s.connect((host, port))

    # WebSocket handshake
    key = base64.b64encode(random.randbytes(16)).decode()
    req = (f"GET {path} HTTP/1.1\r\n"
           f"Host: {host}:{port}\r\n"
           f"Upgrade: websocket\r\nConnection: Upgrade\r\n"
           f"Sec-WebSocket-Key: {key}\r\n"
           f"Sec-WebSocket-Version: 13\r\n\r\n")
    s.sendall(req.encode())
    resp = b''
    while b'\r\n\r\n' not in resp:
        resp += s.recv(4096)

    # Send CDP command
    msg = json.dumps({"id": 1, "method": method, "params": params or {}})
    payload = msg.encode()
    frame = bytearray([0x81])
    mask_key = random.randbytes(4)
    length = len(payload)
    if length < 126:
        frame.append(0x80 | length)
    elif length < 65536:
        frame.append(0x80 | 126)
        frame.extend(struct.pack(">H", length))
    else:
        frame.append(0x80 | 127)
        frame.extend(struct.pack(">Q", length))
    frame.extend(mask_key)
    frame.extend(bytearray(b ^ mask_key[i % 4] for i, b in enumerate(payload)))
    s.sendall(bytes(frame))

    # Read response frame
    data = b''
    while len(data) < 2:
        data += s.recv(65536)
    plen = data[1] & 0x7F
    offset = 2
    if plen == 126:
        while len(data) < 4: data += s.recv(65536)
        plen = struct.unpack(">H", data[2:4])[0]
        offset = 4
    elif plen == 127:
        while len(data) < 10: data += s.recv(65536)
        plen = struct.unpack(">Q", data[2:10])[0]
        offset = 10
    while len(data) < offset + plen:
        data += s.recv(65536)
    result = json.loads(data[offset:offset+plen].decode())
    s.close()
    return result

def _cdp_eval(chrome_port, expression):
    """Evaluate JavaScript in Chrome via CDP."""
    ws = _cdp_ws_url(chrome_port)
    if not ws:
        return {"ok": False, "error": "No Chrome page found"}
    result = _cdp_send_ws(ws, "Runtime.evaluate", {"expression": expression})
    return {"ok": True, "result": result}

def _cdp_screenshot(chrome_port, out_path):
    """Take a screenshot from Chrome via CDP."""
    ws = _cdp_ws_url(chrome_port)
    if not ws:
        return {"ok": False, "error": "No Chrome page found"}
    result = _cdp_send_ws(ws, "Page.captureScreenshot", {"format": "png"})
    if 'result' in result and 'data' in result['result']:
        import base64
        data = base64.b64decode(result['result']['data'])
        with open(out_path, 'wb') as f:
            f.write(data)
        return {"ok": True, "path": out_path, "size": len(data)}
    return {"ok": False, "error": "No screenshot data", "raw": str(result)[:200]}


# ── CLI ──────────────────────────────────────────────────────────────────────

def _interactive(port):
    import threading
    s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
    try:
        s.connect(('127.0.0.1', port))
    except ConnectionRefusedError:
        print(f"Cannot connect to 127.0.0.1:{port} — is the browser running with --debug-port?")
        sys.exit(1)

    print(f'Connected to debug server on port {port}')
    print()
    print('Shortcuts:')
    print('  ss                 screenshot        f <sel>      find elements')
    print('  i <sel>            inspect           deep <sel>   full inspection')
    print('  computed <sel>     computed styles    rules <sel>  matched CSS rules')
    print('  css <sel> <props>  query CSS props   hl <sel>     highlight overlay')
    print('  t [sel]            DOM tree          dt [nid]     JSON DOM tree')
    print('  tx <sel>           text content      a <sel> <n>  get attribute')
    print('  path <sel>         CSS selector path parent <sel> ancestor chain')
    print('  bm <sel>           box model         hit <x> <y>  hit test')
    print('  search <text>      search by text    a11y         accessibility tree')
    print('  c <sel>|<x> <y>    click             h <sel>|<x> <y>  hover')
    print('  k <key>            send key          ty <text>    type text')
    print('  style <sel> <p> <v> set style        cls+ <sel> <c> add class')
    print('  cls- <sel> <c>     remove class      cls~ <sel> <c> toggle class')
    print('  force <sel> <state> force hover/focus/active')
    print('  media <sel> [action] [time/value] inspect/control media elements')
    print('  nav <url>          navigate          r <w> [h]    resize')
    print('  sc <dy>            scroll            vp           viewport info')
    print('  net                network info      perf         load timing')
    print('  bench [n]          benchmark         benchp       progressive bench')
    print('  q                  quit server')
    print('  Any JSON: {"cmd":"...","key":"val"}')
    print()

    def reader():
        buf = b''
        while True:
            try:
                chunk = s.recv(65536)
                if not chunk: break
                buf += chunk
                while b'\n' in buf:
                    line, buf = buf.split(b'\n', 1)
                    try:
                        obj = json.loads(line.decode())
                        print(json.dumps(obj, indent=2))
                    except:
                        print(line.decode())
                    sys.stdout.flush()
            except:
                break
        print('\n[disconnected]')
        os._exit(0)

    t = threading.Thread(target=reader, daemon=True)
    t.start()

    shortcuts = {
        'ss': '{"cmd":"screenshot"}',
        't': '{"cmd":"tree"}',
        'q': '{"cmd":"quit"}',
        'perf': '{"cmd":"perf"}',
        'net': '{"cmd":"network"}',
        'vp': '{"cmd":"viewport"}',
        'a11y': '{"cmd":"a11y"}',
        'benchp': '{"cmd":"bench-progressive"}',
    }

    try:
        while True:
            line = input('dbg> ').strip()
            if not line: continue
            if line in shortcuts:
                line = shortcuts[line]
            elif line.startswith('f '):
                line = json.dumps({"cmd": "find", "selector": line[2:].strip()})
            elif line.startswith('i '):
                line = json.dumps({"cmd": "inspect", "selector": line[2:].strip()})
            elif line.startswith('deep '):
                line = json.dumps({"cmd": "deep", "selector": line[5:].strip()})
            elif line.startswith('computed '):
                line = json.dumps({"cmd": "computed", "selector": line[9:].strip()})
            elif line.startswith('rules '):
                line = json.dumps({"cmd": "matched-rules", "selector": line[6:].strip()})
            elif line.startswith('css '):
                parts = line[4:].strip().split(None, 1)
                sel = parts[0] if parts else ''
                props = parts[1] if len(parts) > 1 else ''
                line = json.dumps({"cmd": "css", "selector": sel, "props": props})
            elif line.startswith('tx '):
                line = json.dumps({"cmd": "text", "selector": line[3:].strip()})
            elif line.startswith('a ') and not line.startswith('a11y'):
                parts = line[2:].strip().split(None, 1)
                sel = parts[0] if parts else ''
                name = parts[1] if len(parts) > 1 else ''
                line = json.dumps({"cmd": "attr", "selector": sel, "name": name})
            elif line.startswith('hl '):
                line = json.dumps({"cmd": "highlight", "selector": line[3:].strip()})
            elif line.startswith('path '):
                line = json.dumps({"cmd": "dom-path", "selector": line[5:].strip()})
            elif line.startswith('parent '):
                line = json.dumps({"cmd": "parent", "selector": line[7:].strip()})
            elif line.startswith('bm '):
                line = json.dumps({"cmd": "box-model", "selector": line[3:].strip()})
            elif line.startswith('dt'):
                rest = line[2:].strip()
                if rest and rest.isdigit():
                    line = json.dumps({"cmd": "dom-tree", "nid": int(rest), "depth": 2})
                else:
                    line = json.dumps({"cmd": "dom-tree", "depth": 2})
            elif line.startswith('hit '):
                parts = line[4:].strip().split()
                if len(parts) >= 2:
                    line = json.dumps({"cmd": "hit", "x": float(parts[0]), "y": float(parts[1])})
            elif line.startswith('search '):
                line = json.dumps({"cmd": "search", "query": line[7:].strip()})
            elif line.startswith('style '):
                parts = line[6:].strip().split(None, 2)
                if len(parts) >= 3:
                    line = json.dumps({"cmd": "setstyle", "selector": parts[0], "prop": parts[1], "value": parts[2]})
            elif line.startswith('cls+ '):
                parts = line[5:].strip().split(None, 1)
                if len(parts) >= 2:
                    line = json.dumps({"cmd": "add-class", "selector": parts[0], "class": parts[1]})
            elif line.startswith('cls- '):
                parts = line[5:].strip().split(None, 1)
                if len(parts) >= 2:
                    line = json.dumps({"cmd": "remove-class", "selector": parts[0], "class": parts[1]})
            elif line.startswith('cls~ '):
                parts = line[5:].strip().split(None, 1)
                if len(parts) >= 2:
                    line = json.dumps({"cmd": "toggle-class", "selector": parts[0], "class": parts[1]})
            elif line.startswith('force '):
                parts = line[6:].strip().split(None, 1)
                if len(parts) >= 2:
                    line = json.dumps({"cmd": "force-state", "selector": parts[0], "state": parts[1]})
            elif line.startswith('media '):
                parts = line[6:].strip().split()
                if len(parts) >= 1:
                    payload = {
                        "cmd": "media",
                        "selector": parts[0],
                        "action": parts[1] if len(parts) > 1 else "state",
                    }
                    if len(parts) > 2:
                        if payload["action"] == "seek":
                            payload["time"] = float(parts[2])
                        elif payload["action"] in ("volume", "rate"):
                            payload["value"] = float(parts[2])
                        elif payload["action"] == "muted":
                            payload["value"] = parts[2].lower() in ("1", "true", "yes", "on")
                    line = json.dumps(payload)
            elif line.startswith('bench'):
                n = 5
                parts = line.split()
                if len(parts) > 1: n = int(parts[1])
                line = json.dumps({"cmd": "bench", "n": n})
            elif line.startswith('c '):
                rest = line[2:].strip().split()
                if len(rest) == 2 and rest[0].replace('.','',1).replace('-','',1).isdigit():
                    line = json.dumps({"cmd": "click", "x": float(rest[0]), "y": float(rest[1])})
                else:
                    line = json.dumps({"cmd": "click", "selector": line[2:].strip()})
            elif line.startswith('h '):
                rest = line[2:].strip().split()
                if len(rest) == 2 and rest[0].replace('.','',1).replace('-','',1).isdigit():
                    line = json.dumps({"cmd": "hover", "x": float(rest[0]), "y": float(rest[1])})
                else:
                    line = json.dumps({"cmd": "hover", "selector": line[2:].strip()})
            elif line.startswith('k '):
                line = json.dumps({"cmd": "key", "key": line[2:].strip()})
            elif line.startswith('ty '):
                line = json.dumps({"cmd": "type", "text": line[3:].strip()})
            elif line.startswith('nav '):
                line = json.dumps({"cmd": "navigate", "url": line[4:].strip()})
            elif line.startswith('r '):
                parts = line[2:].strip().split()
                cmd = {"cmd": "resize", "width": int(parts[0])}
                if len(parts) > 1: cmd["height"] = int(parts[1])
                line = json.dumps(cmd)
            elif line.startswith('sc '):
                line = json.dumps({"cmd": "scroll", "dy": int(line[3:].strip())})
            elif line.startswith('t '):
                line = json.dumps({"cmd": "tree", "selector": line[2:].strip()})
            elif not line.startswith('{'):
                line = json.dumps({"cmd": line})
            s.sendall((line + '\n').encode())
    except (EOFError, KeyboardInterrupt):
        pass
    finally:
        s.close()


if __name__ == '__main__':
    args = sys.argv[1:]
    if args and args[0] == 'send':
        port = int(args[1]) if len(args) > 1 else 9222
        cmd = args[2] if len(args) > 2 else '{}'
        s = socket.socket(socket.AF_INET, socket.SOCK_STREAM)
        s.settimeout(30)
        s.connect(('127.0.0.1', port))
        s.sendall((cmd + '\n').encode())
        buf = b''
        while b'\n' not in buf:
            chunk = s.recv(65536)
            if not chunk: break
            buf += chunk
        print(buf.decode().strip())
        s.close()
    else:
        port = 9222
        if args and args[0].isdigit():
            port = int(args[0])
        _interactive(port)
