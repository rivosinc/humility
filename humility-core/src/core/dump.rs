// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{anyhow, bail, Result};

use crate::hubris::*;
use crate::regs::Register;
use std::collections::BTreeMap;
use std::collections::HashMap;
use std::convert::TryInto;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::str;

use goblin::elf::Elf;

use crate::core::Core;

pub struct DumpCore {
    contents: Vec<u8>,
    regions: BTreeMap<u32, (u32, usize)>,
    registers: HashMap<Register, u32>,
}

impl DumpCore {
    pub fn new(dump: &str, hubris: &HubrisArchive) -> Result<DumpCore> {
        let mut file = fs::File::open(dump)?;
        let mut regions = BTreeMap::new();

        let mut contents = Vec::new();
        file.read_to_end(&mut contents)?;

        let elf = Elf::parse(&contents).map_err(|e| {
            anyhow!("failed to parse {} as an ELF file: {}", dump, e)
        })?;

        for phdr in elf.program_headers.iter() {
            if phdr.p_type != goblin::elf::program_header::PT_LOAD {
                continue;
            }

            regions.insert(
                phdr.p_vaddr as u32,
                (phdr.p_memsz as u32, phdr.p_offset as usize),
            );
        }

        Ok(Self { contents, regions, registers: hubris.dump_registers() })
    }

    fn check_offset(&self, addr: u32, rsize: usize, offs: usize) -> Result<()> {
        if rsize + offs <= self.contents.len() {
            return Ok(());
        }

        //
        // This really shouldn't happen, as it means that we have a defined
        // region in a program header for memory that wasn't in fact dumped.
        // Still, this might occur if the dump is truncated or otherwise
        // corrupt; offer a message pointing in that direction.
        //
        bail!(
            "0x{:x} is valid, but offset in dump \
            (0x{:x}) + size (0x{:x}) exceeds max (0x{:x}); \
            is the dump truncated or otherwise corrupt?",
            addr,
            offs,
            rsize,
            self.contents.len()
        );
    }
}

#[rustfmt::skip::macros(bail)]
impl Core for DumpCore {
    fn info(&self) -> (String, Option<String>) {
        ("core dump".to_string(), None)
    }

    fn read_word_32(&mut self, addr: u32) -> Result<u32> {
        let rsize: usize = 4;

        if let Some((&base, &(size, offset))) =
            self.regions.range(..=addr).rev().next()
        {
            if base > addr {
                // fall out to the bail below.
            } else if (addr - base) + rsize as u32 > size {
                bail!(
                    "0x{:x} is valid, but relative to base (0x{:x}), \
                    offset (0x{:x}) exceeds max (0x{:x})",
                    addr, base, (addr - base) + rsize as u32, size
                );
            } else {
                let offs = offset + (addr - base) as usize;

                self.check_offset(addr, rsize, offs)?;

                return Ok(u32::from_le_bytes(
                    self.contents[offs..offs + rsize].try_into().unwrap(),
                ));
            }
        }
        bail!("read from invalid address: 0x{:x}", addr);
    }

    fn read_8(&mut self, addr: u32, data: &mut [u8]) -> Result<()> {
        let rsize = data.len();

        if let Some((&base, &(size, offset))) =
            self.regions.range(..=addr).rev().next()
        {
            if base > addr {
                // fall out to the bail below.
            } else if (addr - base) + rsize as u32 > size {
                bail!(
                    "0x{:x} is valid, but relative to base (0x{:x}), \
                    offset (0x{:x}) exceeds max (0x{:x})",
                    addr, base, (addr - base) + rsize as u32, size
                );
            } else {
                let offs = offset + (addr - base) as usize;
                self.check_offset(addr, rsize, offs)?;

                data[..rsize]
                    .copy_from_slice(&self.contents[offs..rsize + offs]);
                return Ok(());
            }
        }

        bail!("read of {} bytes from invalid address: 0x{:x}", rsize, addr);
    }

    fn read_reg(&mut self, reg: Register) -> Result<u64> {
        if let Some(val) = self.registers.get(&reg) {
            Ok(*val as u64)
        } else {
            bail!("register {} not found in dump", reg);
        }
    }

    fn write_reg(&mut self, _reg: Register, _value: u64) -> Result<()> {
        bail!("cannot write register on a dump");
    }

    fn write_word_32(&mut self, _addr: u32, _data: u32) -> Result<()> {
        bail!("cannot write a word on a dump");
    }

    fn write_8(&mut self, _addr: u32, _data: &[u8]) -> Result<()> {
        bail!("cannot write a byte on a dump");
    }

    fn halt(&mut self) -> Result<()> {
        Ok(())
    }

    fn run(&mut self) -> Result<()> {
        Ok(())
    }

    fn step(&mut self) -> Result<()> {
        bail!("can't step a dump");
    }

    fn init_swv(&mut self) -> Result<()> {
        bail!("cannot enable SWV on a dump");
    }

    fn read_swv(&mut self) -> Result<Vec<u8>> {
        bail!("cannot read SWV on a dump");
    }

    fn is_dump(&self) -> bool {
        true
    }

    fn load(&mut self, _path: &Path) -> Result<()> {
        bail!("Flash loading is not supported on a dump");
    }

    fn reset(&mut self) -> Result<()> {
        bail!("Reset is not supported on a dump");
    }
}
