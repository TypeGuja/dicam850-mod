"""Read-only probe of an action camera over Wi-Fi.
Ports scan, HTTP/RTSP banners, PTP/IP GetDeviceInfo + GetDevicePropDesc for every property.
Never sends anything that changes state (no SetDeviceProp, no capture, no delete)."""
import socket, struct, sys, uuid, json

HOST = sys.argv[1] if len(sys.argv) > 1 else '192.168.1.1'
SRC = (sys.argv[2], 0) if len(sys.argv) > 2 else None  # bind to Wi-Fi IP to bypass VPN routes

def sock():
    s = socket.socket()
    if SRC: s.bind(SRC)
    return s
PORTS = [21, 22, 23, 53, 80, 443, 554, 8080, 8192, 8554, 9876, 15740, 6666, 7878, 8787, 3333, 5555]

def scan():
    open_ports = []
    for p in PORTS:
        s = sock(); s.settimeout(1.0)
        try:
            s.connect((HOST, p)); open_ports.append(p)
        except OSError:
            pass
        finally:
            s.close()
    print('open ports:', open_ports)
    return open_ports

def banner(port, payload):
    s = sock(); s.settimeout(2.0)
    try:
        s.connect((HOST, port))
        if payload: s.sendall(payload)
        data = s.recv(4096)
        print(f'--- port {port} ---\n' + data.decode('latin1', 'replace')[:1500])
    except OSError as e:
        print(f'--- port {port}: {e}')
    finally:
        s.close()

# ---------------- PTP/IP ----------------
def ptp_str(b, o):
    n = b[o]; o += 1
    s = b[o:o + n * 2].decode('utf-16le', 'replace').rstrip('\x00')
    return s, o + n * 2

def arr16(b, o):
    n = struct.unpack_from('<I', b, o)[0]; o += 4
    return list(struct.unpack_from(f'<{n}H', b, o)), o + n * 2

def recv_pkt(s):
    hdr = b''
    while len(hdr) < 8:
        c = s.recv(8 - len(hdr))
        if not c: raise EOFError
        hdr += c
    ln, typ = struct.unpack('<II', hdr)
    body = b''
    while len(body) < ln - 8:
        c = s.recv(ln - 8 - len(body))
        if not c: raise EOFError
        body += c
    return typ, body

class PTPIP:
    def __init__(self):
        guid = uuid.uuid4().bytes
        name = 'probe\x00'.encode('utf-16le')
        self.c = socket.create_connection((HOST, 15740), 5, source_address=SRC)
        pl = guid + name + struct.pack('<I', 0x00010000)
        self.c.sendall(struct.pack('<II', 8 + len(pl), 1) + pl)
        typ, body = recv_pkt(self.c)
        if typ != 2: raise RuntimeError(f'init refused type={typ} body={body.hex()}')
        conn = struct.unpack_from('<I', body)[0]
        rname = body[20:].decode('utf-16le', 'replace').split('\x00')[0]
        print(f'PTP/IP connected, conn={conn}, responder name={rname!r}')
        self.e = socket.create_connection((HOST, 15740), 5, source_address=SRC)
        self.e.sendall(struct.pack('<III', 12, 3, conn))
        typ, _ = recv_pkt(self.e)
        if typ != 4: raise RuntimeError(f'event init refused type={typ}')
        self.tid = 0

    def op(self, code, *params):
        self.tid += 1
        pl = struct.pack('<IHI', 1, code, self.tid) + b''.join(struct.pack('<I', p) for p in params)
        self.c.sendall(struct.pack('<II', 8 + len(pl), 6) + pl)
        data = b''
        while True:
            typ, body = recv_pkt(self.c)
            if typ == 9: continue                    # start data
            if typ in (10, 12): data += body[4:]    # data / end data
            if typ == 7:
                rc = struct.unpack_from('<H', body)[0]
                return rc, data

TYPES = {1: 'b', 2: 'B', 3: 'h', 4: 'H', 5: 'i', 6: 'I', 7: 'q', 8: 'Q'}

def read_val(b, o, dt):
    if dt == 0xFFFF:
        return ptp_str(b, o)
    if dt in TYPES:
        f = '<' + TYPES[dt]
        return struct.unpack_from(f, b, o)[0], o + struct.calcsize(f)
    if dt & 0x4000 and (dt & 0xFF) in TYPES:   # array
        n = struct.unpack_from('<I', b, o)[0]; o += 4
        f = '<' + TYPES[dt & 0xFF]; sz = struct.calcsize(f)
        return [struct.unpack_from(f, b, o + i * sz)[0] for i in range(n)], o + n * sz
    raise ValueError(f'dtype {dt:#x}')

def parse_propdesc(b):
    code, dt, rw = struct.unpack_from('<HHB', b, 0); o = 5
    d = {'code': f'{code:#06x}', 'type': f'{dt:#x}', 'rw': 'rw' if rw else 'ro'}
    try:
        d['default'], o = read_val(b, o, dt)
        d['current'], o = read_val(b, o, dt)
        form = b[o]; o += 1
        if form == 1:
            mn, o = read_val(b, o, dt); mx, o = read_val(b, o, dt); st, o = read_val(b, o, dt)
            d['range'] = [mn, mx, st]
        elif form == 2:
            n = struct.unpack_from('<H', b, o)[0]; o += 2
            vals = []
            for _ in range(n):
                v, o = read_val(b, o, dt); vals.append(v)
            d['enum'] = vals
    except Exception as e:
        d['raw'] = b.hex(); d['err'] = str(e)
    return d

def ptp_probe():
    p = PTPIP()
    rc, di = p.op(0x1001)                           # GetDeviceInfo
    print(f'GetDeviceInfo rc={rc:#x} len={len(di)}')
    o = 0
    std, vext, vver = struct.unpack_from('<HIH', di, o); o += 8
    vdesc, o = ptp_str(di, o)
    fmode = struct.unpack_from('<H', di, o)[0]; o += 2
    ops, o = arr16(di, o); evs, o = arr16(di, o); props, o = arr16(di, o)
    capf, o = arr16(di, o); imgf, o = arr16(di, o)
    man, o = ptp_str(di, o); model, o = ptp_str(di, o); ver, o = ptp_str(di, o); serial, o = ptp_str(di, o)
    info = dict(std=std, vendor_ext=f'{vext:#x}', vendor_ver=vver, vendor_desc=vdesc, functional_mode=fmode,
                manufacturer=man, model=model, device_version=ver, serial=serial,
                operations=[f'{x:#06x}' for x in ops], events=[f'{x:#06x}' for x in evs],
                properties=[f'{x:#06x}' for x in props])
    print(json.dumps(info, indent=1, ensure_ascii=False))
    rc, _ = p.op(0x1002, 1)                         # OpenSession
    print(f'OpenSession rc={rc:#x}')
    descs = []
    for pc in props:
        rc, b = p.op(0x1014, pc)                    # GetDevicePropDesc (read-only)
        d = parse_propdesc(b) if rc == 0x2001 and b else {'code': f'{pc:#06x}', 'rc': f'{rc:#x}'}
        print(json.dumps(d, ensure_ascii=False))
        descs.append(d)
    p.op(0x1003)                                    # CloseSession
    with open('camprobe_result.json', 'w', encoding='utf-8') as f:
        json.dump({'deviceinfo': info, 'props': descs}, f, indent=1, ensure_ascii=False)
    print('saved camprobe_result.json')

if __name__ == '__main__':
    ports = scan()
    if 80 in ports: banner(80, b'GET / HTTP/1.0\r\n\r\n')
    if 8080 in ports: banner(8080, b'GET / HTTP/1.0\r\n\r\n')
    if 554 in ports: banner(554, b'OPTIONS rtsp://%s/ RTSP/1.0\r\nCSeq: 1\r\n\r\n' % HOST.encode())
    for p in (21, 23):
        if p in ports: banner(p, None)
    if 15740 in ports: ptp_probe()
