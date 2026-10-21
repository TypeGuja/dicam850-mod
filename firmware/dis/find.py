import struct,re,sys
B=0x40000000; img=open('main.bin','rb').read(); n=len(img)//4; W=struct.unpack_from(f'<{n}I',img)
def users(pool):
    o=[]
    for i in range(max(0,(pool-B)//4-1600),min(n,(pool-B)//4+1600)):
        w=W[i]
        if (w&0x0f7f0000)==0x051f0000:
            imm=w&0xfff; a=B+i*4+8+(imm if w&(1<<23) else -imm)
            if a==pool:o.append(B+i*4)
    return o
key=sys.argv[1].encode()
for m in re.finditer(re.escape(key),img):
    s=m.start()
    while s>0 and 32<=img[s-1]<127: s-=1
    va=B+s; end=img.find(b'\x00',s); txt=img[s:end].decode('latin1')[:40]
    pools=[B+i*4 for i,w in enumerate(W) if w==va]
    print(f'  {va:#x} {txt!r} code={[hex(u) for p in pools for u in users(p)][:3]}')
