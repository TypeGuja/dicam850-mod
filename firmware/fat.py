import sys,struct
f=open(sys.argv[1],'rb').read()
# find FAT boot sectors: 0x55AA at off+510, and BPB bytesPerSec=512
cands=[]
for i in range(0,len(f)-512,512):
    if f[i+510:i+512]==b'\x55\xaa' and f[i:i+1] in (b'\xeb',b'\xe9') :
        bps=struct.unpack_from('<H',f,i+11)[0]
        spc=f[i+13]; rsv=struct.unpack_from('<H',f,i+14)[0]; nfat=f[i+16]
        rootent=struct.unpack_from('<H',f,i+17)[0]; spf=struct.unpack_from('<H',f,i+22)[0]
        if bps in (512,) and spc in (1,2,4,8,16,32,64) and nfat in (1,2):
            cands.append((i,bps,spc,rsv,nfat,rootent,spf))
print("FAT boot sectors found:",len(cands))
for c in cands[:20]:
    i,bps,spc,rsv,nfat,rootent,spf=c
    oem=f[i+3:i+11].decode('latin1')
    print(f"  @{i:#x} oem={oem!r} bps={bps} spc={spc} rsvd={rsv} nFAT={nfat} rootEnt={rootent} secPerFAT={spf}")
