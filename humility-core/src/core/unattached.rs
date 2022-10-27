// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use probe_rs::Probe;

use anyhow::{bail, Result};

use crate::regs::Register;
use std::path::Path;

use crate::core::Core;

pub struct UnattachedCore {
    pub probe: probe_rs::Probe,
    pub identifier: String,
    pub vendor_id: u16,
    pub product_id: u16,
    pub serial_number: Option<String>,
}

impl UnattachedCore {
    pub fn new(
        probe: Probe,
        identifier: String,
        vendor_id: u16,
        product_id: u16,
        serial_number: Option<String>,
    ) -> Self {
        Self { probe, identifier, vendor_id, product_id, serial_number }
    }
}

impl Core for UnattachedCore {
    fn info(&self) -> (String, Option<String>) {
        let ident = format!(
            "{}, VID {:04x}, PID {:04x}",
            self.identifier, self.vendor_id, self.product_id
        );

        (ident, self.serial_number.clone())
    }

    fn read_word_32(&mut self, _addr: u32) -> Result<u32> {
        bail!("Unimplemented when unattached!");
    }

    fn read_8(&mut self, _addr: u32, _data: &mut [u8]) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn read_reg(&mut self, _reg: Register) -> Result<u64> {
        bail!("Unimplemented when unattached!");
    }

    fn write_reg(&mut self, _reg: Register, _value: u64) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn write_word_32(&mut self, _addr: u32, _data: u32) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn write_8(&mut self, _addr: u32, _data: &[u8]) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn halt(&mut self) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn run(&mut self) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn step(&mut self) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn init_swv(&mut self) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn read_swv(&mut self) -> Result<Vec<u8>> {
        bail!("Unimplemented when unattached!");
    }

    fn is_dump(&self) -> bool {
        false
    }

    fn load(&mut self, _path: &Path) -> Result<()> {
        bail!("Unimplemented when unattached!");
    }

    fn reset(&mut self) -> Result<()> {
        self.probe.target_reset_assert()?;

        // The closest available documentation on hold time is
        // a comment giving a timeout
        // https://open-cmsis-pack.github.io/Open-CMSIS-Pack-Spec/main/html/debug_description.html#resetHardwareDeassert
        std::thread::sleep(std::time::Duration::from_millis(1000));

        self.probe.target_reset_deassert()?;

        Ok(())
    }
}
