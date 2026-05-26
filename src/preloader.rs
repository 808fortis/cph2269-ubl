use crate::da::DaInterface;
use crate::dalegacy::PartType;
use crate::daxflash;
use anyhow::Result;
use std::path::Path;
use std::fs;

const PRELOADER_SIZE: u32 = 4 * 1024 * 1024;

pub struct PreloaderOps { da_iface: DaInterface }

impl PreloaderOps {
    pub fn new(da_iface: DaInterface) -> Self { PreloaderOps { da_iface } }
    pub fn into_iface(self) -> DaInterface { self.da_iface }

    pub fn dump(&self, slot: &str, output_dir: &Path) -> Result<Vec<u8>> {
        fs::create_dir_all(output_dir)?;
        let name = match slot { "B" => "boot2", _ => "boot1" };
        log::info!("Dump {} slot {}", name, slot);
        self.switch_part(slot)?;
        let data = self.da_iface.read_flash(0, PRELOADER_SIZE)?;
        fs::write(output_dir.join(format!("{}.bin", name)), &data)?;
        log::info!("Dumped {}B", data.len());
        Ok(data)
    }

    pub fn write(&self, slot: &str, data: &[u8]) -> Result<()> {
        let name = match slot { "B" => "boot2", _ => "boot1" };
        log::info!("Write {} slot {}", name, slot);
        self.switch_part(slot)?;
        self.da_iface.write_flash(0, data)?;
        log::info!("Written {}B", data.len());
        Ok(())
    }

    fn switch_part(&self, slot: &str) -> Result<()> {
        match &self.da_iface {
            DaInterface::Legacy(d) => {
                let pt = match slot { "B" => PartType::Boot2, _ => PartType::Boot1 };
                d.switch_part(pt)
            }
            DaInterface::XFlash(d) => {
                let pt = match slot { "B" => daxflash::PartType::Boot2, _ => daxflash::PartType::Boot1 };
                d.switch_part(daxflash::StorageType::EMMC, pt)
            }
        }
    }
}
