"""Parser for iCatch RIFF/SPST .SST menu string tables (DiCam850 RO_RES/UI/SST).
'strn' chunk; after size(4)+header(6): sections (4-ASCII tag + 4-byte marker) and
entries [id u16][byteLen u16][UTF-16LE text]. Entry id == menu-table index.
Trust [id,len] structure + id monotonicity; don't reject text by char range."""
import struct, sys

def parse(path):
    d = open(path, 'rb').read()
    i = d.find(b'strn') + 4
    size = struct.unpack_from('<I', d, i)[0]; i += 4
    end = min(i + size, len(d)); i += 6
    out = []
    last = -1
    while i + 4 <= end:
        idv = d[i] | (d[i + 1] << 8)
        ln = d[i + 2] | (d[i + 3] << 8)
        ok = (4 <= ln <= 200 and ln % 2 == 0 and i + 4 + ln <= end
              and -4 <= idv - last <= 4000 and idv >= last - 1)
        if ok:
            s = d[i + 4:i + 4 + ln].decode('utf-16le', 'replace')
            out.append((idv, s))
            last = idv
            i += 4 + ln
        elif all(0x20 <= d[i + k] < 0x7f for k in range(4)):
            i += 8   # section tag + marker
        else:
            i += 2
    return out

if __name__ == '__main__':
    sys.stdout.reconfigure(encoding='utf-8')
    e = parse(sys.argv[1])
    ids = [x[0] for x in e]
    print(f'{len(e)} entries, id {min(ids)}..{max(ids)}')
    if len(sys.argv) > 2:
        for idv, s in e:
            print(f'  id={idv:4d}  {s!r}')
