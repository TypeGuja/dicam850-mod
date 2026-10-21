import struct, re, sys
BASE=0x40000000; img=open('main.bin','rb').read(); n=len(img)//4
words=struct.unpack_from(f'<{n}I', img)
def users(pool):
    out=[]
    lo=max(0,(pool-BASE)//4-1100); hi=min(n,(pool-BASE)//4+1100)
    for i in range(lo,hi):
        w=words[i]
        if (w & 0x0f7f0000)==0x051f0000:
            imm=w&0xfff; addr=BASE+i*4+8+(imm if w&(1<<23) else -imm)
            if addr==pool: out.append(BASE+i*4)
    return out
# all AE-ish path strings and their full-path starts
for m in re.finditer(rb"A:.RO_RES.IQ.(AE[A-Z0-9_]*|AWDRALG|AWB[A-Z]*)[.]BIN", img):
    va=BASE+m.start(); s=m.group().decode()
    pools=[BASE+i*4 for i,w in enumerate(words) if w==va]
    print(f'{s:42s} str={va:#x} pools={[hex(p) for p in pools]} code={[hex(u) for p in pools for u in users(p)]}')
