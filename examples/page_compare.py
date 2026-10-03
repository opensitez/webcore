"""Read-only live-engine comparison, invoked by debugclient.py compare."""
import argparse
import collections
import json
import math
import re
import time
from pathlib import Path


CHROME_SNAPSHOT = r"""(selector => {
  const roots = selector ? [...document.querySelectorAll(selector)] : [document.documentElement];
  const chosen = new Set(roots.flatMap(e => [e, ...e.querySelectorAll('*')]));
  const idCounts = new Map();
  for (const e of document.querySelectorAll('[id]')) if(e.id) idCounts.set(e.id,(idCounts.get(e.id)||0)+1);
  const anchor = e => {
    const parts = [];
    for (; e; e = e.parentElement) {
      if (e.id && idCounts.get(e.id) === 1) return `${new TextEncoder().encode(e.id).length}|${e.id}${parts.join(' > ')}`;
      let i = 1;
      for (let p=e.previousElementSibling;p;p=p.previousElementSibling) if(p.localName===e.localName)i++;
      parts.unshift(`${e.localName}:nth-of-type(${i})`);
    }
    return '';
  };
  const path = e => {
    const parts = [];
    for (; e; e = e.parentElement) {
      let i = 1;
      for (let p = e.previousElementSibling; p; p = p.previousElementSibling)
        if (p.localName === e.localName) i++;
      parts.unshift(`${e.localName}:nth-of-type(${i})`);
    }
    return parts.join(' > ');
  };
  const canvas = document.createElement('canvas'); canvas.width = canvas.height = 1;
  const ctx = canvas.getContext('2d', {willReadFrequently:true}), colors = new Map();
  const color = value => {
    if (!colors.has(value)) {
      ctx.clearRect(0,0,1,1); ctx.fillStyle = value; ctx.fillRect(0,0,1,1);
      colors.set(value, [...ctx.getImageData(0,0,1,1).data]);
    }
    return colors.get(value);
  };
  const elements = [...chosen].map(e => {
    const s = getComputedStyle(e), r = e.getBoundingClientRect();
    const props = {};
    for (const p of ['display','position','float','visibility','opacity','box-sizing',
      'font-size','font-weight','font-family','text-align','direction','writing-mode',
      'flex-direction','flex-wrap','flex-grow','flex-shrink','align-items','justify-content'])
      props[p.replaceAll('-','_')] = s.getPropertyValue(p);
    props.overflow = [s.overflowX,s.overflowY];
    props.resolved_padding = ['Top','Right','Bottom','Left'].map(p => parseFloat(s['padding'+p]));
    props.resolved_margin = ['Top','Right','Bottom','Left'].map(p => parseFloat(s['margin'+p]));
    const text = [...e.childNodes].filter(n=>n.nodeType===3).map(n=>n.textContent).join(' ').replace(/\s+/gu,' ').trim();
    const attrs = Object.fromEntries(['href','src','alt','aria-label','role','name','type'].filter(k=>e.hasAttribute(k)).map(k=>[k,e.getAttribute(k)]));
    let hiddenBy = s.visibility !== 'visible' ? `visibility:${s.visibility}` : '';
    for(let n=e;n && !hiddenBy;n=n.parentElement) {
      const a=getComputedStyle(n);
      if(a.display==='none') hiddenBy=`display:none on ${path(n)}`;
      else if(Number(a.opacity)===0) hiddenBy=`opacity:0 on ${path(n)}`;
    }
    return {path:path(e), anchor:anchor(e), text:[...text].slice(0,160).join(''), hidden_by:hiddenBy, attrs, id:e.id, tag:e.localName, class:e.getAttribute('class') || '',
      computed:props, rect:[r.x+scrollX,r.y+scrollY,r.width,r.height],
      has_box:e.getClientRects().length > 0,
      transformed: (()=>{for(let n=e;n;n=n.parentElement)if(getComputedStyle(n).transform!=='none')return true;return false})(),
      colors:{color:color(s.color),background:color(s.backgroundColor)},
      image:e instanceof HTMLImageElement ? {src:e.currentSrc,width:e.naturalWidth,height:e.naturalHeight,decoded:e.complete && e.naturalWidth>0}:null};
  });
  return {url:location.href,ready:document.readyState,fonts:document.fonts.status,
    viewport:{width:innerWidth,height:innerHeight,scroll_y:scrollY,dpr:devicePixelRatio},
    elements,limitations:['Light DOM only; generated content, shadow roots, text runs, clipping and pixels are not compared.']};
})"""


def normalized(value):
    if isinstance(value, str):
        return re.sub(r'[\s_\-"\']', '', value).lower()
    return value


def same(a, b, tolerance):
    if isinstance(a, bool) or isinstance(b, bool):
        return type(a) is type(b) and a == b
    if isinstance(a, (float, int)) and isinstance(b, (float, int)):
        return math.isfinite(a) and math.isfinite(b) and abs(a - b) <= tolerance
    if isinstance(a, list) and isinstance(b, list):
        return len(a) == len(b) and all(same(x, y, tolerance) for x, y in zip(a, b))
    return normalized(a) == normalized(b)


def identity(entry):
    return entry if 'tag' in entry else entry['computed']


def fingerprint(entry):
    node = identity(entry)
    attrs = entry.get('attrs',{})
    text = entry.get('text','')
    # Classes and ordinal positions alone are not evidence of identity.
    if not text and not any(attrs.get(k) for k in ('href','src','alt','aria-label','name')):
        return None
    return (node['tag'],text,tuple(sorted(attrs.items())))


def match_elements(web, chrome):
    """Reserve stronger identities before ordinal paths can consume them."""
    matches, used = {}, set()
    def unique_pass(key, method):
        wi,ci = collections.defaultdict(list),collections.defaultdict(list)
        for e in web:
            k = key(e)
            if k: wi[k].append(e)
        for e in chrome:
            k = key(e)
            if k: ci[k].append(e)
        for k,ws in wi.items():
            cs = ci.get(k,[])
            if len(ws)!=1 or len(cs)!=1: continue
            w,c = ws[0],cs[0]
            if w['path'] in matches or c['path'] in used: continue
            a,b = identity(w),identity(c)
            if a['tag'] != b['tag'] or a['id'] != b['id']: continue
            if method != 'unique-id':
                if set(a['class'].split()) != set(b['class'].split()): continue
                if w.get('text') != c.get('text') or w.get('attrs',{}) != c.get('attrs',{}): continue
            matches[w['path']] = (c,method)
            used.add(c['path'])
    unique_pass(lambda e: identity(e)['id'],'unique-id')
    unique_pass(fingerprint,'unique-content')
    unique_pass(lambda e: (identity(e)['tag'],tuple(sorted(identity(e)['class'].split())),
                          e.get('text',''),tuple(sorted(e.get('attrs',{}).items())))
                if identity(e)['class'].strip() else None,'unique-class-signature')
    unique_pass(lambda e:e.get('anchor'),'ancestor-anchor')
    # Remap ordinal descendants only below already matched parents. This avoids
    # a parser's extra wrapper preventing comparison of the entire document.
    reference = {e['path']:e for e in chrome}
    for w in sorted(web,key=lambda e:e['path'].count(' > ')):
        if w['path'] in matches: continue
        parts = w['path'].rsplit(' > ',1)
        if len(parts)!=2 or parts[0] not in matches: continue
        parent = matches[parts[0]][0]['path']
        c = reference.get(parent+' > '+parts[1])
        if c is None or c['path'] in used: continue
        a,b = identity(w),identity(c)
        if (a['tag'],a['id'],set(a['class'].split()),w.get('text',''),w.get('attrs',{})) != (b['tag'],b['id'],set(b['class'].split()),c.get('text',''),c.get('attrs',{})): continue
        matches[w['path']] = (c,'matched-parent-path'); used.add(c['path'])
    unique_pass(lambda e:e['path'],'structural-path')
    return matches,used


def describe(entry):
    n = identity(entry)
    result = {k:n.get(k,'') for k in ('tag','id','class')}
    result.update(text=entry.get('text',''),attrs=entry.get('attrs',{}))
    return result


def compare_snapshots(web, chrome, tolerance=1.0):
    """Compare confidently matched nodes and retain uncertain matches as evidence."""
    reference = {e['path']: e for e in chrome['elements']}
    pairs,used = match_elements(web['elements'],chrome['elements'])
    differences, matched = [], 0
    offsets = {}
    counts = collections.Counter()
    for entry in web['elements']:
        w, path = entry['computed'], entry['path']
        c,method = pairs.get(path,(None,None))
        if c is None:
            differences.append({'path':path,'node_id':w['node_id'],'kind':'unmatched-webcore',
                'element':describe(entry),'interpretation':'Present in Webcore snapshot; no confident Chrome match.'})
            counts['unmatched-webcore'] += 1
            continue
        matched += 1
        changes = []
        def check(prop, a, b, epsilon=tolerance):
            if not same(a,b,epsilon):
                changes.append({'property':prop,'webcore':a,'chrome':b})
                counts[prop] += 1
        s = c['computed']
        hidden_mismatch = bool(entry.get('hidden_by')) != bool(c.get('hidden_by'))
        if hidden_mismatch:
            changes.append({'property':'rendering.hidden_by','webcore':entry.get('hidden_by') or None,'chrome':c.get('hidden_by') or None})
            counts['rendering.hidden_by'] += 1
        if entry.get('text','') != c.get('text',''):
            changes.append({'property':'text','webcore':entry.get('text',''),'chrome':c.get('text','')})
            counts['text'] += 1
        if set(w['class'].split()) != set(c['class'].split()):
            changes.append({'property':'class','webcore':w['class'],'chrome':c['class']})
            counts['class'] += 1
        for attr in sorted(entry.get('attrs',{}).keys() | c.get('attrs',{}).keys()):
            a,b = entry.get('attrs',{}).get(attr),c.get('attrs',{}).get(attr)
            if a != b:
                changes.append({'property':'attributes.'+attr,'webcore':a,'chrome':b})
                counts['attributes.'+attr] += 1
        for prop in ['display','position','float','box_sizing','direction','writing_mode',
                     'text_align','font_family','flex_direction','flex_wrap','overflow']:
            check(prop,w[prop],s[prop])
        if 'flex' in normalized(w['display']):
            check('align_items',w['align_items'],'stretch' if s['align_items']=='normal' else s['align_items'])
            check('justify_content',w['justify_content'],'flex-start' if s['justify_content']=='normal' else s['justify_content'])
        check('visibility.visible',w['visibility']=='true',s['visibility']=='visible',0)
        check('opacity',w['opacity'],float(s['opacity']),0.01)
        check('font_size',w['font_size'],float(s['font_size'].removesuffix('px')),0.15)
        weight = re.search(r'\d+',w['font_weight'])
        if weight: check('font_weight',int(weight.group()),int(s['font_weight']),0)
        for prop in ['resolved_padding','resolved_margin']:
            check(prop,w[prop],s[prop])
        # Chrome client rects include transforms; Webcore's layout rectangles do not.
        if c['has_box'] and not c['transformed'] and w['display'] not in ('None','Contents'):
            offsets[path] = [w['box']['border'][i]-c['rect'][i] for i in (0,1)]
            for i, prop in enumerate(['x','y','width','height']):
                check('geometry.'+prop,w['box']['border'][i],c['rect'][i])
        if c['has_box']:
            for prop in ['color','background']:
                check(prop,entry['colors'][prop],c['colors'][prop],1)
        if c['image']:
            check('image.decoded',entry['image']['decoded'],c['image']['decoded'],0)
        if changes:
            styles = any(not x['property'].startswith(('geometry.','image.')) for x in changes)
            differences.append({'path':path,'chrome_path':c['path'],'node_id':w['node_id'],
                'id':w['id'],'class':w['class'],'kind':'visibility-mismatch' if hidden_mismatch else 'style-or-cascade' if styles else 'layout-or-resource',
                'match_method':method,'element':describe(entry),'changes':changes})
            if any(x['property'].startswith('image.') for x in changes):
                differences[-1]['resources'] = {'webcore':entry['image'],'chrome':c['image']}
    for path,c in reference.items():
        if path not in used:
            differences.append({'path':path,'kind':'unmatched-chrome','id':c['id'],
                'element':describe(c),'interpretation':'Present in Chrome snapshot; no confident Webcore match.',
                'chrome_has_box':c['has_box'],'chrome_rect':c['rect']})
            counts['unmatched-chrome'] += 1
    differences.sort(key=lambda d: (d['path'].count(' > '), d['path']))
    missing = {kind:{d['path']:d for d in differences if d['kind']==kind}
               for kind in ('unmatched-webcore','unmatched-chrome')}
    for group in missing.values():
        for path,d in group.items():
            ancestors = path.split(' > ')
            for end in range(1,len(ancestors)):
                parent = ' > '.join(ancestors[:end])
                if parent in group:
                    d['unmatched_subtree_root'] = parent
                    group[parent]['unmatched_descendants'] = group[parent].get('unmatched_descendants',0)+1
                    break
    for difference in differences:
        parent = difference['path'].rsplit(' > ',1)[0]
        if parent == difference['path'] or parent not in offsets: continue
        for change in difference.get('changes',[]):
            if change['property'] in ('geometry.x','geometry.y'):
                axis = 0 if change['property']=='geometry.x' else 1
                delta = change['webcore']-change['chrome']
                if abs(offsets[parent][axis]) > tolerance and abs(delta-offsets[parent][axis]) <= tolerance:
                    change['same_offset_as_parent'] = parent
    warnings = []
    collapsed = sum(e['computed'].get('visibility')=='collapse' for e in chrome['elements'])
    if collapsed:
        warnings.append(f'{collapsed} Chrome elements use visibility:collapse; Webcore exposes only a visibility boolean, so collapse-specific layout semantics are not verified.')
    if web['url'] != chrome['url']: warnings.append('URLs differ: results may compare different documents.')
    if web.get('loading') or chrome['ready'] != 'complete' or chrome['fonts'] != 'loaded':
        warnings.append('Resources are still loading; rerun after settling to distinguish transient differences.')
    for key in ['width','height','scroll_y']:
        if key in web.get('viewport',{}) and not same(web['viewport'][key],chrome['viewport'][key],1):
            warnings.append(f'Viewport {key} differs; geometry and responsive CSS may not be comparable.')
    actionable = [d for d in differences if not d.get('unmatched_subtree_root') and
                  (not d.get('changes') or any('same_offset_as_parent' not in c for c in d['changes']))]
    def priority(d):
        if d.get('element',{}).get('tag') in ('head','meta','link','style','script','title','base','template'):
            return 3
        if d['kind'].startswith('unmatched') or d['kind']=='visibility-mismatch': return 0
        return 1 if d['kind']=='style-or-cascade' else 2
    actionable.sort(key=lambda d:(priority(d),d['path'].count(' > '),d['path']))
    return {'ok':True,'summary':{'webcore_elements':len(web['elements']),
        'chrome_elements':len(reference),'matched':matched,'different_elements':len(differences),
        'elements_with_independent_differences':sum(not d.get('changes') or any('same_offset_as_parent' not in c for c in d['changes']) for d in differences),
        'unmatched_subtree_roots':sum(not d.get('unmatched_subtree_root') for g in missing.values() for d in g.values()),
        'match_methods':dict(collections.Counter(method for _,method in pairs.values())),
        'properties':dict(counts.most_common())},'warnings':warnings,
        'environment':{'webcore':{k:v for k,v in web.items() if k!='elements'},
                       'chrome':{k:v for k,v in chrome.items() if k!='elements'}},
        'limitations':chrome.get('limitations',[]),'triage':actionable,'differences':differences}


def run(client, selector=None, tolerance=1.0, sync_viewport=False):
    request = {'cmd':'compare-snapshot'}
    if selector: request['selector'] = selector
    web = client.raw(request)
    if not web.get('ok'): raise RuntimeError(web)
    web['viewport'] = client.raw({'cmd':'viewport'})
    if sync_viewport:
        from debugclient import _cdp_send_ws, _cdp_ws_url
        if not web.get('chrome_port'): raise RuntimeError('Start browser with --chrome')
        viewport = web['viewport']
        result = _cdp_send_ws(_cdp_ws_url(web['chrome_port']),'Emulation.setDeviceMetricsOverride',
                             {'width':round(viewport['width']),'height':round(viewport['height']),
                              'deviceScaleFactor':1,'mobile':False})
        if 'error' in result: raise RuntimeError(result['error'])
        result = client.raw({'cmd':'chrome-eval','expression':f"window.scrollTo({float(viewport.get('scroll_x',0))},{float(viewport.get('scroll_y',0))})"})
        if not result.get('ok'): raise RuntimeError(result)
    expression = CHROME_SNAPSHOT + '(' + json.dumps(selector) + ')'
    response = client.raw({'cmd':'chrome-eval','expression':expression})
    if not response.get('ok'): raise RuntimeError(response)
    result = response['cdp'].get('result',{})
    if 'exceptionDetails' in result: raise RuntimeError(result['exceptionDetails'])
    chrome = result.get('result',{}).get('value')
    if not isinstance(chrome,dict): raise RuntimeError('Chrome did not return a snapshot')
    report = compare_snapshots(web,chrome,tolerance)
    report['selector'] = selector
    if not web['elements'] or not chrome['elements']:
        report['warnings'].append('Selection is empty in at least one engine; this is not a passing comparison.')
    return report


def main(args):
    from debugclient import DebugClient
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('port',type=int)
    parser.add_argument('--selector',help='Include every matching element and its descendants; default whole document')
    parser.add_argument('--tolerance',type=float,default=1.0,help='Geometry tolerance in CSS pixels')
    parser.add_argument('--out',help='Write the complete JSON report')
    parser.add_argument('--limit',type=int,default=20,help='Maximum differences printed; report file is never truncated')
    parser.add_argument('--only',choices=['all','missing','visibility','styles','layout'],default='all',help='Filter console output only; JSON file remains complete')
    parser.add_argument('--sync-viewport',action='store_true',help='Explicitly align Chrome CSS viewport and scroll with Webcore before comparing')
    options = parser.parse_args(args)
    if options.tolerance < 0 or not math.isfinite(options.tolerance): parser.error('tolerance must be finite and nonnegative')
    start = time.monotonic()
    with DebugClient(options.port) as client:
        report = run(client,options.selector,options.tolerance,options.sync_viewport)
    report['elapsed_ms'] = round((time.monotonic()-start)*1000,1)
    if options.out:
        Path(options.out).write_text(json.dumps(report,indent=2,ensure_ascii=False)+'\n')
    output = dict(report)
    categories = {'missing':{'unmatched-webcore','unmatched-chrome'},'visibility':{'visibility-mismatch'},
                  'styles':{'style-or-cascade'},'layout':{'layout-or-resource'}}
    def selected(d): return options.only=='all' or d['kind'] in categories[options.only]
    output['triage'] = [d for d in report['triage'] if selected(d)][:max(0,options.limit)]
    output['differences'] = [d for d in report['differences'] if selected(d)][:max(0,options.limit)]
    output['omitted_from_console'] = len(report['differences'])-len(output['differences'])
    print(json.dumps(output,indent=2,ensure_ascii=False))
