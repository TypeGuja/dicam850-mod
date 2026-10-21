"""Map code addresses -> candidate function names taken from strings they load (pc-rel pools + movw/movt)."""
import struct, re, sys, collections
B = 0x40000000; img = open('main.bin', 'rb').read(); n = len(img) // 4
W = struct.unpack_from(f'<{n}I', img)
ANSI = re.compile(r'\x1b\[[0-9;]*m|\[[0-9;]+m')
IDENT = re.compile(r'^[A-Za-z_][A-Za-z0-9_]{5,63}$')
CALL = re.compile(r'(?:^|[\s\]>:])([A-Za-z_][A-Za-z0-9_]{5,63})\s*\(\s*\)?')       # name(...) / name()
PREFIX = re.compile(r'^\s*([A-Za-z_][A-Za-z0-9_]{5,63})\s*[:\]]\s')                # "name: ..."
STOP = set('Should Please Usage usage Error error Warning warning Failed failed Enable Disable Unknown unknown printf sprintf memcpy memset malloc assert return sizeof Invalid invalid Cannot Couldn'.split())
def good(name):
    if name in STOP or name.isupper() and '_' not in name: return False
    if not re.search(r'[a-z]', name): return False
    return bool(re.search(r'[a-z][A-Z]|_', name))       # camelCase or snake_case
strings = {}
for m in re.finditer(rb'(?<=\x00)[\x09\x20-\x7e\x1b]{6,}(?=\x00)', img):
    s = ANSI.sub('', m.group().decode('latin1')).strip()
    cands = []
    if IDENT.match(s) and good(s): cands.append((s, 3))          # bare __func__ string: strongest
    else:
        for rx, w in ((PREFIX, 2), (CALL, 1)):
            c = rx.search(s)
            if c and good(c.group(1)): cands.append((c.group(1), w)); break
    if cands: strings[B + m.start()] = cands[0]
pools = {B + i * 4: strings[w] for i, w in enumerate(W) if w in strings}
out = []
for i, w in enumerate(W):
    if (w & 0x0f7f0000) == 0x051f0000:
        imm = w & 0xfff; a = B + i * 4 + 8 + (imm if w & (1 << 23) else -imm)
        if a in pools: out.append((B + i * 4, *pools[a]))
last = {}
for i, w in enumerate(W):
    if (w & 0x0ff00000) == 0x03000000: last[(w >> 12) & 15] = ((((w >> 4) & 0xf000) | (w & 0xfff)), i)
    elif (w & 0x0ff00000) == 0x03400000:
        rd = (w >> 12) & 15
        if rd in last and i - last[rd][1] <= 8:
            v = (((((w >> 4) & 0xf000) | (w & 0xfff)) << 16) | last[rd][0])
            if v in strings: out.append((B + i * 4, *strings[v]))
open(sys.argv[1], 'w').write('\n'.join(f'{a:#x} {nm} {wt}' for a, nm, wt in out))
c = collections.Counter(wt for _, _, wt in out)
print(f'{len(strings)} name-bearing strings, {len(out)} code refs (bare={c[3]}, prefix={c[2]}, call={c[1]}), {len(set(x[1] for x in out))} distinct names')
