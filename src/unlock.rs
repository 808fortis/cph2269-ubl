use crate::da::{DaInterface, read_gpt, GPT_BLOCK_SIZE};
use anyhow::{bail, Result};

const UNLOCK: u32 = 3;
const LOCK: u32 = 4;

pub fn unlock_bootloader(da: &DaInterface) -> Result<()> {
    let gpt = read_gpt(da)?;
    let seccfg = gpt.iter().find(|e| e.name.to_lowercase() == "seccfg")
        .ok_or_else(|| anyhow::anyhow!("seccfg not found"))?;
    let addr = seccfg.start_lba as u32 * GPT_BLOCK_SIZE;
    let size = seccfg.size_lba as u32 * GPT_BLOCK_SIZE;
    log::info!("seccfg at 0x{:x} size 0x{:x}", addr, size);
    let mut data = da.read_flash(addr, size)?;
    if data[..12] == *b"AND_SECCFG_v\x00" { patch_v3(&mut data, da, addr)?; }
    else if data[..4] == *b"\x4D\x4D\x4D\x4D" { patch_v4(&mut data, da, addr)?; }
    else { patch_heuristic(&mut data, da, addr)?; }
    Ok(())
}

fn state_name(s: u32) -> &'static str {
    match s { 1 => "Default", 2 => "MP_Default", 3 => "Unlock", 4 => "Lock", 5 => "Verified", 6 => "Custom", _ => "?" }
}

fn patch_v3(data: &mut [u8], da: &DaInterface, addr: u32) -> Result<()> {
    let off = 0x20;
    let cur = u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
    log::info!("V3 lock: 0x{:08x} ({})", cur, state_name(cur));
    if cur == UNLOCK { log::info!("Already unlocked"); return Ok(()); }
    data[off..off+4].copy_from_slice(&UNLOCK.to_le_bytes());
    let hash = {
        use sha2::Digest;
        let mut h = sha2::Sha256::new();
        h.update(&data[0x20..]);
        h.finalize().to_vec()
    };
    if data.len() >= 0x40 { data[0x10..0x30].copy_from_slice(&hash); }
    da.write_flash(addr, data)?;
    log::info!("Unlocked V3");
    Ok(())
}

fn patch_v4(data: &mut [u8], da: &DaInterface, addr: u32) -> Result<()> {
    for i in (4..data.len().saturating_sub(4)).step_by(4) {
        let v = u32::from_le_bytes([data[i], data[i+1], data[i+2], data[i+3]]);
        if v == LOCK || v == UNLOCK {
            log::info!("V4 lock at 0x{:x}: 0x{:08x} ({})", i, v, state_name(v));
            if v == UNLOCK { log::info!("Already unlocked"); return Ok(()); }
            data[i..i+4].copy_from_slice(&UNLOCK.to_le_bytes());
            da.write_flash(addr, data)?;
            log::info!("Unlocked V4");
            return Ok(());
        }
    }
    for off in [0x20, 0x24, 0x2C, 0x30] {
        if off + 4 <= data.len() {
            let v = u32::from_le_bytes([data[off], data[off+1], data[off+2], data[off+3]]);
            if v == LOCK || v == UNLOCK {
                log::info!("V4 lock at std 0x{:x}", off);
                data[off..off+4].copy_from_slice(&UNLOCK.to_le_bytes());
                da.write_flash(addr, data)?;
                return Ok(());
            }
        }
    }
    bail!("No lock state in V4");
}

fn patch_heuristic(data: &mut [u8], da: &DaInterface, addr: u32) -> Result<()> {
    for i in (0..data.len().saturating_sub(4)).step_by(4) {
        let v = u32::from_le_bytes([data[i], data[i+1], data[i+2], data[i+3]]);
        if v == LOCK {
            log::info!("Found lock at 0x{:x}", i);
            data[i..i+4].copy_from_slice(&UNLOCK.to_le_bytes());
            da.write_flash(addr, data)?;
            return Ok(());
        }
    }
    bail!("Lock state not found");
}
