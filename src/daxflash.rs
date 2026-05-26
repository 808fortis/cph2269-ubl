use crate::usb::MtkUsb;
use anyhow::{bail, Result};

const XFLASH_MAGIC: u32 = 0xFEEEEEEF;

#[allow(dead_code)]
pub enum StorageType { EMMC = 1, NAND = 2, NOR = 3, UFS = 4 }
#[allow(dead_code)]
pub enum PartType { Boot1 = 1, Boot2 = 2, User = 0, GPT = 3 }

pub struct DaXFlash { usb: MtkUsb }

impl DaXFlash {
    #[allow(dead_code)]
    pub fn new(usb: MtkUsb) -> Self { DaXFlash { usb } }

    fn write_pkt(&self, dt: u32, pl: &[u8]) -> Result<()> {
        let mut p = Vec::with_capacity(12 + pl.len());
        p.extend_from_slice(&XFLASH_MAGIC.to_le_bytes());
        p.extend_from_slice(&dt.to_le_bytes());
        p.extend_from_slice(&(pl.len() as u32).to_le_bytes());
        p.extend_from_slice(pl);
        let wmax = self.usb.wmax as usize;
        for c in p.chunks(wmax) { self.usb.bulk_write(c)?; }
        if p.len() % wmax == 0 { self.usb.bulk_write(&[])?; }
        Ok(())
    }

    fn read_pkt(&self) -> Result<Vec<u8>> {
        let h = self.usb.bulk_read_exact(12)?;
        let magic = u32::from_le_bytes([h[0], h[1], h[2], h[3]]);
        if magic != XFLASH_MAGIC { bail!("Bad XFlash magic"); }
        let dt = u32::from_le_bytes([h[4], h[5], h[6], h[7]]);
        let len = u32::from_le_bytes([h[8], h[9], h[10], h[11]]) as usize;
        if len == 0 { return Ok(vec![]); }
        let pl = self.usb.bulk_read_exact(len)?;
        if dt == 4 { bail!("XFlash NACK"); }
        Ok(pl)
    }

    pub fn switch_part(&self, storage: StorageType, part: PartType) -> Result<()> {
        self.write_pkt(0, &[storage as u8, part as u8, 0xFF, 0xFF])?;
        let r = self.read_pkt()?;
        if r.is_empty() || r[0] != 0x5A { bail!("Switch part failed"); }
        Ok(())
    }

    pub fn read_flash(&self, addr: u32, length: u32) -> Result<Vec<u8>> {
        let mut r = Vec::with_capacity(8);
        r.extend_from_slice(&addr.to_be_bytes());
        r.extend_from_slice(&length.to_be_bytes());
        self.write_pkt(1, &r)?;
        self.read_pkt()?;
        let mut data = Vec::with_capacity(length as usize);
        let wmax = self.usb.wmax as usize;
        let mut rem = length as usize;
        while rem > 0 {
            let sz = std::cmp::min(rem, wmax);
            let mut buf = self.usb.bulk_read_exact(sz)?;
            rem -= buf.len();
            data.append(&mut buf);
        }
        Ok(data)
    }

    pub fn write_flash(&self, addr: u32, data: &[u8]) -> Result<()> {
        let mut r = Vec::with_capacity(8);
        r.extend_from_slice(&addr.to_be_bytes());
        r.extend_from_slice(&(data.len() as u32).to_be_bytes());
        self.write_pkt(2, &r)?;
        let ack = self.read_pkt()?;
        if ack.is_empty() || ack[0] != 0x5A { bail!("Write ACK failed"); }
        let csum: u32 = data.chunks(4).map(|c| {
            let mut b = [0u8; 4]; let n = std::cmp::min(c.len(), 4);
            b[..n].copy_from_slice(&c[..n]); u32::from_le_bytes(b) as u64
        }).fold(0u64, |a, v| a.wrapping_add(v)) as u32;
        let mut f = vec![0x5Au8];
        f.extend_from_slice(&csum.to_le_bytes());
        self.write_pkt(6, &f)?;
        let resp = self.read_pkt()?;
        if resp.is_empty() || resp[0] != 0x5A { bail!("Write finalize failed"); }
        Ok(())
    }
}
