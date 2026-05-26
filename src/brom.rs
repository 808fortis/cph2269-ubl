use crate::usb::MtkUsb;
use anyhow::{bail, Result};

const CMD_GET_BL_VER: u8 = 0xFE;
const CMD_GET_HW_CODE: u8 = 0xFD;
const CMD_GET_HW_SW_VER: u8 = 0xFC;
const CMD_GET_TARGET_CONFIG: u8 = 0xD8;
const CMD_SEND_DA: u8 = 0xD7;
const CMD_JUMP_DA: u8 = 0xD5;
pub struct Preloader {
    pub usb: MtkUsb,
    pub bl_ver: u8,
    pub hw_code: u32,
    pub hw_ver: u32,
    pub target_config: u32,
}

impl Preloader {
    pub fn new(usb: MtkUsb) -> Result<Self> {
        let mut pl = Preloader {
            usb,
            bl_ver: 0,
            hw_code: 0,
            hw_ver: 0,
            target_config: 0,
        };
        pl.detect()?;
        Ok(pl)
    }

    fn detect(&mut self) -> Result<()> {
        self.usb.echo(&[CMD_GET_BL_VER])?;
        self.usb.echo(&[CMD_GET_HW_CODE])?;
        let hw = self.rdword()?;
        self.hw_code = hw >> 16;
        self.hw_ver = hw & 0xFFFF;
        let _ = self.usb.echo(&[CMD_GET_HW_SW_VER]);
        let tc = self.get_target_config()?;
        self.target_config = tc;
        log::info!("BROM: bl_ver=0x{:02x} hw=0x{:04x}.{:04x} cfg=0x{:08x}",
            self.bl_ver, self.hw_code, self.hw_ver, tc);
        Ok(())
    }

    pub fn rdword(&self) -> Result<u32> {
        let buf = self.usb.bulk_read_exact(4)?;
        Ok(u32::from_be_bytes([buf[0], buf[1], buf[2], buf[3]]))
    }

    pub fn wrword(&self, val: u32) -> Result<()> {
        self.usb.bulk_write(&val.to_be_bytes())?;
        Ok(())
    }

    pub fn read_status(&self) -> Result<u16> {
        let buf = self.usb.bulk_read_exact(2)?;
        Ok(u16::from_le_bytes([buf[0], buf[1]]))
    }

    pub fn get_target_config(&self) -> Result<u32> {
        self.usb.echo(&[CMD_GET_TARGET_CONFIG])?;
        let val = self.rdword()?;
        let _status = self.read_status()?;
        Ok(val)
    }

    pub fn send_da(&self, da_data: &[u8], addr: u32, sig_len: u32) -> Result<()> {
        self.usb.echo(&[CMD_SEND_DA])?;
        self.wrword(addr)?;
        self.wrword(da_data.len() as u32)?;
        self.wrword(sig_len)?;
        let status = self.read_status()?;
        if status == 0x1D0D {
            bail!("SLA authentication required");
        }
        if status != 0 {
            bail!("SEND_DA failed status=0x{:04x}", status);
        }
        let wmax = self.usb.wmax as usize;
        for chunk in da_data.chunks(wmax) {
            self.usb.bulk_write(chunk)?;
        }
        if da_data.len() % wmax == 0 {
            self.usb.bulk_write(&[])?;
        }
        let csum = self.read_status()?;
        let final_status = self.read_status()?;
        if final_status != 0 {
            bail!("DA data xfer failed: status=0x{:04x}", final_status);
        }
        log::info!("DA sent: {}B csum=0x{:04x}", da_data.len(), csum);
        Ok(())
    }

    pub fn jump_da(&self, addr: u32) -> Result<()> {
        self.usb.echo(&[CMD_JUMP_DA])?;
        self.wrword(addr)?;
        let echoed = self.rdword()?;
        if echoed != addr {
            log::warn!("Jump addr mismatch: 0x{:08x} != 0x{:08x}", echoed, addr);
        }
        let status = self.read_status()?;
        if status != 0 {
            bail!("JUMP_DA failed status=0x{:04x}", status);
        }
        log::info!("Jumped to DA at 0x{:08x}", addr);
        Ok(())
    }
}

pub const DA_BYTES: &[u8] = include_bytes!("../src/da/DA_BR_MT6765_20271.bin");
