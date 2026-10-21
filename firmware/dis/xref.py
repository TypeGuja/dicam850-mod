"""Collect absolute constants built by ARM movw/movt pairs and literal pools; map strings -> code xrefs."""
import struct, re, sys, pickle
BASE = 0x40000000
img = open('main.bin', 'rb').read()
n = len(img) // 4
words = struct.unpack_from(f'<{n}I', img)
xref = {}            # const -> [code VA]
last = {}            # reg -> (movw value, addr)
for i, w in enumerate(words):
    if (w & 0x0ff00000) == 0x03000000:          # movw
        rd = (w >> 12) & 15; last[rd] = (((w >> 4) & 0xf000) | (w & 0xfff), i)
    elif (w & 0x0ff00000) == 0x03400000:        # movt
        rd = (w >> 12) & 15
        if rd in last and i - last[rd][1] <= 8:
            v = ((((w >> 4) & 0xf000) | (w & 0xfff)) << 16) | last[rd][0]
            xref.setdefault(v, []).append(BASE + i * 4)
# literal pool words too
lit = {}
for i, w in enumerate(words):
    if BASE <= w < BASE + len(img): lit.setdefault(w, []).append(BASE + i * 4)
pickle.dump((xref, lit), open('xref.pkl', 'wb'))
def va_of(s):
    return [BASE + m.start() for m in re.finditer(re.escape(s), img)]
hits = tot = 0
for m in re.finditer(rb'(?<=\x00)[\x20-\x7e]{8,}\x00', img[:0x900000]):
    tot += 1; va = BASE + m.start()
    if va in xref or va in lit: hits += 1
print(f'string starts referenced by code at base {BASE:#x}: {hits}/{tot}')
for s in [b'A:\RO_RES\IQ\AECONFIG.BIN', b'A:\RO_RES\IQ\AETARGET.BIN', b'A:\RO_RES\IQ\AEWINDOWCENTER.BIN', b'A:\RO_RES\IQ\AWDRALG.BIN', b'AEPCHART']:
    for va in va_of(s):
        print(f'{s.decode():40s} @ {va:#x}  movw/movt xrefs: {[hex(x) for x in xref.get(va,[])]}  literal xrefs: {[hex(x) for x in lit.get(va,[])][:5]}')
