//! brntool — inspect and safely edit iCatch SUNP BURN FILE firmware.
//!
//!   brntool info   <in.BRN>
//!   brntool verify <in.BRN>                        (exit 0 if checksum valid)
//!   brntool unpack <in.BRN> <out_dir>              (main.bin + RO_RES/*)
//!   brntool replace <in.BRN> <res_path> <file> <out.BRN>   (in-place, fixes checksum)
//!   brntool fix    <in.BRN> <out.BRN>              (recompute checksum only)

use brntool::Brn;
use std::path::Path;
use std::process::exit;

fn read(p: &str) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| {
        eprintln!("read {p}: {e}");
        exit(2);
    })
}

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    match a.first().map(String::as_str) {
        Some("info") if a.len() == 2 => {
            let b = parse(&a[1]);
            let h = b.header();
            println!("total size   {:#x} ({} bytes)", h.total_size, h.total_size);
            println!("HDR1 @ {:#x}  HDR2 @ {:#x}  main image @ {:#x}", h.hdr1_off, h.hdr2_off, h.main_off);
            println!("FAT @ {:#x}  root @ {:#x}  data @ {:#x}", b.fat_off, b.root_off, b.data_off);
            println!("checksum     {}", if b.checksum_valid() { "VALID" } else { "INVALID" });
            let files = b.files();
            println!("RO_RES: {} files", files.len());
            for (name, f) in &files {
                println!("  {:>9}  {}", f.size, name);
            }
        }
        Some("verify") if a.len() == 2 => {
            let b = parse(&a[1]);
            if b.checksum_valid() {
                println!("checksum VALID");
            } else {
                println!("checksum INVALID");
                exit(1);
            }
        }
        Some("unpack") if a.len() == 3 => {
            let b = parse(&a[1]);
            let out = Path::new(&a[2]);
            std::fs::create_dir_all(out).unwrap();
            std::fs::write(out.join("main.bin"), b.main_image()).unwrap();
            let mut n = 0;
            for (name, f) in b.files() {
                let p = out.join(&name);
                std::fs::create_dir_all(p.parent().unwrap()).unwrap();
                std::fs::write(&p, b.read_file(&f)).unwrap();
                n += 1;
            }
            println!("unpacked main.bin + {n} resource files into {}", out.display());
        }
        Some("replace") if a.len() == 5 => {
            let mut b = parse(&a[1]);
            let entry = b.files().get(&a[2]).cloned().unwrap_or_else(|| {
                eprintln!("resource not found: {}", a[2]);
                exit(2);
            });
            let new = read(&a[3]);
            if let Err(e) = b.replace_file(&entry, &new) {
                eprintln!("replace failed: {e}");
                exit(2);
            }
            b.fix_checksum();
            assert!(b.checksum_valid());
            std::fs::write(&a[4], &b.data).unwrap();
            println!("wrote {} ({} -> {} bytes in {}), checksum fixed", a[4], entry.size, new.len(), a[2]);
        }
        Some("read-va") if a.len() == 4 => {
            let b = parse(&a[1]);
            let va = u32::from_str_radix(a[2].trim_start_matches("0x"), 16).expect("hex VA");
            let len: usize = a[3].parse().expect("len");
            match b.read_va(va, len) {
                Some(d) => println!("{}", d.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")),
                None => { eprintln!("VA out of range"); exit(2); }
            }
        }
        Some("patch-va") if a.len() == 5 => {
            let mut b = parse(&a[1]);
            let va = u32::from_str_radix(a[2].trim_start_matches("0x"), 16).expect("hex VA");
            let bytes: Vec<u8> = a[3].split(|c| c == ' ' || c == ',')
                .filter(|s| !s.is_empty())
                .map(|s| u8::from_str_radix(s.trim_start_matches("0x"), 16).expect("hex byte"))
                .collect();
            match b.patch_va(va, &bytes) {
                Ok(old) => {
                    b.fix_checksum();
                    assert!(b.checksum_valid());
                    std::fs::write(&a[4], &b.data).unwrap();
                    println!("patched {} bytes @ {:#x} (was {}), checksum fixed -> {}",
                        bytes.len(), va, old.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "), a[4]);
                }
                Err(e) => { eprintln!("patch failed: {e}"); exit(2); }
            }
        }
        Some("fix") if a.len() == 3 => {
            let mut b = parse(&a[1]);
            b.fix_checksum();
            std::fs::write(&a[2], &b.data).unwrap();
            println!("checksum fixed -> {}", a[2]);
        }
        _ => {
            eprintln!("usage:\n  brntool info|verify <in.BRN>\n  brntool unpack <in.BRN> <dir>\n  brntool replace <in.BRN> <res_path> <file> <out.BRN>\n  brntool read-va <in.BRN> <0xVA> <len>\n  brntool patch-va <in.BRN> <0xVA> \"<hex bytes>\" <out.BRN>\n  brntool fix <in.BRN> <out.BRN>");
            exit(2);
        }
    }
}

fn parse(p: &str) -> Brn {
    Brn::parse(read(p)).unwrap_or_else(|e| {
        eprintln!("{p}: {e}");
        exit(2);
    })
}
