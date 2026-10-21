import sys
brn=open(sys.argv[1],'rb').read(); dump=open(sys.argv[2],'rb').read()
B=0x1000
# index dump 4K blocks by content (skip trivial)
idx={}
for o in range(0,len(dump)-B+1,0x200):
    k=dump[o:o+64]
    if k.count(k[0])==64: continue
    idx.setdefault(k,[]).append(o)
runs=[]; cur=None
for bo in range(0,len(brn)-B+1,B):
    blk=brn[bo:bo+B]
    if blk.count(blk[0])==B: 
        cur=None; continue
    hit=None
    for do in idx.get(blk[:64],[]):
        if dump[do:do+B]==blk: hit=do; break
    if hit is None: cur=None; continue
    if cur and cur[1]+cur[2]==bo and cur[0]+cur[2]==hit: cur[2]+=B
    else:
        cur=[hit,bo,B]; runs.append(cur)
tot=sum(r[2] for r in runs)
print(f"matched {tot:#x} of {len(brn):#x} bytes ({tot*100//len(brn)}%) in {len(runs)} runs")
for d,b,l in runs:
    if l>=0x10000: print(f"  BRN {b:#09x}-{b+l:#09x}  ->  FLASH {d:#09x}  (len {l:#x}, delta {d-b:+#x})")
