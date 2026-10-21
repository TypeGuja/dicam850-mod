import sys
brn=open(sys.argv[1],'rb').read(); dump=open(sys.argv[2],'rb').read()
def runs_for(delta, lo=0, hi=None):
    hi=hi or len(brn); out=[]; s=None
    B=0x100
    for b in range(lo,hi,B):
        d=b+delta
        ok = 0<=d and d+B<=len(dump) and brn[b:b+B]==dump[d:d+B]
        if ok and s is None: s=b
        if not ok and s is not None: out.append((s,b)); s=None
    if s is not None: out.append((s,hi))
    return out
# candidate deltas from shared magics
for name,delta in [("DRAM CODE",0x48804e-0x52836e),("SUNP#2",0xe1eb38-0x97a36),("SUNP#3",0xf2190c-0xebce58),("SUNP#2b",0xe1eb38-0xebce58)]:
    r=[x for x in runs_for(delta) if x[1]-x[0]>=0x1000]
    tot=sum(b-a for a,b in r)
    print(f"{name}: delta {delta:+#x}  matched {tot:#x} bytes")
    for a,b in r[:8]: print(f"   BRN {a:#09x}-{b:#09x} -> FLASH {a+delta:#09x}  len {b-a:#x}")
