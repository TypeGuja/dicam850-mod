import subprocess, sys, struct, re
BASE=0x40000000; img=open('main.bin','rb').read()
LB=r'C:\Users\user\.rustup\toolchains\stable-x86_64-pc-windows-msvc\lib\rustlib\x86_64-pc-windows-msvc\bin\llvm-objdump.exe'
lo,hi=int(sys.argv[1],16),int(sys.argv[2],16)
# objcopy wrap exists as main.elf
out=subprocess.run([LB,'-d','--triple=armv7a-none-eabi','--mcpu=cortex-a7',f'--start-address={lo-BASE:#x}',f'--stop-address={hi-BASE:#x}','main.elf'],capture_output=True,text=True).stdout
def u32(va): return struct.unpack_from('<I',img,va-BASE)[0] if 0<=va-BASE<len(img)-4 else None
def cstr(va):
    o=va-BASE
    if not(0<=o<len(img)): return None
    e=img.find(b'\0',o); s=img[o:e]
    return s.decode('latin1') if 3<=len(s)<96 and all(32<=c<127 or c in(9,) for c in s) else None
for line in out.splitlines():
    m=re.match(r'\s*([0-9a-f]+):\s+([0-9a-f]{8})\s+(.*)',line)
    if not m: continue
    va=BASE+int(m.group(1),16); asm=re.sub(r'\s+@ imm = #\S+','',m.group(3).split('<')[0].rstrip())
    note=''
    lm=re.search(r'ldr\s+\w+,\s*\[pc,\s*#(-?\d+|-?0x[0-9a-f]+)\]',asm)
    if lm:
        v=u32(va+8+int(lm.group(1),0))
        if v is not None:
            s=cstr(v); note=f'  ; ={v:#x}'+(f' \"{s.strip()}\"' if s else '')
    bm=re.match(r'(bl|b|blx)\s+0x([0-9a-f]+)',asm)
    if bm: asm=f'{bm.group(1)}\t{BASE+int(bm.group(2),16):#x}'
    print(f'{va:08x}: {asm}{note}')
