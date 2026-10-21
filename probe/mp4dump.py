import struct, sys, re

CONTAINERS = {b'moov', b'trak', b'mdia', b'minf', b'stbl', b'udta', b'edts', b'dinf', b'meta', b'ilst'}

def walk(f, start, end, depth):
    pos = start
    while pos + 8 <= end:
        f.seek(pos)
        hdr = f.read(8)
        size, typ = struct.unpack('>I4s', hdr)
        hlen = 8
        if size == 1:
            size = struct.unpack('>Q', f.read(8))[0]; hlen = 16
        elif size == 0:
            size = end - pos
        if size < hlen:
            print('  ' * depth + f'!! bad atom size at {pos}'); return
        name = typ.decode('latin1')
        print('  ' * depth + f'{name} size={size} @{pos}')
        body_start = pos + hlen
        if typ in CONTAINERS:
            off = 4 if typ == b'meta' else 0
            walk(f, body_start + off, pos + size, depth + 1)
        elif typ != b'mdat' and size < 4096:
            f.seek(body_start)
            data = f.read(size - hlen)
            if typ == b'stsd':
                fmt = data[12:16]
                print('  ' * (depth + 1) + f'codec={fmt!r}')
                if fmt in (b'avc1', b'hvc1', b'hev1'):
                    w, h = struct.unpack('>HH', data[8 + 4 + 24 + 8: 8 + 4 + 24 + 12])
                    cname = data[8 + 4 + 42 + 8: 8 + 4 + 42 + 8 + 32]
                    print('  ' * (depth + 1) + f'{w}x{h} compressor={cname[1:1 + cname[0]]!r}')
                    i = data.find(b'avcC')
                    if i >= 0:
                        print('  ' * (depth + 1) + f'avcC profile={data[i + 5]} level={data[i + 7]}')
            txt = re.findall(rb'[\x20-\x7e]{4,}', data)
            if txt and typ not in (b'stts', b'stsz', b'stco', b'co64', b'stsc', b'stss'):
                print('  ' * (depth + 1) + 'strings: ' + ' | '.join(t.decode() for t in txt[:12]))
            if typ in (b'mvhd', b'tkhd', b'mdhd') or size < 200:
                print('  ' * (depth + 1) + 'hex: ' + data[:96].hex())
        pos += size

path = sys.argv[1]
with open(path, 'rb') as f:
    f.seek(0, 2); n = f.tell()
    walk(f, 0, n, 0)
    # strings in mdat head (some SoCs put SEI user data with encoder name)
    f.seek(0); head = f.read(262144)
    print('--- interesting strings in first 256K ---')
    seen = set()
    for t in re.findall(rb'[\x20-\x7e]{6,}', head):
        if t not in seen:
            seen.add(t); print(t.decode()[:120])
        if len(seen) > 60: break
