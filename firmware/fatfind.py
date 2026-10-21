import sys,struct,re
f=open(sys.argv[1],'rb').read()
dot=b'.          \x10'
hits=[]
for m in re.finditer(re.escape(dot), f):
    o=m.start()
    if f[o+32:o+44]==b'..         \x10':
        cl=struct.unpack_from('<H',f,o+26)[0]; parent=struct.unpack_from('<H',f,o+32+26)[0]
        hits.append((o,cl,parent))
for o,cl,p in hits: print(f"dir @{o:#x} self_cluster={cl} parent={p}")
# solve base + cl*csize
if len(hits)>=2:
    (o1,c1,_),(o2,c2,_)=hits[0],hits[1]
    for a,b in [(hits[i],hits[j]) for i in range(len(hits)) for j in range(i+1,len(hits)) if hits[i][1]!=hits[j][1]][:6]:
        cs=(b[0]-a[0])/(b[1]-a[1]); base=a[0]-a[1]*cs
        print(f"  pair cl{a[1]}/cl{b[1]}: csize={cs} base={base:#x}" if cs==int(cs) else f"  pair non-int {cs}")
