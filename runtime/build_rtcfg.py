"""Build SPHOST_rtcfg9.BRN: runtime SD-card config (formats + video ISO cap).

usage: py -I build_rtcfg.py [base.BRN] [out.BRN]
Patches (each checks the original bytes first, aborts on mismatch):
  * hook.bin (runtime/hook crate, linked at 0x4075fd94) into the 620 B padding after
    the __clzsi2 lookup table (0x4075fd84..93 -- rtcfg2 overwrote its zero half: broken image)
  * PUSH @0x40046ce8 -> B video_wrap               (appVideoPvStateInit entry -> read cfg)
  * MOV r0,#0 @0x4006b9ec -> BL chart_thunk        (video AEPCHART loaded -> read cfg, clamp gain)
  * VBRC caps rec0..2 -> 160000 kbps so per-mode bitrates up to 160 Mbit are not clamped
then fixes the checksum with brntool and re-verifies everything from the output file.
History: rtcfg3 hooked only the photo app (BL @0x4003e170, dropped since rtcfg6);
rtcfg5 kept its file buffer + ISO global in the power manager's RAM @0x409ffa84..
(see hook/src/main.rs) -- rtcfg6 uses only the firmware heap.
"""
import os, struct, subprocess, sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
BRNTOOL = os.path.join(ROOT, "brntool", "target", "release", "brntool.exe")
BASE = sys.argv[1] if len(sys.argv) > 1 else os.path.join(ROOT, "patches", "SPHOST_smooth_br.BRN")
OUT = sys.argv[2] if len(sys.argv) > 2 else os.path.join(ROOT, "patches", "SPHOST_rtcfg9.BRN")
HOOK = os.path.join(HERE, "hook.bin")

IMG_OFF, VA0 = 0x4b8120, 0x40000000
GAP, GAP_LEN = 0x4075FD94, 620
VIDEO_WRAP, CHART_THUNK = 0x4075FD94, 0x4075FDA8
CLZ_TABLE = (0x4075FD84, bytes.fromhex("04030202010101010000000000000000"))
CHART_SITE = 0x4006B9EC
PV_INIT = 0x40046CE8
PV_PUSH = bytes.fromhex("f0412de9")  # push {r4-r8, lr}
VBRC_CAPS = [0x407C4420, 0x407C443C, 0x407C4458]
VBRC_CAP = 160000


def off(va):
    return IMG_OFF + va - VA0


def bl(src, dst, op=0xEB):
    return struct.pack("<I", op << 24 | (((dst - src - 8) >> 2) & 0xFFFFFF))


def b(src, dst):
    return bl(src, dst, 0xEA)


def expect(d, va, want, what):
    got = bytes(d[off(va):off(va) + len(want)])
    if got != want:
        sys.exit(f"ABORT {what} @{va:#x}: have {got.hex()} want {want.hex()}")


def main():
    d = bytearray(open(BASE, "rb").read())
    hook = open(HOOK, "rb").read()
    assert len(hook) <= GAP_LEN, len(hook)

    expect(d, *CLZ_TABLE, "__clzsi2 table")
    expect(d, GAP, b"\0" * GAP_LEN, "code gap not empty")
    expect(d, CHART_SITE, bytes.fromhex("0000a0e3"), "chart MOV r0,#0")
    expect(d, PV_INIT, PV_PUSH, "appVideoPvStateInit push")
    vw, ct = VIDEO_WRAP - GAP, CHART_THUNK - GAP
    assert hook[vw:vw + 4] == bytes.fromhex("0f502de9") and hook[vw + 12:vw + 16] == PV_PUSH, "video_wrap layout"
    assert hook[ct:ct + 4] == bytes.fromhex("10402de9"), "chart_thunk layout"

    d[off(GAP):off(GAP) + len(hook)] = hook
    d[off(CHART_SITE):off(CHART_SITE) + 4] = bl(CHART_SITE, CHART_THUNK)
    d[off(PV_INIT):off(PV_INIT) + 4] = b(PV_INIT, VIDEO_WRAP)
    for va in VBRC_CAPS:
        old = struct.unpack_from("<I", d, off(va))[0]
        if not 20000 <= old <= VBRC_CAP:
            sys.exit(f"ABORT VBRC cap @{va:#x} unexpected {old}")
        struct.pack_into("<I", d, off(va), VBRC_CAP)

    tmp = OUT + ".tmp"
    open(tmp, "wb").write(d)
    subprocess.run([BRNTOOL, "fix", tmp, OUT], check=True)
    os.remove(tmp)

    # re-verify from the written file
    o = open(OUT, "rb").read()
    base = open(BASE, "rb").read()
    assert len(o) == len(base)
    assert o[off(GAP):off(GAP) + len(hook)] == hook
    assert o[off(CLZ_TABLE[0]):off(CLZ_TABLE[0]) + 16] == CLZ_TABLE[1], "clz table damaged"
    assert o[off(CHART_SITE):off(CHART_SITE) + 4] == bl(CHART_SITE, CHART_THUNK)
    assert o[off(PV_INIT):off(PV_INIT) + 4] == b(PV_INIT, VIDEO_WRAP)
    s = sum(struct.unpack_from("<%dI" % (len(o) // 4), o)) + sum(o[len(o) // 4 * 4:])
    assert s & 0xFFFFFFFF == 0, "checksum"
    diff = [i for i in range(len(o)) if o[i] != base[i]]
    print(f"OK {OUT}: {len(diff)} bytes differ from base, checksum VALID")
    outside = [i for i in diff if not (
        off(GAP) <= i < off(GAP) + GAP_LEN
        or i in range(off(CHART_SITE), off(CHART_SITE) + 4) or i in range(0x1FC, 0x200)
        or i in range(off(PV_INIT), off(PV_INIT) + 4)
        or any(off(v) <= i < off(v) + 4 for v in VBRC_CAPS))]
    assert not outside, [hex(i) for i in outside]
    print("all changed bytes are inside the expected patch sites")


if __name__ == "__main__":
    main()
