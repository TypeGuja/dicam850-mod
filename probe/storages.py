# Read-only: list PTP storages and objects (GetStorageIDs/GetStorageInfo/GetObjectHandles/GetObjectInfo)
import struct, sys, json
import os; sys_path_dir = os.path.dirname(os.path.abspath(__file__))
sys.argv = ['x', '192.168.1.1']
sys.path.insert(0, sys_path_dir); import camprobe as c
p = c.PTPIP()
rc, _ = p.op(0x1002, 1); print('OpenSession', hex(rc))
rc, b = p.op(0x1004); ids = list(struct.unpack_from(f'<{struct.unpack_from("<I", b)[0]}I', b, 4)) if rc == 0x2001 else []
print('GetStorageIDs', hex(rc), [hex(i) for i in ids])
for sid in ids:
    rc, b = p.op(0x1005, sid)
    if rc == 0x2001:
        st, fs, acc, cap, free, freeimg = struct.unpack_from('<HHHQQI', b)
        o = 26; desc, o = c.ptp_str(b, o); label, o = c.ptp_str(b, o)
        print(f'storage {sid:#x}: type={st} fs={fs} access={acc} cap={cap} free={free} desc={desc!r} label={label!r}')
    rc, b = p.op(0x1007, sid, 0, 0)   # all objects, all formats, whole tree
    if rc != 0x2001: print('  GetObjectHandles', hex(rc)); continue
    n = struct.unpack_from('<I', b)[0]; hs = struct.unpack_from(f'<{n}I', b, 4)
    print(f'  {n} objects')
    for h in hs[:200]:
        rc, oi = p.op(0x1008, h)
        if rc != 0x2001: print('   ', hex(h), hex(rc)); continue
        fmt = struct.unpack_from('<H', oi, 4)[0]; size = struct.unpack_from('<I', oi, 8)[0]
        parent = struct.unpack_from('<I', oi, 38)[0]
        name, _ = c.ptp_str(oi, 52)
        print(f'    h={h:#x} parent={parent:#x} fmt={fmt:#06x} size={size} {name}')
p.op(0x1003)
