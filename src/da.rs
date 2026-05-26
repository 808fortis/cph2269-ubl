use crate::brom::Preloader;
use crate::dalegacy::DaLegacy;
use crate::daxflash::DaXFlash;
use crate::usb::MtkUsb;
use anyhow::{bail, Result};

#[allow(dead_code)]
pub enum DaInterface {
    Legacy(DaLegacy),
    XFlash(DaXFlash),
}

impl DaInterface {
    pub fn read_flash(&self, addr: u32, length: u32) -> Result<Vec<u8>> {
        match self {
            DaInterface::Legacy(d) => d.read_flash(addr, length),
            DaInterface::XFlash(d) => d.read_flash(addr, length),
        }
    }

    pub fn write_flash(&self, addr: u32, data: &[u8]) -> Result<()> {
        match self {
            DaInterface::Legacy(d) => d.write_flash(addr, data),
            DaInterface::XFlash(d) => d.write_flash(addr, data),
        }
    }
}

pub struct DaLoader;

impl DaLoader {
    pub fn load_and_jump(pl: &Preloader) -> Result<MtkUsb> {
        let da_data = crate::brom::DA_BYTES;
        let load_addr = resolve_da_load_addr(da_data)?;
        let sig_len = if da_data.len() >= 64 {
            u32::from_le_bytes([da_data[56], da_data[57], da_data[58], da_data[59]])
        } else {
            0
        };
        log::info!("DA: {}B addr=0x{:08x} sig_len={}", da_data.len(), load_addr, sig_len);
        pl.send_da(da_data, load_addr, sig_len)?;
        pl.jump_da(load_addr)?;
        std::thread::sleep(std::time::Duration::from_secs(2));
        let usb = MtkUsb::connect()?;
        Ok(usb)
    }
}

fn resolve_da_load_addr(da_data: &[u8]) -> Result<u32> {
    if da_data.len() < 8 {
        bail!("DA too small");
    }
    let addr = u32::from_le_bytes([da_data[4], da_data[5], da_data[6], da_data[7]]);
    if addr > 0x00100000 && addr < 0xFFFFFFFF {
        return Ok(addr);
    }
    let addr2 = u32::from_le_bytes([da_data[0], da_data[1], da_data[2], da_data[3]]);
    if addr2 > 0x00100000 && addr2 < 0xFFFFFFFF {
        return Ok(addr2);
    }
    Ok(0x20100000)
}

pub struct GptEntry {
    pub name: String,
    pub start_lba: u64,
    pub size_lba: u64,
}

pub const GPT_BLOCK_SIZE: u32 = 512;

pub fn read_gpt(da: &DaInterface) -> Result<Vec<GptEntry>> {
    let mbr = da.read_flash(0, GPT_BLOCK_SIZE)?;
    if mbr.len() < GPT_BLOCK_SIZE as usize {
        bail!("MBR read failed");
    }
    let gpt_header = da.read_flash(GPT_BLOCK_SIZE, GPT_BLOCK_SIZE)?;
    if &gpt_header[0..8] != b"EFI PART" {
        bail!("Not GPT");
    }
    let partition_entries_lba = u64::from_le_bytes([
        gpt_header[72], gpt_header[73], gpt_header[74], gpt_header[75],
        gpt_header[76], gpt_header[77], gpt_header[78], gpt_header[79],
    ]);
    let num_partitions = u32::from_le_bytes([
        gpt_header[80], gpt_header[81], gpt_header[82], gpt_header[83],
    ]);
    let entry_size = u32::from_le_bytes([
        gpt_header[84], gpt_header[85], gpt_header[86], gpt_header[87],
    ]);
    let total = (num_partitions as u64 * entry_size as u64) as u32;
    let raw = da.read_flash(partition_entries_lba as u32 * GPT_BLOCK_SIZE, total)?;
    let mut entries = Vec::new();
    for i in 0..num_partitions as usize {
        let off = i * entry_size as usize;
        if off + 128 > raw.len() {
            break;
        }
        if raw[off..off + 16].iter().all(|&b| b == 0) {
            continue;
        }
        let raw_name = &raw[off + 56..off + 128];
        let utf16: Vec<u16> = raw_name
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&c| c != 0)
            .collect();
        let name = String::from_utf16(&utf16).unwrap_or_default();
        let start_lba = u64::from_le_bytes([
            raw[off + 32], raw[off + 33], raw[off + 34], raw[off + 35],
            raw[off + 36], raw[off + 37], raw[off + 38], raw[off + 39],
        ]);
        let size_lba = u64::from_le_bytes([
            raw[off + 40], raw[off + 41], raw[off + 42], raw[off + 43],
            raw[off + 44], raw[off + 45], raw[off + 46], raw[off + 47],
        ]);
        entries.push(GptEntry { name, start_lba, size_lba });
    }
    Ok(entries)
}

pub fn get_active_slot(da: &DaInterface) -> Result<String> {
    let gpt = read_gpt(da)?;
    let misc = gpt.iter().find(|e| e.name.to_lowercase() == "misc");
    let boot_ctrl = gpt.iter().find(|e| {
        let n = e.name.to_lowercase();
        n == "boot_control" || n == "bcb"
    });
    if let Some(bc) = boot_ctrl {
        let addr = bc.start_lba as u32 * GPT_BLOCK_SIZE;
        if let Ok(data) = da.read_flash(addr, 64) {
            if data.len() >= 42 {
                let slot = std::str::from_utf8(&data[40..42]).unwrap_or("");
                let s = slot.trim_matches('\0').trim();
                if s == "_a" { return Ok("A".into()); }
                if s == "_b" { return Ok("B".into()); }
            }
        }
    }
    if let Some(m) = misc {
        let addr = m.start_lba as u32 * GPT_BLOCK_SIZE;
        if let Ok(data) = da.read_flash(addr, 2048) {
            if data.len() >= 230 {
                let s = std::str::from_utf8(&data[223..228]).unwrap_or("");
                if s.contains("_a") { return Ok("A".into()); }
                if s.contains("_b") { return Ok("B".into()); }
            }
        }
    }
    let b1 = gpt.iter().any(|e| matches!(e.name.to_lowercase().as_str(), "boot1" | "boot_1" | "boot"));
    let b2 = gpt.iter().any(|e| matches!(e.name.to_lowercase().as_str(), "boot2" | "boot_2"));
    if b1 && !b2 { return Ok("A".into()); }
    if b1 && b2 { log::warn!("Both boot1/boot2, default A"); return Ok("A".into()); }
    Ok(String::new())
}
