use anyhow::{bail, Result};
use std::path::Path;
use std::fs;

const NORMAL_SIZE: u64 = 4 * 1024 * 1024;
const FLAG_PATTERN: &[u8] = b"AND_ROMINFO_v";

pub fn detect_memory_type(data: &[u8]) -> &str {
    if data.starts_with(b"EMMC_BOOT") { "EMMC" }
    else if data.starts_with(b"UFS_BOOT") { "UFS" }
    else if data.starts_with(b"COMBO_BOOT") { "COMBO" }
    else { "UNKNOWN" }
}

pub fn find_flag_block(data: &[u8]) -> Option<(usize, u8)> {
    let p = data.windows(FLAG_PATTERN.len()).position(|w| w == FLAG_PATTERN)?;
    let off = p + 0x4C;
    if off < data.len() { Some((p, data[off])) } else { None }
}

pub fn patch_raw(data: &[u8], force: bool) -> Result<Vec<u8>> {
    let sz = data.len() as u64;
    if sz != NORMAL_SIZE {
        log::warn!("Size 0x{:x} (expected 0x400000)", sz);
        if !force { log::warn!("Use --force to override"); }
    }
    log::info!("Memory: {}", detect_memory_type(data));
    let (flag_off, lock) = find_flag_block(data)
        .ok_or_else(|| anyhow::anyhow!("AND_ROMINFO_v not found"))?;
    log::info!("Lock: 0x{:02x} {}", lock,
        if lock == 0x22 { "(locked)" } else if lock == 0x11 { "(hard)" } else { "" });
    let code_off = data[0x20D] as usize * 256;
    let raw_end = (sz as usize).saturating_sub(0x3000);
    if raw_end <= code_off { bail!("Bad code offset 0x{:x}", code_off); }
    let raw = &data[code_off..raw_end];
    log::info!("Zeros: 0x{:x}:0x2000  Jump: 0x{:x}->0x2000", code_off, code_off);
    let mut out = data.to_vec();
    for b in out[code_off..raw_end].iter_mut() { *b = 0; }
    let end = 0x2000 + raw.len();
    if end > out.len() { bail!("Overflow"); }
    out[0x2000..end].copy_from_slice(raw);
    out[0x20D] = 0x20; out[0x21D] = 0x20;
    out[0x211] = 0x10; out[0x212] = 0x10;
    out[0x221] = 0x10; out[0x222] = 0x10;
    let fdata = &data[flag_off..flag_off + 0x78];
    let fe = 0x1000 + fdata.len();
    if fe <= out.len() { out[0x1000..fe].copy_from_slice(fdata); }
    if 0x104C < out.len() { out[0x104C] = 0x00; }
    log::info!("Fastboot lock: 0x{:02x} -> 00", lock);
    Ok(out)
}

#[allow(dead_code)]
pub fn patch_file(input: &Path, output: Option<&Path>, force: bool) -> Result<Vec<u8>> {
    let data = fs::read(input)?;
    let patched = patch_raw(&data, force)?;
    let out = match output {
        Some(p) => p.to_path_buf(),
        None => {
            let stem = input.file_stem().unwrap_or_default();
            let parent = input.parent().unwrap_or(Path::new("."));
            parent.join(format!("{}_patched.bin", stem.to_string_lossy()))
        }
    };
    fs::write(&out, &patched)?;
    log::info!("Written {:?} ({}B)", out, patched.len());
    Ok(patched)
}
