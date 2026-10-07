"""Generate Wiffletree logo SVGs from Instrument Sans SemiBold (600) outlines."""
import os
from fontTools.ttLib import TTFont
from fontTools.pens.svgPathPen import SVGPathPen
from fontTools.pens.transformPen import TransformPen
from fontTools.pens.boundsPen import BoundsPen

FONT = TTFont(os.path.join(os.path.dirname(__file__), 'instrument-sans-600-subset.ttf'))
GS = FONT.getGlyphSet()
CMAP = FONT.getBestCmap()
UPM = 1000

# ---- colours (Tidal) ----
DEEP = '#1A262D'; STONE = '#D1D1CD'; TIDE = '#2D5A5A'; SIGNAL = '#D63C35'
CANVAS_L = '#F3F2ED'; SURFACE_L = '#FFFDF7'; SEAGLASS = '#86C1BC'

# ---- kerning from GPOS (PairPos, possibly inside Extension) ----
def _pairpos_subtables():
    for lk in FONT['GPOS'].table.LookupList.Lookup:
        for st in lk.SubTable:
            if lk.LookupType == 9:
                st = st.ExtSubTable
            if getattr(st, 'LookupType', 2) == 2 or st.__class__.__name__ == 'PairPos':
                yield st

def kern(a, b):
    for st in _pairpos_subtables():
        cov = st.Coverage.glyphs
        if a not in cov:
            continue
        if st.Format == 1:
            ps = st.PairSet[cov.index(a)]
            for pvr in ps.PairValueRecord:
                if pvr.SecondGlyph == b:
                    v = pvr.Value1
                    return getattr(v, 'XAdvance', 0) or 0
        else:
            c1 = st.ClassDef1.classDefs.get(a, 0)
            c2 = st.ClassDef2.classDefs.get(b, 0)
            v = st.Class1Record[c1].Class2Record[c2].Value1
            x = getattr(v, 'XAdvance', 0) or 0
            if x:
                return x
    return 0

def glyph_path(name, x, y_base, scale):
    """SVG path d for glyph placed with origin at (x, y_base) in SVG coords, scale = px per unit."""
    pen = SVGPathPen(GS)
    tp = TransformPen(pen, (scale, 0, 0, -scale, x, y_base))
    GS[name].draw(tp)
    return pen.getCommands()

def bounds(name):
    p = BoundsPen(GS); GS[name].draw(p); return p.bounds

def fmt(v):
    return ('%.2f' % v).rstrip('0').rstrip('.')

# ---- wordmark: original Instrument Sans spacing, native i dot ----
TRACK = -30

def layout(text='wiffletree'):
    names = [CMAP[ord(ch)] for ch in text]
    x = 0
    glyphs = []
    for i, name in enumerate(names):
        glyphs.append((name, x))
        x += FONT['hmtx'][name][0] + TRACK
        if i + 1 < len(names):
            x += kern(name, names[i + 1])
    return glyphs

def wordmark_bounds():
    glyphs = layout()
    return (min(x + bounds(n)[0] for n, x in glyphs),
            min(bounds(n)[1] for n, _ in glyphs),
            max(x + bounds(n)[2] for n, x in glyphs),
            max(bounds(n)[3] for n, _ in glyphs))

def wordmark_elems(x, baseline, scale, ink, tree=None):
    main_paths, tree_paths = [], []
    for i, (name, offset) in enumerate(layout()):
        path = glyph_path(name, x + offset * scale, baseline, scale)
        (tree_paths if tree and i >= 6 else main_paths).append(path)
    elements = [f'<path fill="{ink}" d="{" ".join(main_paths)}"/>']
    if tree_paths:
        elements.append(f'<path fill="{tree}" d="{" ".join(tree_paths)}"/>')
    return elements

def svg(w, h, body, title='Wiffletree'):
    return (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 {fmt(w)} {fmt(h)}" '
            f'width="{fmt(w)}" height="{fmt(h)}" role="img" aria-label="{title}">\n'
            f'<title>{title}</title>\n' + '\n'.join(body) + '\n</svg>\n')

def wordmark(ink, tree=None):
    x0, y0, x1, y1 = wordmark_bounds()
    scale = 100 / UPM
    return svg((x1-x0)*scale, (y1-y0)*scale,
               wordmark_elems(-x0*scale, y1*scale, scale, ink, tree))

# Open-joint silhouette redrawn from the selected concept on a 444 × 214 grid.
# The red joint meets the center stroke; open channels separate both outer arms.
LEFT = 'M18 0 H79 Q97 0 107 16 L190 146 Q193 150 190 155 L149 201 Q128 223 107 203 L3 29 Q0 10 7 2 Q11 0 18 0 Z'
RIGHT = 'M277 106 L324 18 Q334 0 355 0 H425 Q453 0 439 26 L326 191 Z'
CENTER = 'M212 147 H265 L302 201 Q310 210 299 210 H269 Q248 210 239 192 Z'
JOINT = 'M176 74 H207 Q220 74 229 88 L265 143 Q268 147 261 147 H212 L169 83 Q165 74 176 74 Z'
MARK_WIDTH = 444
MARK_HEIGHT = 214

def mark_elems(ink, joint=SIGNAL, width=444, x=0, y=0):
    scale = width / MARK_WIDTH
    return [f'<g transform="translate({fmt(x)} {fmt(y)}) scale({scale:.8f})">',
            f'<path fill="{ink}" d="{LEFT} {RIGHT} {CENTER}"/>',
            f'<path fill="{joint}" d="{JOINT}"/>', '</g>']

def icon_elems(tile, ink, small=False, size=1000, x=0, y=0):
    width = size * (0.82 if small else 0.76)
    height = width * MARK_HEIGHT / MARK_WIDTH
    return [f'<rect fill="{tile}" x="{fmt(x)}" y="{fmt(y)}" width="{fmt(size)}" '
            f'height="{fmt(size)}" rx="{fmt(size*0.23)}"/>',
            *mark_elems(ink, width=width, x=x+(size-width)/2, y=y+(size-height)/2)]

def lockup(ink, tree, tile=None, icon_ink=None):
    x0, y0, x1, y1 = wordmark_bounds()
    scale = 100 / UPM
    word_w, word_h = (x1-x0)*scale, (y1-y0)*scale
    symbol_w = 200 if tile is None else 127
    height = symbol_w * MARK_HEIGHT / MARK_WIDTH if tile is None else symbol_w
    gap = 30
    elements = (mark_elems(ink, width=symbol_w) if tile is None else
                icon_elems(tile, icon_ink, size=symbol_w))
    elements += wordmark_elems(symbol_w+gap-x0*scale,
                              (height-word_h)/2+y1*scale, scale, ink, tree)
    return svg(symbol_w+gap+word_w, height, elements)

FILES = {}
for name, ink, tree in [('light', DEEP, None), ('dark', CANVAS_L, None),
                        ('two-tone-light', DEEP, TIDE), ('two-tone-dark', CANVAS_L, SEAGLASS),
                        ('mono-deep-teal', DEEP, None), ('mono-white', '#FFFFFF', None)]:
    FILES[f'logo/wordmark-{name}.svg'] = wordmark(ink, tree)

FILES['logo/lockup-light.svg'] = lockup(DEEP, TIDE)
FILES['logo/lockup-dark.svg'] = lockup(CANVAS_L, SEAGLASS)
FILES['logo/lockup-sea-glass-light.svg'] = lockup(DEEP, TIDE, SEAGLASS, DEEP)
FILES['logo/lockup-tide-dark.svg'] = lockup(CANVAS_L, SEAGLASS, TIDE, CANVAS_L)
for name, ink, joint in [('light', DEEP, SIGNAL), ('dark', CANVAS_L, SIGNAL),
                         ('mono-deep-teal', DEEP, DEEP), ('mono-white', '#FFFFFF', '#FFFFFF')]:
    FILES[f'logo/symbol-{name}.svg'] = svg(MARK_WIDTH, MARK_HEIGHT, mark_elems(ink, joint))

for name, tile, ink in [('deep-teal', DEEP, CANVAS_L), ('stone', STONE, DEEP),
                         ('sea-glass', SEAGLASS, DEEP), ('tide', TIDE, CANVAS_L)]:
    for small in (False, True):
        suffix = '-small' if small else ''
        FILES[f'icon/icon-{name}{suffix}.svg'] = svg(1000, 1000, icon_elems(tile, ink, small))
FILES['icon/favicon.svg'] = FILES['icon/icon-deep-teal-small.svg']

# Backgrounds are included only in the review sheet; delivery assets stay transparent.
preview = [f'<rect width="1200" height="780" fill="{CANVAS_L}"/>',
           f'<rect y="390" width="1200" height="390" fill="{DEEP}"/>']
for y, ink, tree in [(80, DEEP, TIDE), (470, CANVAS_L, SEAGLASS)]:
    contents = lockup(ink, tree)
    import re
    contents = re.sub(r' width="[^"]*" height="[^"]*"', '', contents, count=1)
    preview.append(contents.replace('<svg ', f'<svg x="100" y="{y}" width="1000" height="150" ', 1))
for y in (265, 655):
    for x, tile, ink in [(100, DEEP, CANVAS_L), (205, SEAGLASS, DEEP), (310, STONE, DEEP)]:
        preview += icon_elems(tile, ink, size=80, x=x, y=y)
FILES['preview.svg'] = svg(1200, 780, preview, 'Wiffletree — red Interlock and two-tone wordmark')

if __name__ == '__main__':
    import sys
    from pathlib import Path
    output = Path(sys.argv[1]) if len(sys.argv) > 1 else Path(__file__).resolve().parent.parent
    for name, content in FILES.items():
        path = output / name
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content)
    print(f'{len(FILES)} SVGs')
