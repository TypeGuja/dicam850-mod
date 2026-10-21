//! iCatch "SUNP BURN FILE" firmware container (DiCam850 SPHOST.BRN).
//!
//! Layout (confirmed on all 5 DiCam850 BRNs):
//!   0x000  "SUNP BURN FILE", u32 @0x10 = total size, @0x14 = HDR1 off, @0x18 = HDR2 off,
//!          @0x1c = main-image off, u32 @0x1fc = checksum compensation word.
//!   HDR2+0x320 .. = an embedded FAT16 volume (RO_RES): FAT1, FAT2, root dir, data,
//!          512-byte clusters, 32-byte directory entries, VFAT long names.
//!
//! Integrity: the sum of every little-endian u32 in the whole file is 0 (mod 2^32);
//! the word at 0x1fc absorbs the difference. There is no signature.
//!
//! This library only does non-destructive edits: it replaces the bytes of an existing
//! resource file (new content must fit the file's current cluster chain) and fixes the
//! checksum. It never rebuilds the FAT, so everything else stays byte-identical.

use std::collections::BTreeMap;

pub const MAGIC: &[u8] = b"SUNP BURN FILE";
pub const CHECKSUM_SLOT: usize = 0x1fc;
const CLUSTER: usize = 512;
const DIR_ENTRY: usize = 32;

#[derive(Debug)]
pub struct Header {
    pub total_size: u32,
    pub hdr1_off: u32,
    pub hdr2_off: u32,
    pub main_off: u32,
    pub checksum_word: u32,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub dir_entry_off: usize, // offset of the 8.3 entry (where size lives, +0x1c)
    pub start_cluster: u16,
    pub size: u32,
    pub clusters: Vec<u16>,
}

pub struct Brn {
    pub data: Vec<u8>,
    pub fat_off: usize,
    pub root_off: usize,
    pub data_off: usize,
}

fn rd_u16(b: &[u8], o: usize) -> u16 {
    u16::from_le_bytes([b[o], b[o + 1]])
}
fn rd_u32(b: &[u8], o: usize) -> u32 {
    u32::from_le_bytes([b[o], b[o + 1], b[o + 2], b[o + 3]])
}

/// Whole-file u32 sum (mod 2^32); 0 means the checksum is valid.
pub fn file_sum(data: &[u8]) -> u32 {
    let mut s = 0u32;
    let mut i = 0;
    while i + 4 <= data.len() {
        s = s.wrapping_add(rd_u32(data, i));
        i += 4;
    }
    // trailing bytes (file length is a multiple of 4 for these BRNs, but be safe)
    let mut k = 0;
    while i < data.len() {
        s = s.wrapping_add((data[i] as u32) << (k * 8));
        i += 1;
        k += 1;
    }
    s
}

impl Brn {
    pub fn parse(data: Vec<u8>) -> Result<Self, String> {
        if data.len() < 0x200 || &data[..MAGIC.len()] != MAGIC {
            return Err("not a SUNP BURN FILE".into());
        }
        let hdr2 = rd_u32(&data, 0x18) as usize;
        // The RO_RES FAT volume starts right after HDR2 (HDR2 block is 0x320 bytes).
        let fat_off = hdr2 + 0x320;
        if fat_off + 4 > data.len() || rd_u32(&data, fat_off) != 0xffff_fff8 {
            return Err(format!("no FAT16 id at {:#x}", fat_off));
        }
        // FAT size = distance to the identical second FAT copy (same media id word).
        let mut fat2 = fat_off + 0x200;
        while fat2 < data.len() && rd_u32(&data, fat2) != 0xffff_fff8 {
            fat2 += 0x200;
        }
        if fat2 >= data.len() {
            return Err("second FAT copy not found".into());
        }
        let fat_size = fat2 - fat_off;
        let root_off = fat2 + fat_size; // fixed root dir follows the two FATs
        // Data region = where cluster 2 lives = the first "."/".." subdir after the root.
        let data_off = Self::find_first_dir(&data, root_off)?;
        Ok(Brn { data, fat_off, root_off, data_off })
    }

    fn find_first_dir(data: &[u8], from: usize) -> Result<usize, String> {
        let dot = b".          \x10";
        let mut o = from & !(DIR_ENTRY - 1);
        while o + 34 <= data.len() {
            if &data[o..o + dot.len()] == dot && &data[o + 32..o + 32 + 2] == b".." {
                return Ok(o);
            }
            o += DIR_ENTRY;
        }
        Err("no subdirectory found for cluster-2 anchor".into())
    }

    pub fn header(&self) -> Header {
        Header {
            total_size: rd_u32(&self.data, 0x10),
            hdr1_off: rd_u32(&self.data, 0x14),
            hdr2_off: rd_u32(&self.data, 0x18),
            main_off: rd_u32(&self.data, 0x1c),
            checksum_word: rd_u32(&self.data, CHECKSUM_SLOT),
        }
    }

    pub fn checksum_valid(&self) -> bool {
        file_sum(&self.data) == 0
    }

    fn fat_next(&self, c: u16) -> u16 {
        rd_u16(&self.data, self.fat_off + c as usize * 2)
    }

    fn chain(&self, start: u16) -> Vec<u16> {
        let mut out = Vec::new();
        let mut c = start;
        while c >= 2 && c < 0xfff8 && !out.contains(&c) {
            out.push(c);
            c = self.fat_next(c);
        }
        out
    }

    fn cluster_off(&self, c: u16) -> usize {
        self.data_off + (c as usize - 2) * CLUSTER
    }

    /// All files in the RO_RES volume (recursively), keyed by path with '/' separators.
    pub fn files(&self) -> BTreeMap<String, FileEntry> {
        let mut out = BTreeMap::new();
        self.walk(self.root_off, String::new(), &mut out);
        out
    }

    fn read_dir_bytes(&self, start_cluster: u16) -> Vec<u8> {
        let mut v = Vec::new();
        for c in self.chain(start_cluster) {
            let o = self.cluster_off(c);
            v.extend_from_slice(&self.data[o..o + CLUSTER]);
        }
        v
    }

    fn walk(&self, dir_off: usize, path: String, out: &mut BTreeMap<String, FileEntry>) {
        // dir_off points into self.data for the root; subdirs are read via clusters, but
        // to keep offsets valid for in-place edits we only need file entries' absolute
        // offsets, so we walk the root in place and subdirs by cluster-mapped offset.
        let mut lfn: Vec<u16> = Vec::new();
        let mut o = dir_off;
        while o + DIR_ENTRY <= self.data.len() {
            let e = &self.data[o..o + DIR_ENTRY];
            if e[0] == 0 {
                break;
            }
            if e[0] == 0xe5 {
                lfn.clear();
                o += DIR_ENTRY;
                continue;
            }
            let attr = e[11];
            if attr == 0x0f {
                let mut part = Vec::new();
                for &i in &[1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30] {
                    part.push(rd_u16(e, i));
                }
                let mut seg: Vec<u16> = part.into_iter().take_while(|&c| c != 0 && c != 0xffff).collect();
                seg.append(&mut lfn);
                lfn = seg;
                o += DIR_ENTRY;
                continue;
            }
            let short = {
                let base = String::from_utf8_lossy(&e[0..8]).trim_end().to_string();
                let ext = String::from_utf8_lossy(&e[8..11]).trim_end().to_string();
                if ext.is_empty() { base } else { format!("{base}.{ext}") }
            };
            let name = if lfn.is_empty() {
                short.clone()
            } else {
                String::from_utf16_lossy(&lfn)
            };
            lfn.clear();
            if short != "." && short != ".." {
                let cluster = rd_u16(e, 26);
                let size = rd_u32(e, 28);
                let full = if path.is_empty() { name.clone() } else { format!("{path}/{name}") };
                if attr & 0x10 != 0 {
                    // subdirectory: copy its bytes out, walk a synthetic view
                    let bytes = self.read_dir_bytes(cluster);
                    self.walk_bytes(&bytes, cluster, full, out);
                } else {
                    out.insert(
                        full.clone(),
                        FileEntry { name: full, dir_entry_off: o, start_cluster: cluster, size, clusters: self.chain(cluster) },
                    );
                }
            }
            o += DIR_ENTRY;
        }
    }

    // Walk a subdirectory whose bytes were gathered from clusters. We still need the
    // absolute dir-entry offset for in-place size edits, so map each entry back.
    fn walk_bytes(&self, bytes: &[u8], dir_cluster: u16, path: String, out: &mut BTreeMap<String, FileEntry>) {
        let chain = self.chain(dir_cluster);
        let abs = |i: usize| self.cluster_off(chain[i / CLUSTER]) + (i % CLUSTER);
        let mut lfn: Vec<u16> = Vec::new();
        let mut o = 0;
        while o + DIR_ENTRY <= bytes.len() {
            let e = &bytes[o..o + DIR_ENTRY];
            if e[0] == 0 {
                break;
            }
            if e[0] == 0xe5 {
                lfn.clear();
                o += DIR_ENTRY;
                continue;
            }
            let attr = e[11];
            if attr == 0x0f {
                let mut part = Vec::new();
                for &i in &[1usize, 3, 5, 7, 9, 14, 16, 18, 20, 22, 24, 28, 30] {
                    part.push(rd_u16(e, i));
                }
                let mut seg: Vec<u16> = part.into_iter().take_while(|&c| c != 0 && c != 0xffff).collect();
                seg.append(&mut lfn);
                lfn = seg;
                o += DIR_ENTRY;
                continue;
            }
            let short = {
                let base = String::from_utf8_lossy(&e[0..8]).trim_end().to_string();
                let ext = String::from_utf8_lossy(&e[8..11]).trim_end().to_string();
                if ext.is_empty() { base } else { format!("{base}.{ext}") }
            };
            let name = if lfn.is_empty() { short.clone() } else { String::from_utf16_lossy(&lfn) };
            lfn.clear();
            if short != "." && short != ".." {
                let cluster = rd_u16(e, 26);
                let size = rd_u32(e, 28);
                let full = format!("{path}/{name}");
                if attr & 0x10 != 0 {
                    let sub = self.read_dir_bytes(cluster);
                    self.walk_bytes(&sub, cluster, full, out);
                } else {
                    out.insert(
                        full.clone(),
                        FileEntry { name: full, dir_entry_off: abs(o), start_cluster: cluster, size, clusters: self.chain(cluster) },
                    );
                }
            }
            o += DIR_ENTRY;
        }
    }

    /// Read a resource file's bytes.
    pub fn read_file(&self, f: &FileEntry) -> Vec<u8> {
        let mut v = Vec::new();
        for &c in &f.clusters {
            let o = self.cluster_off(c);
            v.extend_from_slice(&self.data[o..o + CLUSTER]);
        }
        v.truncate(f.size as usize);
        v
    }

    /// The main code image (everything from the main-image offset to HDR3).
    pub fn main_image(&self) -> &[u8] {
        let start = self.header().main_off as usize;
        &self.data[start..]
    }

    /// Replace a resource file's bytes in place. `new` must be <= the file's current size
    /// (so it fits the existing cluster chain); the directory size field is updated and
    /// unused tail bytes in the last cluster are left as they were. Does NOT fix checksum.
    pub fn replace_file(&mut self, f: &FileEntry, new: &[u8]) -> Result<(), String> {
        if new.len() as u32 > f.size {
            return Err(format!("{}: new {} bytes > current {} (growing not supported)", f.name, new.len(), f.size));
        }
        let mut written = 0;
        for &c in &f.clusters {
            if written >= new.len() {
                break;
            }
            let o = self.cluster_off(c);
            let n = (new.len() - written).min(CLUSTER);
            self.data[o..o + n].copy_from_slice(&new[written..written + n]);
            written += n;
        }
        // update size in the directory entry
        let sz = (new.len() as u32).to_le_bytes();
        self.data[f.dir_entry_off + 0x1c..f.dir_entry_off + 0x20].copy_from_slice(&sz);
        Ok(())
    }

    /// Main image load address (ARM VA) — confirmed 0x40000000 for DiCam850.
    pub const IMAGE_BASE: u32 = 0x4000_0000;

    /// File offset of a main-image virtual address, if it lands inside the image.
    pub fn va_to_file(&self, va: u32) -> Option<usize> {
        let main = self.header().main_off as usize;
        let off = main + (va.checked_sub(Self::IMAGE_BASE)? as usize);
        if off < self.data.len() { Some(off) } else { None }
    }

    /// Overwrite bytes at a main-image virtual address (same length, in-place).
    /// For code/table patches; does NOT fix checksum (call `fix_checksum`). Returns the
    /// original bytes so a caller can verify or revert.
    pub fn patch_va(&mut self, va: u32, new: &[u8]) -> Result<Vec<u8>, String> {
        let off = self.va_to_file(va).ok_or_else(|| format!("VA {:#x} outside main image", va))?;
        if off + new.len() > self.data.len() {
            return Err("patch runs past end of image".into());
        }
        let old = self.data[off..off + new.len()].to_vec();
        self.data[off..off + new.len()].copy_from_slice(new);
        Ok(old)
    }

    /// Read bytes at a main-image virtual address.
    pub fn read_va(&self, va: u32, len: usize) -> Option<&[u8]> {
        let off = self.va_to_file(va)?;
        self.data.get(off..off + len)
    }

    /// Recompute and store the checksum compensation word so `file_sum` becomes 0.
    pub fn fix_checksum(&mut self) {
        self.data[CHECKSUM_SLOT..CHECKSUM_SLOT + 4].copy_from_slice(&0u32.to_le_bytes());
        let s = file_sum(&self.data);
        let comp = 0u32.wrapping_sub(s);
        self.data[CHECKSUM_SLOT..CHECKSUM_SLOT + 4].copy_from_slice(&comp.to_le_bytes());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn sample() -> Vec<u8> {
        std::fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("sample.BRN")).expect("sample.BRN")
    }

    #[test]
    fn parses_and_checksum_is_valid() {
        let b = Brn::parse(sample()).unwrap();
        let h = b.header();
        assert_eq!(h.total_size as usize, b.data.len());
        assert!(b.checksum_valid());
    }

    #[test]
    fn lists_expected_resources() {
        let b = Brn::parse(sample()).unwrap();
        let files = b.files();
        // AGC.BIN is not a FAT file here (it lives inside AFECMD.BIN in the main image).
        for want in ["RO_RES/IQ/AECONFIG.BIN", "RO_RES/IQ/AETARGET.BIN", "RO_RES/CONFIG.TXT", "RO_RES/WDRFT.BIN"] {
            assert!(files.contains_key(want), "missing {want}");
        }
        let cfg = &files["RO_RES/IQ/AECONFIG.BIN"];
        assert_eq!(cfg.size, 328);
    }

    #[test]
    fn roundtrip_replace_with_identical_is_byte_identical() {
        // The safety gate: reading a file and writing the same bytes back, then fixing the
        // checksum, must reproduce the input exactly.
        let orig = sample();
        let mut b = Brn::parse(orig.clone()).unwrap();
        let entry = b.files()["RO_RES/IQ/AECONFIG.BIN"].clone();
        let bytes = b.read_file(&entry);
        b.replace_file(&entry, &bytes).unwrap();
        b.fix_checksum();
        assert_eq!(b.data, orig, "round-trip changed the image");
    }

    #[test]
    fn edit_keeps_everything_else_identical_and_valid() {
        let orig = sample();
        let mut b = Brn::parse(orig.clone()).unwrap();
        let entry = b.files()["RO_RES/IQ/AETARGET.BIN"].clone();
        let mut bytes = b.read_file(&entry);
        bytes[0] ^= 0xff; // change one byte
        b.replace_file(&entry, &bytes).unwrap();
        b.fix_checksum();
        assert!(b.checksum_valid());
        // exactly the one data byte and the checksum word differ
        let diffs: Vec<usize> = (0..orig.len()).filter(|&i| orig[i] != b.data[i]).collect();
        let cluster0 = b.cluster_off(entry.start_cluster);
        assert!(diffs.contains(&cluster0));
        for d in &diffs {
            assert!(*d == cluster0 || (CHECKSUM_SLOT..CHECKSUM_SLOT + 4).contains(d), "unexpected change at {:#x}", d);
        }
    }

    #[test]
    fn image_va_roundtrip_and_patch() {
        let orig = sample();
        let mut b = Brn::parse(orig.clone()).unwrap();
        // the SPCA6500 marker string sits at a known VA in the image head
        let base = Brn::IMAGE_BASE + 0x40; // "SPCA6500 V000" area (image +0x40)
        let before = b.read_va(base, 8).unwrap().to_vec();
        assert_eq!(&before, b"SPCA6500");
        // patch with identical bytes -> fix checksum -> byte-identical file
        b.patch_va(base, &before).unwrap();
        b.fix_checksum();
        assert_eq!(b.data, orig, "identical image patch must reproduce input");
        // a real 1-byte change: only that byte + checksum word differ
        let mut b2 = Brn::parse(orig.clone()).unwrap();
        let off = b2.va_to_file(base).unwrap();
        b2.patch_va(base, b"XPCA6500").unwrap();
        b2.fix_checksum();
        assert!(b2.checksum_valid());
        let diffs: Vec<usize> = (0..orig.len()).filter(|&i| orig[i] != b2.data[i]).collect();
        assert!(diffs.contains(&off));
        for d in &diffs {
            assert!(*d == off || (CHECKSUM_SLOT..CHECKSUM_SLOT + 4).contains(d), "stray change at {:#x}", d);
        }
    }

    #[test]
    fn rejects_out_of_range_va() {
        let mut b = Brn::parse(sample()).unwrap();
        assert!(b.patch_va(0x3fff_0000, b"x").is_err()); // below image base
        assert!(b.read_va(0x4fff_ffff, 4).is_none());    // past end
    }

    #[test]
    fn rejects_growing_a_file() {
        let mut b = Brn::parse(sample()).unwrap();
        let entry = b.files()["RO_RES/IQ/AECONFIG.BIN"].clone();
        let big = vec![0u8; entry.size as usize + 1];
        assert!(b.replace_file(&entry, &big).is_err());
    }
}
