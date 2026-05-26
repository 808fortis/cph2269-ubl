use crate::usb::MtkUsb;
use anyhow::{bail, Result};

const DA_ACK: u8 = 0x5A;
const CMD_SDMMC_SWITCH_PART: u8 = 0x60;
const CMD_SDMMC_WRITE_IMAGE: u8 = 0x61;
const CMD_SDMMC_READ_DATA: u8 = 0x7B;

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub enum PartType {
    Boot1 = 1,
    Boot2 = 2,
    User = 0,
    GPT = 3,
}

pub struct DaLegacy {
    usb: MtkUsb,
}

impl DaLegacy {
    pub fn new(usb: MtkUsb) -> Self {
        DaLegacy { usb }
    }

    fn send_cmd(&self, cmd: u8, params: &[u8]) -> Result<()> {
        let mut packet = vec![DA_ACK, cmd];
        packet.extend_from_slice(params);
        let csum: u16 = packet.iter().map(|&b| b as u16).sum::<u16>() & 0xFFFF;
        packet.extend_from_slice(&csum.to_le_bytes());
        self.usb.bulk_write(&packet)?;
        Ok(())
    }

    fn read_ack(&self) -> Result<()> {
        let buf = self.usb.bulk_read_exact(1)?;
        if buf[0] != DA_ACK {
            bail!("DA NACK 0x{:02x}", buf[0]);
        }
        Ok(())
    }

    pub fn switch_part(&self, part: PartType) -> Result<()> {
        let _ = self.send_cmd(CMD_SDMMC_SWITCH_PART, &[part as u8]);
        std::thread::sleep(std::time::Duration::from_millis(100));
        self.read_ack()?;
        Ok(())
    }

    pub fn read_flash(&self, addr: u32, length: u32) -> Result<Vec<u8>> {
        let mut p = Vec::with_capacity(8);
        p.extend_from_slice(&addr.to_be_bytes());
        p.extend_from_slice(&length.to_be_bytes());
        self.send_cmd(CMD_SDMMC_READ_DATA, &p)?;
        self.read_ack()?;
        let mut data = Vec::with_capacity(length as usize);
        let wmax = self.usb.wmax as usize;
        let mut rem = length as usize;
        while rem > 0 {
            let sz = std::cmp::min(rem, wmax);
            let mut buf = self.usb.bulk_read_exact(sz)?;
            rem -= buf.len();
            data.append(&mut buf);
        }
        let _csum = self.usb.bulk_read_exact(2)?;
        Ok(data)
    }

    pub fn write_flash(&self, addr: u32, data: &[u8]) -> Result<()> {
        let csum: u16 = data
            .chunks(4)
            .map(|c| {
                let mut b = [0u8; 4];
                let n = std::cmp::min(c.len(), 4);
                b[..n].copy_from_slice(&c[..n]);
                u32::from_le_bytes(b) as u64
            })
            .fold(0u64, |a, v| a.wrapping_add(v)) as u16;
        let mut p = Vec::with_capacity(8);
        p.extend_from_slice(&addr.to_be_bytes());
        p.extend_from_slice(&(data.len() as u32).to_be_bytes());
        self.send_cmd(CMD_SDMMC_WRITE_IMAGE, &p)?;
        self.read_ack()?;
        let wmax = self.usb.wmax as usize;
        for chunk in data.chunks(wmax) {
            self.usb.bulk_write(chunk)?;
        }
        if data.len() % wmax == 0 {
            self.usb.bulk_write(&[])?;
        }
        self.usb.bulk_write(&csum.to_le_bytes())?;
        self.read_ack()?;
        Ok(())
    }
}
