import struct, re
BASE=0x40000000; img=open('main.bin','rb').read(); n=len(img)//4
W=struct.unpack_from(f'<{n}I',img)
def u32s(va,k): return struct.unpack_from(f'<{k}I',img,va-BASE)
print('EV table @0x407b4388+0x2c (13):', u32s(0x407b4388+0x2c,13))
for a in (0x407b43e8,0x407b4430,0x407b4478): print(f'target scale Q10 @{a:#x}:', u32s(a,18))
# all BL to target fn; recover r1 constant from preceding mov r1,#imm
def bl_target(i,w):
    if (w & 0x0f000000)!=0x0b000000: return None
    off=w & 0xffffff
    if off & 0x800000: off-=0x1000000
    return BASE+i*4+8+off*4
def imm_mov(w,rd):
    if (w & 0x0fef0000)==0x03a00000 and ((w>>12)&15)==rd:
        rot=((w>>8)&15)*2; v=w&0xff; return ((v>>rot)|(v<<(32-rot)))&0xffffffff
    if (w & 0x0ff00000)==0x03000000 and ((w>>12)&15)==rd: return ((w>>4)&0xf000)|(w&0xfff)
    return None
import collections
calls=collections.defaultdict(list)
for fn in (0x4020f624,):
    for i,w in enumerate(W):
        if bl_target(i,w)==fn or ((w&0x0f000000)==0x0a000000 and bl_target(i,(w&0xf0ffffff)|0x0b000000)==fn):
            r1=r0=None
            for j in range(i-1,max(i-14,0),-1):
                if r1 is None: r1=imm_mov(W[j],1)
                if r0 is None: r0=imm_mov(W[j],0)
            calls[r1].append((BASE+i*4,r0))
for k in sorted(calls, key=lambda x:(x is None,x)):
    v=calls[k]; print(f'param {k!s:>5}: {len(v):3d} calls  e.g. {[hex(a) for a,_ in v[:6]]}')
