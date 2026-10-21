//! Runtime SD-card config loader for the DiCam 850 (injected firmware hook).
//!
//! Reads `G:\DICAM.CFG` (plain `key=value` text) at settings-apply time and
//! patches live firmware tables in RAM. No file / bad value -> no-op.
//! Written panic-free with raw pointers (no slice bounds checks) so the
//! compiled .text is small and self-contained — no panic handler, no .rodata.
//! Compiled for `armv7a-none-eabihf`; position-independent (firmware entry
//! points are absolute constants in the literal pool, strings built on the
//! stack), so the bytes can be copied into free image space and reached by a
//! redirected `BL`.
#![allow(clippy::missing_safety_doc)]

// ---- fixed firmware addresses (rev4 2023-04-22) ----
const FS_READ: usize = 0x40689068; // fsSimpleRead(path, buf, size) -> i32 bytecount
const BUF: usize = 0x409ffb00; // scratch in the 16 KB zero gap @0x409ffa83
const BUF_LEN: u32 = 2048;

const MODE_TABLE: usize = 0x409aa060; // video-mode table, 0x40-byte records
const MODE_STRIDE: usize = 0x40;
const N_MODES: usize = 10;
const OFF_W: usize = 0x00; // u16
const OFF_H: usize = 0x02; // u16
const OFF_FPS: usize = 0x0c; // u32
const OFF_BR: usize = 0x14; // u32 (High-preset Mbit)

type FsRead = extern "C" fn(*const u8, *mut u8, u32) -> i32;

/// Scan `n` bytes at `buf` for `key` (len `klen`); return the unsigned decimal
/// that follows it, or `u32::MAX` if not found / no digits. Pure pointer math,
/// no bounds-checked indexing (so no panic paths).
unsafe fn find_val(buf: *const u8, n: usize, key: *const u8, klen: usize) -> u32 {
    if klen == 0 || n < klen {
        return u32::MAX;
    }
    let mut i = 0usize;
    while i + klen <= n {
        let mut k = 0usize;
        while k < klen && *buf.add(i + k) == *key.add(k) {
            k += 1;
        }
        if k == klen {
            let mut j = i + klen;
            let mut v: u32 = 0;
            let mut have = false;
            while j < n {
                let c = *buf.add(j);
                if c < b'0' || c > b'9' {
                    break;
                }
                v = v.wrapping_mul(10).wrapping_add((c - b'0') as u32);
                have = true;
                j += 1;
            }
            if have {
                return v;
            }
        }
        i += 1;
    }
    u32::MAX
}

/// Base address of the mode record matching width `w` @ fps `fps`, or 0.
unsafe fn mode_rec(w: u16, fps: u32) -> usize {
    let mut i = 0usize;
    while i < N_MODES {
        let base = MODE_TABLE + i * MODE_STRIDE;
        if core::ptr::read_volatile((base + OFF_W) as *const u16) == w
            && core::ptr::read_volatile((base + OFF_FPS) as *const u32) == fps
        {
            return base;
        }
        i += 1;
    }
    0
}

#[no_mangle]
pub extern "C" fn dicam_cfg_apply() {
    unsafe {
        // "G:\DICAM.CFG\0" built on the stack
        let path: [u8; 13] = [71, 58, 92, 68, 73, 67, 65, 77, 46, 67, 70, 71, 0];
        let fsread: FsRead = core::mem::transmute::<usize, FsRead>(FS_READ);
        let n = fsread(path.as_ptr(), BUF as *mut u8, BUF_LEN);
        if n <= 0 {
            return;
        }
        let n = n as usize;

        // bitrate4k=NN  -> 4K30 (2160@30) High-preset Mbit  (10..=200)
        let k_br: [u8; 10] = *b"bitrate4k=";
        let v = find_val(BUF as *const u8, n, k_br.as_ptr(), 10);
        if v >= 10 && v <= 200 {
            let base = mode_rec(3840, 30);
            if base != 0 {
                core::ptr::write_volatile((base + OFF_BR) as *mut u32, v);
            }
        }

        // aspect4k=NNN  (ratio x100, e.g. 235) -> crop 4K30 height  (100..=300)
        let k_ar: [u8; 9] = *b"aspect4k=";
        let ar = find_val(BUF as *const u8, n, k_ar.as_ptr(), 9);
        if ar >= 100 && ar <= 300 {
            let base = mode_rec(3840, 30);
            if base != 0 {
                let h = (((3840u32 * 100 / ar) / 16) * 16) as u16;
                core::ptr::write_volatile((base + OFF_H) as *mut u16, h);
            }
        }
    }
}
