"""iCatch SPHOST.BRN checksum: sum of LE u32 words (tail bytes by position) mod 2^32 must be 0.
Compensation word stored at offset 0x1fc."""
import struct, sys
SLOT = 0x1fc
def brn_sum(d):
    n = len(d) // 4
    s = sum(struct.unpack_from(f'<{n}I', d)) 
    for i in range(n * 4, len(d)): s += d[i] << ((i & 3) * 8)
    return s & 0xffffffff
def fix(d):
    d = bytearray(d); struct.pack_into('<I', d, SLOT, 0)
    struct.pack_into('<I', d, SLOT, (-brn_sum(d)) & 0xffffffff); return bytes(d)
if __name__ == '__main__':
    for p in sys.argv[1:]:
        d = open(p, 'rb').read()
        stored = struct.unpack_from('<I', d, SLOT)[0]
        print(f'{p}\n   sum={brn_sum(d):#010x}  stored@0x1fc={stored:#010x}  recomputed={struct.unpack_from("<I", fix(d), SLOT)[0]:#010x}  {"OK" if brn_sum(d)==0 and fix(d)==d else "MISMATCH"}')
