#!/usr/bin/env python3
"""diprofile — bake a JSON recording profile into a DiCam 850 SPHOST.BRN.

Usage:
    python -I diprofile.py <profile.json> [--base clean.BRN] [--out SPHOST.BRN]

Applies custom recording settings (per-mode bitrate, custom resolution /
stretch) by editing the firmware's video-mode table @VA 0x409aa060 in the
main image, then fixes the BRN checksum. Pure image-table edits — safe,
same-size, reversible by reflashing the base.

This is the PATCH-TIME version of the "JSON profiles" idea (profile on PC ->
baked firmware). The runtime "camera reads JSON from the SD card" version needs
a JSON parser injected into the firmware (later, strangler stage).

Profile JSON schema (all keys optional):
{
  "name": "my-profile",
  "bitrate_scale": 1.5,                 # multiply every mode's 3 bitrates
  "bitrate_mbit": {                     # per-mode override, [High,Std,Low] or one int (High only)
     "3840x2160@30": [60, 52, 45],
     "1920x1080@30": 30
  },
  "resolution": {                       # EXPERIMENTAL: custom output w/h (stretch/scale)
     "2704x1520@60": {"width": 2560, "height": 1440}
  },
  "vbrc_cap_kbps": 120000               # raise VBR caps so they never clamp (default auto)
}
Mode keys are "WIDTHxHEIGHT@FPS" as the stock table reports them (run with
--list to print them).
"""
import struct, json, sys, os

IMG_BASE_OFF = 0x4b8120      # main image offset in the BRN
VA_BASE      = 0x40000000
MODE_TBL_VA  = 0x409aa060    # video-mode table
MODE_STRIDE  = 0x40
VBRC_CAPS_VA = [0x407c4420, 0x407c443c, 0x407c4458]  # kbps fields to lift

def va2off(va): return IMG_BASE_OFF + (va - VA_BASE)

def rd_u16(d, va): return struct.unpack_from('<H', d, va2off(va))[0]
def rd_u32(d, va): return struct.unpack_from('<I', d, va2off(va))[0]
def wr_u16(d, va, v): struct.pack_into('<H', d, va2off(va), v & 0xffff)
def wr_u32(d, va, v): struct.pack_into('<I', d, va2off(va), v & 0xffffffff)

def modes(d):
    """Discover video modes in the table: list of (va, w, h, fps)."""
    out = []
    va = MODE_TBL_VA
    for _ in range(32):
        w = rd_u16(d, va); h = rd_u16(d, va + 2); fps = rd_u32(d, va + 0xc)
        if w == 0 or h == 0 or fps == 0 or fps > 1000:
            break
        out.append((va, w, h, fps))
        va += MODE_STRIDE
    return out

def key(w, h, fps): return f"{w}x{h}@{fps}"

def fix_checksum(d):
    struct.pack_into('<I', d, 0x1fc, 0)
    n = len(d) // 4
    s = 0
    for i in range(n):
        s = (s + struct.unpack_from('<I', d, i * 4)[0]) & 0xffffffff
    struct.pack_into('<I', d, 0x1fc, (-s) & 0xffffffff)

def checksum_ok(d):
    return sum(struct.unpack_from('<I', d, i * 4)[0] for i in range(len(d) // 4)) & 0xffffffff == 0

def expand_formats(prof):
    """Translate a 'formats' list (named custom formats on slots) into the
    resolution/bitrate override dicts, collecting a summary + warnings."""
    res_ov = dict(prof.get("resolution", {}))
    br_ov = dict(prof.get("bitrate_mbit", {}))
    summary = []
    for f in prof.get("formats", []):
        slot = f["slot"]
        r = {}
        if "aspect" in f: r["aspect"] = f["aspect"]
        if "width" in f:  r["width"] = f["width"]
        if "height" in f: r["height"] = f["height"]
        if r: res_ov[slot] = r
        if "bitrate" in f: br_ov[slot] = f["bitrate"]
        summary.append((slot, f.get("name", ""), r, f.get("bitrate")))
    return res_ov, br_ov, summary

def apply(d, prof):
    log = []
    ms = modes(d)
    by_key = {key(w, h, fps): va for va, w, h, fps in ms}
    scale = prof.get("bitrate_scale")
    res_ov, br_ov, fmt_summary = expand_formats(prof)
    # validate custom formats against the slot's stock (sensor) envelope
    for slot, r in res_ov.items():
        if slot not in by_key:
            log.append(f"  WARNING: slot '{slot}' not found (see --list)")
            continue
        va = by_key[slot]; bw = rd_u16(d, va); bh = rd_u16(d, va + 2)
        nw = r.get("width", bw)
        nh = int(round(nw / float(r["aspect"]) / 16.0)) * 16 if "aspect" in r else r.get("height", bh)
        if nw % 16 or nh % 16:
            log.append(f"  WARNING: {slot} {nw}x{nh} not multiple of 16 (H.264 may reject)")
        if nw > bw or nh > bh:
            log.append(f"  WARNING: {slot} {nw}x{nh} EXCEEDS sensor readout {bw}x{bh} (upscale/unsupported — likely broken)")
    max_br = 0
    for va, w, h, fps in ms:
        k = key(w, h, fps)
        trip_va = [va + 0x14, va + 0x18, va + 0x1c]
        cur = [rd_u32(d, a) for a in trip_va]
        new = list(cur)
        if scale:
            new = [max(1, round(v * scale)) for v in new]
        if k in br_ov:
            o = br_ov[k]
            if isinstance(o, int):
                new[0] = o
            else:
                for i, v in enumerate(o[:3]):
                    new[i] = v
        if new != cur:
            for a, v in zip(trip_va, new):
                wr_u32(d, a, v)
            log.append(f"  {k:18s} bitrate {cur} -> {new} Mbit")
        max_br = max(max_br, *new)
        if k in res_ov:
            r = res_ov[k]
            nw = r.get("width", w)
            if "aspect" in r:
                # cinematic crop: keep width, derive height from aspect, round to mult of 16
                nh = int(round(nw / float(r["aspect"]) / 16.0)) * 16
            else:
                nh = r.get("height", h)
            wr_u16(d, va, nw); wr_u16(d, va + 2, nh)
            ar = nw / nh if nh else 0
            log.append(f"  {k:18s} resolution {w}x{h} -> {nw}x{nh} ({ar:.3f}:1) (EXPERIMENTAL)")
    # lift VBRC caps so they never clamp the new target
    cap = prof.get("vbrc_cap_kbps", max(80000, (max_br + 20) * 1000))
    for cva in VBRC_CAPS_VA:
        if rd_u32(d, cva) < cap:
            old = rd_u32(d, cva); wr_u32(d, cva, cap)
            log.append(f"  VBRC cap @{cva:#x} {old} -> {cap} kbps")
    # final-state summary: the resulting recording menu
    log.append("  --- resulting recording menu ---")
    for va, _w, _h, fps in modes(d):
        w = rd_u16(d, va); h = rd_u16(d, va + 2)
        br = rd_u32(d, va + 0x14)
        ar = w / h if h else 0
        log.append(f"    {w}x{h}@{fps:<3d} {ar:.2f}:1   {br} Mbit")
    return log

BRNTOOL = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "brntool", "target", "release", "brntool.exe")
ISO100_GAIN = 96     # gain byte of the ISO100 charts (anchor)
GAIN_PER_EV = 16     # assumed chart gain units per stop

def iso_to_gain(iso):
    import math
    return max(ISO100_GAIN, int(round(ISO100_GAIN + GAIN_PER_EV * math.log2(iso / 100.0))))

def cap_pchart(data, cap):
    """Clamp the gain byte (+4) of every 6-byte AE program-chart row to <= cap."""
    d = bytearray(data); changed = 0
    for i in range(0, len(d) - 5, 6):
        if d[i] == 0 and d[i + 2] == 0 and d[i + 3] == 0 and d[i + 4] == 0:
            break
        if d[i + 4] > cap:
            d[i + 4] = cap; changed += 1
    return bytes(d), changed

def apply_resources(out, prof, log):
    """Resource-level edits (RO_RES files) via brntool: max_iso -> video AE charts."""
    cap = prof.get("max_gain")
    if cap is None and "max_iso" in prof:
        cap = iso_to_gain(prof["max_iso"])
    if cap is None:
        return
    import tempfile, subprocess, shutil, glob
    tmp = tempfile.mkdtemp(prefix="diprof_")
    try:
        subprocess.run([BRNTOOL, "unpack", out, tmp], check=True, capture_output=True)
        cur = out
        for p in sorted(glob.glob(os.path.join(tmp, "RO_RES", "IQ", "AEPCHART*.BIN"))):
            name = os.path.basename(p)
            if "CAP" in name:          # photo/capture charts: leave alone
                continue
            new, n = cap_pchart(open(p, "rb").read(), cap)
            if not n:
                continue
            open(p, "wb").write(new)
            nxt = out + ".tmp"
            subprocess.run([BRNTOOL, "replace", cur, "RO_RES/IQ/" + name, p, nxt], check=True, capture_output=True)
            shutil.move(nxt, out); cur = out
            log.append(f"  {name:28s} gain capped at {cap} ({n} rows)")
        iso = 100 * 2 ** ((cap - ISO100_GAIN) / GAIN_PER_EV)
        log.append(f"  video max ISO ~{iso:.0f} (gain {cap})")
    finally:
        shutil.rmtree(tmp, ignore_errors=True)

def main():
    a = sys.argv[1:]
    if not a:
        print(__doc__); return 2
    base = os.path.join(os.path.dirname(__file__), "..", "flash_ready", "SPHOST.BRN")
    out = os.path.join(os.path.dirname(__file__), "SPHOST.BRN")
    prof_path = None
    i = 0
    while i < len(a):
        if a[i] == "--base": base = a[i + 1]; i += 2
        elif a[i] == "--out": out = a[i + 1]; i += 2
        elif a[i] == "--list": prof_path = "--list"; i += 1
        else: prof_path = a[i]; i += 1
    d = bytearray(open(base, "rb").read())
    if not checksum_ok(d):
        print(f"WARNING: base {base} checksum invalid", file=sys.stderr)
    if prof_path == "--list":
        print("video modes in", base, ":")
        for va, w, h, fps in modes(d):
            trip = [rd_u32(d, va + 0x14), rd_u32(d, va + 0x18), rd_u32(d, va + 0x1c)]
            print(f"  {key(w,h,fps):18s} bitrate(H/S/L) = {trip} Mbit")
        return 0
    prof = json.load(open(prof_path, encoding="utf-8"))
    print(f"profile: {prof.get('name', prof_path)}  base: {base}")
    log = apply(d, prof)
    if not log:
        print("  (no changes)")
    else:
        print("\n".join(log))
    fix_checksum(d)
    assert checksum_ok(d), "checksum fix failed"
    open(out, "wb").write(d)
    rlog = []
    apply_resources(out, prof, rlog)
    if rlog:
        print("\n".join(rlog))
    d = open(out, "rb").read()
    assert checksum_ok(d), "checksum invalid after resource edits"
    diff = sum(1 for x, y in zip(open(base,'rb').read(), d) if x != y)
    print(f"wrote {out}  (checksum VALID, {diff} bytes changed vs base)")
    return 0

if __name__ == "__main__":
    sys.exit(main())
