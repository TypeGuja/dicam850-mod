"""Emulate the patched rtcfg BRN hooks on the real main image (unicorn, ARMv7).

usage: py -I emu_test.py <pylib dir with unicorn> [BRN]
Runs the real patched instructions: appVideoPvStateInit entry @0x40046ce8 ->
video_wrap and the chart site @0x4006b9ec -> chart_thunk, both into the Rust
loader, with fsSimpleReadEx / free stubbed after the real firmware semantics.
Every memory write is watched: anything outside the stack, the stub heap
block, the mode table and the AE charts is a FAIL (rtcfg2..5 wrote into the
power manager's RAM @0x409ffa84 and nothing noticed).
"""
import os, struct, sys

sys.path.insert(0, sys.argv[1])
from unicorn import Uc, UC_ARCH_ARM, UC_MODE_ARM, UC_HOOK_CODE, UC_HOOK_MEM_WRITE
from unicorn.arm_const import *

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)
BRN = sys.argv[2] if len(sys.argv) > 2 else os.path.join(ROOT, "patches", "SPHOST_rtcfg9.BRN")
CHART = open(os.path.join(ROOT, "firmware", "rev4_rores", "RO_RES", "IQ", "AEPCHART60HZ30F_AUTO.BIN"), "rb").read()
IMG = open(BRN, "rb").read()[0x4B8120:]

FS_READ_EX, FREE = 0x406890E8, 0x40733CA4
MODE_TABLE, CHART_BUF, AE_CTX = 0x409AA060, 0x40A2DE60, 0x40A6B7B4
FAKE_CTX, HEAP, STACK, RET = 0x40C00000, 0x40E00000, 0x41000000, 0x40FFF000
fails = 0


def step_run(uc, start, stop, limit=200000):
    """Run one instruction per emu_start: unicorn 2.1.4 skipped a Thumb-2 cmp.w right
    after a not-taken conditional branch when it translated whole blocks."""
    uc.emu_start(start, 0xFFFFFFFF, count=1)
    for _ in range(limit):
        pc = uc.reg_read(UC_ARM_REG_PC)
        if pc == stop:
            return
        thumb = (uc.reg_read(UC_ARM_REG_CPSR) >> 5) & 1
        uc.emu_start(pc | thumb, 0xFFFFFFFF, count=1)
    raise RuntimeError('step limit')


def check(cond, msg):
    global fails
    print(("  ok   " if cond else "  FAIL ") + msg)
    fails += not cond


def gains(b):
    out = []
    for i in range(0, 180, 6):
        if b[i] == 0 and b[i + 2:i + 5] == b"\0\0\0":
            break
        out.append(b[i + 4])
    return out


def modes(uc):
    r = {}
    for i in range(10):
        b = uc.mem_read(MODE_TABLE + i * 0x40, 0x20)
        w, h = struct.unpack_from("<HH", b, 0)
        fps, br = struct.unpack_from("<I", b, 0xC)[0], struct.unpack_from("<I", b, 0x14)[0]
        r[(w, fps)] = (h, br)
    return r


CAPTAB = 0x4075FFF8  # per-fps ISO cap table (2 x {u16 fps, u8 gaincap, u8 pad})
CUR_FPS = 0x409ACD10  # live video fps the firmware chart loader reads


class Machine:
    def __init__(self, cfg, fps=60):
        self.cfg, self.allocs, self.frees, self.bad_writes = cfg, [], [], []
        self.reads = 0
        uc = self.uc = Uc(UC_ARCH_ARM, UC_MODE_ARM)
        uc.ctl_set_cpu_model(UC_CPU_ARM_CORTEX_A7)  # the camera's core
        uc.mem_map(0x40000000, 0x1000000)
        uc.mem_write(0x40000000, IMG)
        uc.mem_write(CHART_BUF, CHART)
        uc.mem_write(FAKE_CTX + 428, CHART)
        uc.mem_write(AE_CTX, struct.pack("<IIII", FAKE_CTX, 0, 0xDEADBEEF, 0))
        uc.mem_write(CUR_FPS, struct.pack("<I", fps))
        uc.reg_write(UC_ARM_REG_SP, STACK - 0x100)
        uc.hook_add(UC_HOOK_CODE, self.stub_read, begin=FS_READ_EX, end=FS_READ_EX)
        uc.hook_add(UC_HOOK_CODE, self.stub_free, begin=FREE, end=FREE)
        uc.hook_add(UC_HOOK_MEM_WRITE, self.on_write)

    def ret(self, r0):
        self.uc.reg_write(UC_ARM_REG_R0, r0)
        self.uc.reg_write(UC_ARM_REG_PC, self.uc.reg_read(UC_ARM_REG_LR))

    def stub_read(self, uc, *_):
        # fsSimpleReadEx(path, &buf, &size): 0 = ok (buf = malloc(size), filled), 1 = fail
        self.reads += 1
        path = bytes(uc.mem_read(uc.reg_read(UC_ARM_REG_R0), 16)).split(b"\0")[0]
        assert path == b"G:\\DICAM.CFG", path
        if self.cfg is None:
            return self.ret(1)
        p = HEAP + 0x1000 * len(self.allocs)
        self.allocs.append(p)
        self.in_stub = True
        uc.mem_write(p, self.cfg)
        uc.mem_write(uc.reg_read(UC_ARM_REG_R1), struct.pack("<I", p))
        uc.mem_write(uc.reg_read(UC_ARM_REG_R2), struct.pack("<I", len(self.cfg)))
        self.in_stub = False
        self.ret(0)

    def stub_free(self, uc, *_):
        self.frees.append(uc.reg_read(UC_ARM_REG_R0))
        self.uc.reg_write(UC_ARM_REG_PC, self.uc.reg_read(UC_ARM_REG_LR))

    in_stub = False

    def on_write(self, uc, access, addr, size, value, _):
        if self.in_stub:
            return
        ok = (STACK - 0x10000 <= addr < STACK
              or any(p <= addr < p + 0x1000 for p in self.allocs)
              or any(MODE_TABLE + i * 0x40 + o <= addr < MODE_TABLE + i * 0x40 + o + n
                     for i in range(10) for o, n in ((0x02, 2), (0x14, 4)))
              or CHART_BUF <= addr < CHART_BUF + 180
              or FAKE_CTX + 428 <= addr < FAKE_CTX + 608
              or CAPTAB <= addr < CAPTAB + 8)  # our own cap table, inside the hook gap
        if not ok:
            self.bad_writes.append((hex(uc.reg_read(UC_ARM_REG_PC)), hex(addr), size))

    def run_pv(self):
        """Execute appVideoPvStateInit's patched entry until its 2nd insn."""
        uc = self.uc
        regs = [0x11, 0x22, 0x33, 0x44]
        for r, v in zip([UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3], regs):
            uc.reg_write(r, v)
        for r, v in [(UC_ARM_REG_R4, 0x4444), (UC_ARM_REG_R5, 0x5555), (UC_ARM_REG_R8, 0x8888)]:
            uc.reg_write(r, v)
        uc.reg_write(UC_ARM_REG_LR, 0x4004801C + 4)
        sp0 = uc.reg_read(UC_ARM_REG_SP)
        step_run(uc, 0x40046CE8, 0x40046CEC)
        check(uc.reg_read(UC_ARM_REG_PC) == 0x40046CEC, "pv: reached appVideoPvStateInit+4")
        got = [uc.reg_read(r) for r in [UC_ARM_REG_R0, UC_ARM_REG_R1, UC_ARM_REG_R2, UC_ARM_REG_R3]]
        check(got == regs, "pv: r0-r3 preserved")
        sp = uc.reg_read(UC_ARM_REG_SP)
        frame = struct.unpack("<6I", uc.mem_read(sp, 24))
        check(sp == sp0 - 24 and frame[0] == 0x4444 and frame[1] == 0x5555 and frame[4] == 0x8888
              and frame[5] == 0x40048020, "pv: displaced push {r4-r8,lr} frame exact")
        uc.reg_write(UC_ARM_REG_SP, sp0)

    def run_chart(self, fresh=True):
        uc = self.uc
        if fresh:
            uc.mem_write(CHART_BUF, CHART)
        uc.reg_write(UC_ARM_REG_R4, 0x4444)
        uc.reg_write(UC_ARM_REG_LR, 0x12345678)
        sp0 = uc.reg_read(UC_ARM_REG_SP)
        step_run(uc, 0x4006B9EC, 0x4006B9F0)
        check(uc.reg_read(UC_ARM_REG_R0) == 0, "chart site: r0 == 0 like the replaced MOV")
        check(uc.reg_read(UC_ARM_REG_R4) == 0x4444 and uc.reg_read(UC_ARM_REG_SP) == sp0
              and uc.reg_read(UC_ARM_REG_LR) != 0, "chart site: r4/sp intact")
        return gains(bytes(uc.mem_read(CHART_BUF, 180)))

    def run_search(self, target):
        """Call the firmware AE program-search FUN_4020fb40 on the current chart
        for a target EV; returns (A, B, gain) it would program."""
        uc = self.uc
        out = STACK - 0x80
        sp = STACK - 0x100
        uc.mem_write(sp, struct.pack("<II", out + 8, out + 12))  # stack args param5,param6
        uc.reg_write(UC_ARM_REG_SP, sp)
        uc.reg_write(UC_ARM_REG_R0, CHART_BUF)
        uc.reg_write(UC_ARM_REG_R1, target & 0xFFFFFFFF)
        uc.reg_write(UC_ARM_REG_R2, out)       # *A
        uc.reg_write(UC_ARM_REG_R3, out + 4)   # *B
        uc.reg_write(UC_ARM_REG_LR, RET)
        uc.emu_start(0x4020FB40, RET, count=100000)
        a, b, gain, _flag = struct.unpack("<IiII", uc.mem_read(out, 16))
        return a, b, gain

    def finish(self, what):
        check(not self.bad_writes, f"{what}: no writes outside allowed RAM {self.bad_writes[:5]}")
        check(sorted(self.frees) == sorted(self.allocs), f"{what}: every read buffer freed once "
              f"(allocs {len(self.allocs)}, frees {len(self.frees)})")


def captab(mc):
    return [struct.unpack("<HB", bytes(mc.uc.mem_read(CAPTAB + i * 4, 3))) for i in range(2)]


def sweep(mc):
    """Program-search over a wide target-EV range: the set of distinct (A,B,gain)
    outputs and the max gain returned."""
    outs = [mc.run_search(t) for t in range(-100, 400, 7)]
    return set(outs), max(o[2] for o in outs)


def clz_check():
    """Run the firmware's own __clzsi2 @0x4075fd4c on the patched image."""
    uc = Machine(None).uc
    bad = []
    for v in [1, 7, 8, 9, 15, 0x80, 0xF0, 0x1234, 0x8000, 0xFFFF, 0x80000, 0x7FFFFFFF, 0x80000000, 0xFFFFFFFF]:
        uc.reg_write(UC_ARM_REG_R0, v)
        uc.reg_write(UC_ARM_REG_LR, RET)
        uc.emu_start(0x4075FD4C, RET, count=100)
        got = uc.reg_read(UC_ARM_REG_R0)
        if got != 32 - v.bit_length():
            bad.append((hex(v), got))
    check(not bad, f"firmware __clzsi2 still correct {bad}")


print("[0] firmware code next to the hook")
clz_check()
orig = IMG[0x3E170:0x3E174]
check(orig == struct.pack("<I", 0xEB000000 | (((0x4003D8D0 - 0x4003E170 - 8) >> 2) & 0xFFFFFF)),
      "photo-app BL @0x4003e170 is stock again")

stock_g = gains(CHART)
print("stock chart gains:", stock_g)
# gain byte for ISO800 / ISO400 via the hook's formula (96 + 16/stop)
ISO800, ISO400 = 144, 128

print("[1] no DICAM.CFG")
mc = Machine(None, fps=60)
m0 = modes(mc.uc)
mc.run_pv()
check(modes(mc.uc) == m0, "mode table untouched")
check(captab(mc) == [(0, 0), (0, 0)], "no ISO cap cached")
check(mc.run_chart() == stock_g, "chart reload unchanged")
check(mc.reads == 1, "no file: cfg read once (at pv entry), chart_thunk did NOT read")
mc.finish("no file")

# per-fps ISO: cap 30fps at ISO800, 60fps at ISO400, leave others alone
cfg = (b"# comment line\r\n"
       b"mode=3840,30,90,235,800\r\n"    # 4K30: crop+bitrate, ISO cap 800 for 30fps
       b"mode=2704,60,70,1152,400\n"     # 2.7K60: ISO cap 400 for 60fps
       b"mode=1920,120,45,0\n"           # 1080p120: bitrate only, no ISO cap
       b"mode=1920,30,0,185\n"           # 1080p30: crop only (shares the 30fps cap above)
       b"mode=9999,30,50,235,800\n")     # no such slot: formats skipped, cap still none-for-it

print("[2] formats applied (video-preview entry)")
mc = Machine(cfg, fps=30)
mc.run_pv()
m = modes(mc.uc)
for k, v in sorted(m.items()):
    if v != m0[k]:
        print(f"    {k[0]}@{k[1]}: h {m0[k][0]}->{v[0]}  br {m0[k][1]}->{v[1]}")
check(m[(3840, 30)] == (1632, 90), "4K30 -> 3840x1632 @90")
check(m[(2704, 60)] == (1152, 70), "2.7K60 -> 2704x1152 @70")
check(m[(1920, 120)] == (m0[(1920, 120)][0], 45), "1080p120 bitrate 45, height kept")
check(m[(1920, 30)] == (1024, m0[(1920, 30)][1]), "1080p30 -> 1920x1024 (1.85), bitrate kept")
check(captab(mc) == [(30, ISO800), (60, ISO400)], f"per-fps caps stored {captab(mc)}")
check(mc.reads == 1, "cfg read once at pv entry")
mc.finish("formats")

print("[3] ISO cap applies only to the matching fps (chart_thunk, no I/O)")
# 30fps: chart trimmed so no gain > 144; 60fps in the SAME cfg: capped at 128;
# 120fps: not in cfg -> untouched.
for fps, cap in [(30, ISO800), (60, ISO400), (120, None)]:
    mc = Machine(cfg, fps=fps)
    mc.run_pv()
    r0 = mc.reads
    g = mc.run_chart()
    maxg = max(g)
    if cap is None:
        check(g == stock_g, f"{fps}fps: no cap -> chart unchanged {g}")
    else:
        check(maxg <= cap, f"{fps}fps: chart trimmed, max gain {maxg} <= {cap}")
        check(g == [x for x in stock_g if x <= cap], f"{fps}fps: over-cap rows dropped {g}")
    check(mc.reads == r0, f"{fps}fps: chart_thunk did NO file I/O")
    mc.finish(f"{fps}fps")

print("[4] AE program-search stays responsive on the trimmed chart (freeze fix)")
# stock vs trimmed at 60fps (cap 400=128): the search must still return a RANGE of
# outputs across target EVs and never a gain above the cap. A stuck search (the old
# flatten bug) would collapse to a single output.
base = Machine(cfg, fps=60); base.cfg = None  # reuse image, no trim
stock_set, stock_max = sweep(base)
mc = Machine(cfg, fps=60); mc.run_pv(); mc.run_chart()
trim_set, trim_max = sweep(mc)
print(f"    stock: {len(stock_set)} distinct outputs, max gain {stock_max}; "
      f"trimmed: {len(trim_set)} distinct, max gain {trim_max}")
check(len(stock_set) >= 3, "stock search is responsive (sanity)")
check(len(trim_set) >= 3, "trimmed search still responsive -> AE not stuck")
check(trim_max <= ISO400, f"trimmed search never exceeds the cap (max {trim_max})")

print("[5] real sample runtime/DICAM.CFG")
mc = Machine(open(os.path.join(HERE, "DICAM.CFG"), "rb").read(), fps=30)
mc.run_pv()
print("   ", {k: (m0[k], v) for k, v in modes(mc.uc).items() if v != m0[k]}, captab(mc))
mc.run_chart()
mc.finish("sample cfg")

print("[5b] trailing '# ...' comment is not read as MAXISO (rtcfg8 bug)")
# rtcfg8 parsed "mode=1920,60,40,0   # 1080p 60" as MAXISO=1080 -> 60fps capped -> dark.
mc = Machine(b"mode=1920,60,40,0        # 1080p 60\n"
             b"mode=1920,30,30,0        # 1080p 30\n", fps=60)
mc.run_pv()
check(captab(mc) == [(0, 0), (0, 0)], f"comment digits ignored, no ISO cap {captab(mc)}")
check(modes(mc.uc)[(1920, 60)] == (m0[(1920, 60)][0], 40), "1080p60 bitrate still applied")
check(mc.run_chart() == stock_g, "60fps chart unchanged")
mc.finish("trailing comment")

print("[6] oversized file (>8192 B) is ignored but still freed")
mc = Machine(b"mode=3840,30,90,235\n" + b"#" * 9000, fps=30)
mc.run_pv()
check(modes(mc.uc) == m0, "oversized: untouched")
mc.finish("oversized")

print("FAILURES:", fails)
sys.exit(1 if fails else 0)
