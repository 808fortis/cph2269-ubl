use anyhow::{bail, Context, Result};
use rusb::{DeviceList, Direction, TransferType};
use std::time::{Duration, Instant};

const MTK_VID: u16 = 0x0E8D;
const TIMEOUT: Duration = Duration::from_secs(60);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(60);
const POLL_INTERVAL: Duration = Duration::from_millis(500);

pub struct MtkUsb {
    handle: rusb::DeviceHandle<rusb::GlobalContext>,
    pub(crate) ep_out: u8,
    pub(crate) ep_in: u8,
    pub(crate) wmax: u16,
}

impl MtkUsb {
    pub fn connect() -> Result<Self> {
        let start = Instant::now();
        while start.elapsed() < CONNECT_TIMEOUT {
            if let Ok(dev) = Self::try_connect_once() {
                return Ok(dev);
            }
            std::thread::sleep(POLL_INTERVAL);
        }
        bail!("No MediaTek device (VID 0x{:04X}) found in BROM mode (timeout {}s)", MTK_VID, CONNECT_TIMEOUT.as_secs());
    }

    fn try_connect_once() -> Result<Self> {
        let devices = DeviceList::new()?;
        for device in devices.iter() {
            let desc = match device.device_descriptor() {
                Ok(d) => d,
                Err(_) => continue,
            };
            if desc.vendor_id() != MTK_VID {
                continue;
            }
            let handle = match device.open() {
                Ok(h) => h,
                Err(_) => continue,
            };
            let config = match device.config_descriptor(0) {
                Ok(c) => c,
                Err(_) => continue,
            };
            let mut ep_out = 0x01;
            let mut ep_in = 0x81;
            let mut wmax = 512;
            let mut claimed = false;
            for interface in config.interfaces() {
                for iface_desc in interface.descriptors() {
                    for ep in iface_desc.endpoint_descriptors() {
                        if ep.transfer_type() != TransferType::Bulk {
                            continue;
                        }
                        if ep.direction() == Direction::Out {
                            ep_out = ep.address();
                            wmax = ep.max_packet_size();
                        } else if ep.direction() == Direction::In {
                            ep_in = ep.address();
                        }
                    }
                    let _ = handle.detach_kernel_driver(iface_desc.interface_number());
                    if let Ok(_) = handle.claim_interface(iface_desc.interface_number()) {
                        claimed = true;
                        break;
                    }
                }
                if claimed {
                    break;
                }
            }
            if !claimed {
                continue;
            }
            log::info!("USB: EP_OUT=0x{:02x} EP_IN=0x{:02x} wMax={}", ep_out, ep_in, wmax);
            return Ok(MtkUsb { handle, ep_out, ep_in, wmax });
        }
        bail!("No MediaTek device found this poll");
    }

    pub fn bulk_write(&self, data: &[u8]) -> Result<()> {
        let mut written = 0;
        while written < data.len() {
            let n = self
                .handle
                .write_bulk(self.ep_out, &data[written..], TIMEOUT)
                .context("Bulk write failed")?;
            if n == 0 {
                bail!("Bulk write returned 0");
            }
            written += n;
        }
        Ok(())
    }

    pub fn bulk_read(&self, size: usize) -> Result<Vec<u8>> {
        let mut buf = vec![0u8; size];
        let n = self
            .handle
            .read_bulk(self.ep_in, &mut buf, TIMEOUT)
            .context("Bulk read failed")?;
        buf.truncate(n);
        Ok(buf)
    }

    pub fn bulk_read_exact(&self, size: usize) -> Result<Vec<u8>> {
        let mut result = Vec::with_capacity(size);
        while result.len() < size {
            let rem = size - result.len();
            let chunk = std::cmp::min(rem, 65536);
            let mut buf = self.bulk_read(chunk)?;
            result.append(&mut buf);
        }
        Ok(result)
    }

    pub fn echo(&self, data: &[u8]) -> Result<Vec<u8>> {
        self.bulk_write(data)?;
        self.bulk_read_exact(data.len())
    }
}
