// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use probe_rs::flashing;
use probe_rs::MemoryInterface;

use anyhow::{bail, Result};

use crate::regs::Register;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::Path;
use std::rc::Rc;

use crate::core::{Core, CORE_MAX_READSIZE};

pub struct ProbeCore {
    pub session: probe_rs::Session,
    pub identifier: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
    unhalted_reads: bool,
    halted: u32,
    unhalted_read: BTreeMap<u32, u32>,
    can_flash: bool,
}

impl ProbeCore {
    pub fn new(
        session: probe_rs::Session,
        identifier: String,
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<String>,
        unhalted_reads: bool,
        can_flash: bool,
    ) -> Self {
        Self {
            session,
            identifier,
            vendor_id,
            product_id,
            serial_number,
            unhalted_reads,
            halted: 0,
            //TODO probably a way to abstract this out
            unhalted_read: crate::arch::arm::unhalted_read_regions(),
            can_flash,
        }
    }

    fn halt_and_read(
        &mut self,
        mut func: impl FnMut(&mut probe_rs::Core) -> Result<()>,
    ) -> Result<()> {
        let mut core = self.session.core(0)?;

        if self.unhalted_reads {
            func(&mut core)
        } else {
            let halted = if self.halted == 0 && !core.core_halted()? {
                core.halt(std::time::Duration::from_millis(1000))?;
                true
            } else {
                false
            };

            let rval = func(&mut core);

            if halted {
                core.run()?;
            }

            rval
        }
    }
}

#[rustfmt::skip::macros(anyhow, bail)]
impl Core for ProbeCore {
    fn info(&self) -> (String, Option<String>) {
        let ident = format!(
            "{}, VID {:04x}, PID {:04x}",
            self.identifier, self.vendor_id, self.product_id
        );

        (ident, self.serial_number.clone())
    }

    fn read_word_32(&mut self, addr: u32) -> Result<u32> {
        log::trace!("reading word at {:x}", addr);
        let mut rval = 0;

        if let Some(range) = self.unhalted_read.range(..=addr).next_back() {
            if addr + 4 < range.0 + range.1 {
                let mut core = self.session.core(0)?;
                return Ok(core.read_word_32(addr)?);
            }
        }

        self.halt_and_read(|core| {
            rval = core.read_word_32(addr)?;
            Ok(())
        })?;

        Ok(rval)
    }

    fn read_8(&mut self, addr: u32, data: &mut [u8]) -> Result<()> {
        if data.len() > CORE_MAX_READSIZE {
            bail!("read of {} bytes at 0x{:x} exceeds max of {}",
                data.len(), addr, CORE_MAX_READSIZE);
        }

        if let Some(range) = self.unhalted_read.range(..=addr).next_back() {
            if addr + (data.len() as u32) < range.0 + range.1 {
                let mut core = self.session.core(0)?;
                return Ok(core.read_8(addr, data)?);
            }
        }

        self.halt_and_read(|core| Ok(core.read_8(addr, data)?))
    }

    // TODO need to bump probe-rs version to support 64bit values
    // for now just upcast everything to match the interface
    fn read_reg(&mut self, reg: Register) -> Result<u64> {
        let mut core = self.session.core(0)?;
        let reg_id = Register::to_u16(&reg).unwrap();

        use num_traits::ToPrimitive;

        Ok(core.read_core_reg(Into::<probe_rs::CoreRegisterAddress>::into(
            reg_id,
        ))? as u64)
    }

    // TODO need to bump probe-rs version to support 64bit values
    // for now just upcast everything to match the interface
    fn write_reg(&mut self, reg: Register, value: u64) -> Result<()> {
        let mut core = self.session.core(0)?;
        let reg_id = Register::to_u16(&reg).unwrap();

        use num_traits::ToPrimitive;

        core.write_core_reg(
            Into::<probe_rs::CoreRegisterAddress>::into(reg_id),
            value as u32,
        )?;

        Ok(())
    }

    fn write_word_32(&mut self, addr: u32, data: u32) -> Result<()> {
        let mut core = self.session.core(0)?;
        core.write_word_32(addr, data)?;
        Ok(())
    }

    fn write_8(&mut self, addr: u32, data: &[u8]) -> Result<()> {
        let mut core = self.session.core(0)?;
        core.write_8(addr, data)?;
        Ok(())
    }

    fn halt(&mut self) -> Result<()> {
        if self.halted == 0 {
            let mut core = self.session.core(0)?;
            core.halt(std::time::Duration::from_millis(1000))?;
        }

        self.halted += 1;
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        self.halted -= 1;

        if self.halted == 0 {
            let mut core = self.session.core(0)?;
            core.run()?;
        }

        Ok(())
    }

    fn step(&mut self) -> Result<()> {
        let mut core = self.session.core(0)?;
        core.step()?;
        Ok(())
    }

    fn init_swv(&mut self) -> Result<()> {
        use probe_rs::architecture::arm::swo::SwoConfig;

        let config = SwoConfig::new(0).set_baud(2_000_000);
        self.session.setup_swv(0, &config)?;

        //
        // Because the probe can have sticky errors, we perform one read
        // (and discard the results) to assure that any further errors
        // are legit.
        //
        let _discard = self.session.read_swo();
        Ok(())
    }

    fn read_swv(&mut self) -> Result<Vec<u8>> {
        Ok(self.session.read_swo()?)
    }

    fn load(&mut self, path: &Path) -> Result<()> {
        #[derive(Debug, Default)]
        struct LoadProgress {
            /// total bytes that need to be erased
            total_erase: usize,

            /// bytes that have been erased
            erased: usize,

            /// total bytes that need to be written
            total_write: usize,

            /// number of bytes that have been written
            written: usize,
        }

        use indicatif::{ProgressBar, ProgressStyle};

        if !self.can_flash {
            bail!("cannot flash without explicitly attaching to flash");
        }

        let progress =
            Rc::new(RefCell::new(LoadProgress { ..Default::default() }));

        let bar = ProgressBar::new(0);

        let progress = flashing::FlashProgress::new(move |event| match event {
            flashing::ProgressEvent::Initialized { flash_layout } => {
                progress.borrow_mut().total_erase = flash_layout
                    .sectors()
                    .iter()
                    .map(|s| s.size() as usize)
                    .sum();

                progress.borrow_mut().total_write = flash_layout
                    .pages()
                    .iter()
                    .map(|s| s.size() as usize)
                    .sum();

                bar.set_style(ProgressStyle::default_bar().template(
                    "humility: erasing [{bar:30}] {bytes}/{total_bytes}",
                ));
                bar.set_length(progress.borrow().total_erase as u64);
            }

            flashing::ProgressEvent::SectorErased { size, .. } => {
                progress.borrow_mut().erased += size as usize;
                bar.set_position(progress.borrow().erased as u64);
            }

            flashing::ProgressEvent::PageProgrammed { size, .. } => {
                let mut progress = progress.borrow_mut();

                if progress.written == 0 {
                    progress.erased = progress.total_erase;
                    bar.set_style(ProgressStyle::default_bar().template(
                        "humility: flashing [{bar:30}] {bytes}/{total_bytes}",
                    ));
                    bar.set_length(progress.total_write as u64);
                }

                progress.written += size as usize;
                bar.set_position(progress.written as u64);
            }

            flashing::ProgressEvent::FinishedProgramming => {
                bar.finish_and_clear();
            }

            _ => {}
        });

        let mut options = flashing::DownloadOptions::default();
        options.progress = Some(&progress);

        if let Err(e) = flashing::download_file_with_options(
            &mut self.session,
            path,
            flashing::Format::Hex,
            options,
        ) {
            bail!("Flash loading failed {:?}", e);
        };

        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        let mut core = self.session.core(0)?;
        core.reset()?;
        Ok(())
    }

    fn op_start(&mut self) -> Result<()> {
        if !self.unhalted_reads {
            self.halt()?;
        }

        Ok(())
    }

    fn op_done(&mut self) -> Result<()> {
        if !self.unhalted_reads {
            self.run()?;
        }

        Ok(())
    }
}
