use crate::da::{DaInterface, read_gpt, GPT_BLOCK_SIZE};
use anyhow::{bail, Result};

const DISABLE: u32 = 3;

pub fn disable_vbmeta(da: &DaInterface) -> Result<()> {
    let gpt = read_gpt(da)?;
    let parts: Vec<_> = gpt.iter().filter(|e| e.name.to_lowercase().starts_with("vbmeta")).collect();
    if parts.is_empty() { bail!("No vbmeta partition"); }
    for e in &parts {
        let addr = e.start_lba as u32 * GPT_BLOCK_SIZE;
        let size = e.size_lba as u32 * GPT_BLOCK_SIZE;
        let mut data = da.read_flash(addr, size)?;
        if data.len() < 0x80 || &data[0..4] != b"AVBf" {
            log::warn!("{}: bad vbmeta", e.name);
            continue;
        }
        let cur = u32::from_be_bytes([data[0x78], data[0x79], data[0x7A], data[0x7B]]);
        log::info!("{}: flags 0x{:08x} -> 0x{:08x}", e.name, cur, DISABLE);
        data[0x78..0x7C].copy_from_slice(&DISABLE.to_be_bytes());
        da.write_flash(addr, &data)?;
    }
    Ok(())
}
