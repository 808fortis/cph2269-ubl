mod brom;
mod da;
mod dalegacy;
mod daxflash;
mod patch;
mod preloader;
mod unlock;
mod usb;
mod vbmeta;

use da::DaInterface;
use eframe::egui;
use std::path::Path;
use std::sync::mpsc;
use std::thread;

enum Cmd {
    Connect,
    Disconnect,
    Unlock,
    Vbmeta,
    Dump(String),
    Write(String),
    FullUnlock(String),
}

enum Event {
    Log(String),
    Chip(String),
    Slot(String),
    Connected,
    Disconnected,
    Done,
    Error(String),
}

fn worker(mut iface: Option<DaInterface>, cmd_rx: mpsc::Receiver<Cmd>, evt_tx: mpsc::Sender<Event>) {
    let log = |m: String| { let _ = evt_tx.send(Event::Log(m)); };
    loop {
        let cmd = match cmd_rx.recv() {
            Ok(c) => c,
            Err(_) => break,
        };
        match cmd {
            Cmd::Connect => {
                log("Connecting...".into());
                let usb = match usb::MtkUsb::connect() {
                    Ok(u) => u,
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                };
                log("BROM detected".into());
                let pl = match brom::Preloader::new(usb) {
                    Ok(p) => p,
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                };
                let _ = evt_tx.send(Event::Chip(format!("0x{:04x}", pl.hw_code)));
                log("Loading DA...".into());
                let da_usb = match da::DaLoader::load_and_jump(&pl) {
                    Ok(d) => d,
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                };
                let di = DaInterface::Legacy(dalegacy::DaLegacy::new(da_usb));
                match da::get_active_slot(&di) {
                    Ok(s) => {
                        let disp = if s.is_empty() { "(none)".to_string() } else { s.clone() };
                        let _ = evt_tx.send(Event::Slot(disp));
                    }
                    Err(_) => { let _ = evt_tx.send(Event::Slot("?".into())); }
                }
                iface = Some(di);
                let _ = evt_tx.send(Event::Connected);
                log("Connected".into());
                let _ = evt_tx.send(Event::Done);
            }
            Cmd::Disconnect => {
                iface = None;
                let _ = evt_tx.send(Event::Disconnected);
                log("Disconnected".into());
                let _ = evt_tx.send(Event::Done);
            }
            Cmd::Unlock => {
                let i = match iface.as_ref() {
                    Some(i) => i,
                    None => { let _ = evt_tx.send(Event::Error("Not connected".into())); continue; }
                };
                log("Unlocking bootloader...".into());
                match unlock::unlock_bootloader(i) {
                    Ok(_) => log("Bootloader unlocked!".into()),
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                }
                let _ = evt_tx.send(Event::Done);
            }
            Cmd::Vbmeta => {
                let i = match iface.as_ref() {
                    Some(i) => i,
                    None => { let _ = evt_tx.send(Event::Error("Not connected".into())); continue; }
                };
                log("Disabling vbmeta...".into());
                match vbmeta::disable_vbmeta(i) {
                    Ok(_) => log("Vbmeta disabled!".into()),
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                }
                let _ = evt_tx.send(Event::Done);
            }
            Cmd::Dump(slot) => {
                let i = match iface.take() {
                    Some(i) => i,
                    None => { let _ = evt_tx.send(Event::Error("Not connected".into())); continue; }
                };
                let ops = preloader::PreloaderOps::new(i);
                log(format!("Dumping slot {}...", slot));
                match ops.dump(&slot, Path::new("dumps")) {
                    Ok(data) => {
                        log(format!("Dumped {} bytes", data.len()));
                        match patch::patch_raw(&data, false) {
                            Ok(patched) => {
                                let _ = std::fs::write(Path::new("dumps/boot1_patched.bin"), &patched);
                                log(format!("Patched -> dumps/boot1_patched.bin"));
                            }
                            Err(e) => log(format!("Patch: {}", e)),
                        }
                    }
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); }
                }
                iface = Some(ops.into_iface());
                let _ = evt_tx.send(Event::Done);
            }
            Cmd::Write(slot) => {
                let path = Path::new("dumps/boot1_patched.bin");
                let data = match std::fs::read(path) {
                    Ok(d) => d,
                    Err(_) => { let _ = evt_tx.send(Event::Error("dumps/boot1_patched.bin not found".into())); continue; }
                };
                let i = match iface.take() {
                    Some(i) => i,
                    None => { let _ = evt_tx.send(Event::Error("Not connected".into())); continue; }
                };
                let ops = preloader::PreloaderOps::new(i);
                log(format!("Writing {} bytes to slot {}...", data.len(), slot));
                match ops.write(&slot, &data) {
                    Ok(_) => log("Written!".into()),
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); }
                }
                iface = Some(ops.into_iface());
                let _ = evt_tx.send(Event::Done);
            }
            Cmd::FullUnlock(slot) => {
                let _ = std::fs::create_dir_all("dumps");
                let usb = match usb::MtkUsb::connect() {
                    Ok(u) => u,
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                };
                log("BROM OK".into());
                let pl = match brom::Preloader::new(usb) {
                    Ok(p) => p,
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                };
                let _ = evt_tx.send(Event::Chip(format!("0x{:04x}", pl.hw_code)));
                log("Loading DA...".into());
                let da_usb = match da::DaLoader::load_and_jump(&pl) {
                    Ok(d) => d,
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                };
                let di = DaInterface::Legacy(dalegacy::DaLegacy::new(da_usb));
                match da::get_active_slot(&di) {
                    Ok(s) => {
                        let disp = if s.is_empty() { "(none)".into() } else { s.clone() };
                        let _ = evt_tx.send(Event::Slot(disp));
                    }
                    Err(_) => {}
                };
                log("1/4 Unlock bootloader...".into());
                if let Err(e) = unlock::unlock_bootloader(&di) {
                    let _ = evt_tx.send(Event::Error(format!("Unlock failed: {}", e)));
                    continue;
                }
                log("2/4 Disable vbmeta...".into());
                if let Err(e) = vbmeta::disable_vbmeta(&di) {
                    let _ = evt_tx.send(Event::Error(format!("Vbmeta failed: {}", e)));
                    continue;
                }
                log("3/4 Dump & patch preloader...".into());
                let ops = preloader::PreloaderOps::new(di);
                let raw = match ops.dump(&slot, Path::new("dumps")) {
                    Ok(d) => d,
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                };
                let patched = match patch::patch_raw(&raw, false) {
                    Ok(p) => p,
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                };
                let _ = std::fs::write(Path::new("dumps/boot1_patched.bin"), &patched);
                log("4/4 Write patched preloader...".into());
                match ops.write(&slot, &patched) {
                    Ok(_) => log("Patched preloader written!".into()),
                    Err(e) => { let _ = evt_tx.send(Event::Error(e.to_string())); continue; }
                }
                let _ = evt_tx.send(Event::Connected);
                log("Full unlock complete!".into());
                let _ = evt_tx.send(Event::Done);
            }
        }
    }
}

struct MtkApp {
    cmd_tx: mpsc::Sender<Cmd>,
    evt_rx: mpsc::Receiver<Event>,
    log: Vec<String>,
    connected: bool,
    chip: String,
    slot: String,
    busy: bool,
    force: bool,
}

impl Default for MtkApp {
    fn default() -> Self {
        let (cmd_tx, cmd_rx) = mpsc::channel();
        let (evt_tx, evt_rx) = mpsc::channel();
        thread::spawn(|| worker(None, cmd_rx, evt_tx));
        MtkApp {
            cmd_tx,
            evt_rx,
            log: vec!["Ready. Connect device in BROM mode.".into()],
            connected: false,
            chip: "-".into(),
            slot: "-".into(),
            busy: false,
            force: false,
        }
    }
}

impl MtkApp {
    fn send(&mut self, cmd: Cmd) {
        self.busy = true;
        let _ = self.cmd_tx.send(cmd);
    }

    fn drain(&mut self) {
        while let Ok(ev) = self.evt_rx.try_recv() {
            match ev {
                Event::Log(m) => self.log.push(m),
                Event::Chip(c) => self.chip = c,
                Event::Slot(s) => self.slot = s,
                Event::Connected => self.connected = true,
                Event::Disconnected => { self.connected = false; self.chip = "-".into(); self.slot = "-".into(); }
                Event::Done => self.busy = false,
                Event::Error(e) => { self.log.push(format!("ERROR: {}", e)); self.busy = false; }
            }
        }
        if self.log.len() > 500 { self.log.drain(0..self.log.len() - 500); }
    }
}

impl eframe::App for MtkApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.drain();
        ctx.request_repaint_after(std::time::Duration::from_millis(100));

        egui::TopBottomPanel::top("header").show(ctx, |ui| {
            ui.heading("MTK Flash Tool");
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            egui::Grid::new("info").num_columns(2).striped(true).show(ui, |ui| {
                ui.label("Status:");
                ui.colored_label(
                    if self.connected { egui::Color32::GREEN } else { egui::Color32::RED },
                    if self.connected { "Connected" } else { "No device" },
                );
                ui.end_row();
                ui.label("Chip:");
                ui.label(&self.chip);
                ui.end_row();
                ui.label("Slot:");
                ui.label(&self.slot);
                ui.end_row();
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy, egui::Button::new("Connect")).clicked() {
                    self.send(Cmd::Connect);
                }
                if ui.add_enabled(!self.busy && self.connected, egui::Button::new("Disconnect")).clicked() {
                    self.send(Cmd::Disconnect);
                }
            });

            ui.separator();
            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy && self.connected, egui::Button::new("Unlock Bootloader")).clicked() {
                    self.send(Cmd::Unlock);
                }
                if ui.add_enabled(!self.busy && self.connected, egui::Button::new("Disable VBMeta")).clicked() {
                    self.send(Cmd::Vbmeta);
                }
            });

            ui.horizontal(|ui| {
                if ui.add_enabled(!self.busy && self.connected, egui::Button::new("Dump Preloader")).clicked() {
                    self.send(Cmd::Dump("A".into()));
                }
                if ui.add_enabled(!self.busy && self.connected, egui::Button::new("Write Patched")).clicked() {
                    self.send(Cmd::Write("A".into()));
                }
            });

            ui.separator();
            let fl = if !self.busy { "★ UNLOCK BOOTLOADER (FULL AUTO)" } else { "Working..." };
            if ui.add_enabled(!self.busy, egui::Button::new(fl).min_size(egui::vec2(ui.available_width(), 40.0))).clicked() {
                self.send(Cmd::FullUnlock("A".into()));
            }

            ui.separator();
            if self.busy {
                ui.horizontal(|ui| { ui.spinner(); ui.label("Working..."); });
            }

            ui.separator();
            let label = format!("Log ({})", self.log.len());
            egui::ScrollArea::vertical()
                .id_salt("log")
                .max_height(220.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.label(label);
                    for l in &self.log {
                        let c = if l.starts_with("ERROR") { egui::Color32::RED }
                            else if l.starts_with("WARN") { egui::Color32::YELLOW }
                            else { egui::Color32::LIGHT_GRAY };
                        ui.colored_label(c, l);
                    }
                });

            ui.horizontal(|ui| {
                ui.checkbox(&mut self.force, "Force");
                if ui.button("Clear Log").clicked() { self.log.clear(); }
            });
        });
    }
}

fn main() -> Result<(), eframe::Error> {
    let _ = std::fs::create_dir_all("dumps");
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default().with_inner_size([640.0, 500.0]).with_resizable(true),
        ..Default::default()
    };
    eframe::run_native("MTK Flash Tool", options, Box::new(|_cc| Ok(Box::new(MtkApp::default()))))
}
