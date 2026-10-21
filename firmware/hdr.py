import sys, struct, re, math, collections
def ent(b):
    if not b: return 0
    c=collections.Counter(b); n=len(b)
    return -sum(v/n*math.log2(v/n) for v in c.values())
for p in sys.argv[1:]:
    f=open(p,'rb').read()
    print(f"\n######## {p}  size={len(f)} ({len(f):#x})")
    print("header u32[4..16]:", [hex(x) for x in struct.unpack_from('<12I', f, 0x10)])
    print("tail 32B:", f[-32:].hex())
    for m in [b'SUNP BURN FILE', b'DRAM CODE', b'SPCA6500', b'\x55\xaa']:
        offs=[x.start() for x in re.finditer(re.escape(m), f)]
        if m==b'\x55\xaa': offs=[o for o in offs if o%512==510][:10]
        print(f"  {m!r}: {[hex(o) for o in offs[:12]]}")
    # entropy map, 256 KiB blocks
    B=0x40000; row=[]
    for i in range(0,len(f),B):
        blk=f[i:i+B]; e=ent(blk); z=blk.count(0)/len(blk); ffc=blk.count(0xff)/len(blk)
        tag='F' if ffc>0.9 else ('0' if z>0.9 else ('Z' if e>7.8 else ('c' if e>6 else 'd')))
        row.append(tag)
    print("  entropy map (256K/char; Z=compressed/encrypted c=code-ish d=data 0=zero F=0xFF):")
    s=''.join(row); print('   ', '\n    '.join(s[i:i+32] for i in range(0,len(s),32)))
