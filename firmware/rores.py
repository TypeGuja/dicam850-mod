"""Extract the RO_RES FAT16 volume embedded in an iCatch SPHOST.BRN (read-only)."""
import sys, struct, os
img, out = sys.argv[1], sys.argv[2]
f = open(img, 'rb').read()
FAT, ROOT, DATA, CS = 0xa8320, 0xb0320, 0xb4320, 0x200
ROOT_N = (DATA - ROOT) // 32

def nxt(c): return struct.unpack_from('<H', f, FAT + c * 2)[0]
def chain(c):
    seen = []
    while 2 <= c < 0xfff8 and c not in seen:
        seen.append(c); c = nxt(c)
    return seen
def read_file(cl, size):
    data = b''.join(f[DATA + (c - 2) * CS: DATA + (c - 1) * CS] for c in chain(cl))
    return data[:size] if size else data

listing = []
def walk(entries, path):
    lfn = []
    for i in range(0, len(entries), 32):
        e = entries[i:i + 32]
        if e[0] == 0: break
        if e[0] == 0xe5: lfn = []; continue
        attr = e[11]
        if attr == 0x0f:
            part = (e[1:11] + e[14:26] + e[28:32]).decode('utf-16le', 'ignore').split('\x00')[0]
            lfn.insert(0, part); continue
        short = e[0:8].decode('latin1').rstrip() + ('.' + e[8:11].decode('latin1').rstrip() if e[8:11].strip() else '')
        name = ''.join(lfn) or short; lfn = []
        if short in ('.', '..'): continue
        cl = struct.unpack_from('<H', e, 26)[0]; size = struct.unpack_from('<I', e, 28)[0]
        p = os.path.join(path, name)
        if attr & 0x10:
            os.makedirs(os.path.join(out, p), exist_ok=True)
            walk(read_file(cl, 0), p)
        else:
            d = read_file(cl, size)
            with open(os.path.join(out, p), 'wb') as w: w.write(d)
            contiguous = chain(cl) == list(range(cl, cl + len(chain(cl))))
            listing.append((p, size, cl, contiguous))
os.makedirs(out, exist_ok=True)
walk(f[ROOT:ROOT + ROOT_N * 32], '')
for p, s, c, ok in listing:
    print(f"{s:>9}  cl={c:<5} {'' if ok else 'FRAG '}{p}")
print(f"\n{len(listing)} files, {sum(x[1] for x in listing):,} bytes total")
