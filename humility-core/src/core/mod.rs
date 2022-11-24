// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use probe_rs::{Permissions, Probe};

use anyhow::{anyhow, bail, Result};

use crate::hubris::*;
use crate::regs::Register;
use std::convert::TryInto;
use std::path::Path;
use std::str;

mod gdb;
pub use gdb::*;
mod probe;
pub use probe::*;
mod openocd;
pub use openocd::*;
mod unattached;
pub use unattached::*;
mod dump;
pub use dump::*;

pub const CORE_MAX_READSIZE: usize = 65536; // 64K ought to be enough for anyone

pub trait Core {
    fn info(&self) -> (String, Option<String>);
    fn read_word_32(&mut self, addr: u32) -> Result<u32>;
    fn read_8(&mut self, addr: u32, data: &mut [u8]) -> Result<()>;
    fn read_reg(&mut self, reg: Register) -> Result<u64>;
    fn write_reg(&mut self, reg: Register, value: u64) -> Result<()>;
    fn init_swv(&mut self) -> Result<()>;
    fn read_swv(&mut self) -> Result<Vec<u8>>;
    fn write_word_32(&mut self, addr: u32, data: u32) -> Result<()>;
    fn write_8(&mut self, addr: u32, data: &[u8]) -> Result<()>;

    fn halt(&mut self) -> Result<()>;
    fn run(&mut self) -> Result<()>;
    fn step(&mut self) -> Result<()>;
    fn is_dump(&self) -> bool {
        false
    }

    fn read_word_64(&mut self, addr: u32) -> Result<u64> {
        let mut buf = [0; 8];
        self.read_8(addr, &mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    ///
    /// Called to load a flash image.
    ///
    fn load(&mut self, path: &Path) -> Result<()>;

    /// Reset the chip
    fn reset(&mut self) -> Result<()>;

    /// Called before starting a series of operations.  May halt the target if
    /// the target does not allow operations while not halted.  Should not be
    /// intermixed with [`halt`]/[`run`].
    fn op_start(&mut self) -> Result<()> {
        Ok(())
    }

    /// Called after completing a series of operations.  May run the target if
    /// the target does not allow operations while not halted.  Should not be
    /// intermixed with [`halt`]/[`run`].
    fn op_done(&mut self) -> Result<()> {
        Ok(())
    }
}

fn parse_probe(probe: &str) -> (&str, Option<usize>) {
    if probe.contains('-') {
        let pieces: Vec<&str> = probe.split('-').collect();

        if pieces.len() == 2 {
            if let Ok(val) = pieces[1].parse::<usize>() {
                return (pieces[0], Some(val));
            }
        }
    }
    (probe, None)
}

fn get_usb_probe(index: Option<usize>) -> Result<probe_rs::DebugProbeInfo> {
    let probes = Probe::list_all();

    if probes.is_empty() {
        bail!("no debug probe found; is it plugged in?");
    }

    if let Some(index) = index {
        if index < probes.len() {
            Ok(probes[index].clone())
        } else {
            bail!(
                "index ({}) exceeds max probe index ({})",
                index,
                probes.len() - 1
            );
        }
    } else if probes.len() == 1 {
        Ok(probes[0].clone())
    } else {
        bail!(
            "multiple USB probes detected; must \
                       explicitly append index (e.g., \"-p usb-0\")"
        );
    }
}

#[rustfmt::skip::macros(anyhow, bail)]
pub fn attach_to_probe(probe: &str) -> Result<Box<dyn Core>> {
    let (probe, index) = parse_probe(probe);

    match probe {
        "usb" => {
            let probe_info = get_usb_probe(index)?;

            let res = probe_info.open();

            if let Err(probe_rs::DebugProbeError::Usb(Some(ref err))) = res {
                if let Some(rcode) = err.downcast_ref::<rusb::Error>() {
                    if *rcode == rusb::Error::Busy {
                        bail!(
                            "USB link in use; is OpenOCD or \
                            another debugger running?"
                        );
                    }
                }
            }

            let probe = res?;

            crate::msg!("Opened probe {}", probe_info.identifier);
            Ok(Box::new(UnattachedCore::new(
                probe,
                probe_info.identifier.clone(),
                probe_info.vendor_id,
                probe_info.product_id,
                probe_info.serial_number,
            )))
        }
        "ocd" | "ocdgdb" | "jlink" => {
            bail!("Probe only attachment with {} is not supported", probe)
        }
        "auto" => attach_to_probe("usb"),
        _ => match TryInto::<probe_rs::DebugProbeSelector>::try_into(probe) {
            Ok(selector) => {
                let vidpid = probe;
                let vid = selector.vendor_id;
                let pid = selector.product_id;
                let serial = selector.serial_number.clone();
                let probe = probe_rs::Probe::open(selector)?;
                let name = probe.get_name();

                crate::msg!("Opened {} via {}", vidpid, name);
                Ok(Box::new(UnattachedCore::new(probe, name, vid, pid, serial)))
            }
            Err(_) => Err(anyhow!("unrecognized probe: {}", probe)),
        },
    }
}

#[rustfmt::skip::macros(anyhow, bail)]
pub fn attach_to_chip(
    probe: &str,
    hubris: &HubrisArchive,
    chip: Option<&str>,
) -> Result<Box<dyn Core>> {
    let (probe, dev_specifier) = parse_probe(probe);

    match probe {
        "usb" => {
            let probe_info = get_usb_probe(dev_specifier)?;

            let res = probe_info.open();

            if let Err(probe_rs::DebugProbeError::Usb(Some(ref err))) = res {
                if let Some(rcode) = err.downcast_ref::<rusb::Error>() {
                    if *rcode == rusb::Error::Busy {
                        bail!(
                            "USB link in use; is OpenOCD or \
                            another debugger running?"
                        );
                    }
                }
            }

            let probe = res?;

            let name = probe.get_name();

            //
            // probe-rs needs us to specify a chip that it knows about -- but
            // it only really uses this information for flashing the part.  If
            // we are attaching to the part for not pusposes of flashing, we
            // specify a generic ARMv7-M (but then we also indicate that can't
            // flash to assure that we can fail explicitly should flashing be
            // attempted).
            //
            let (session, can_flash) = match chip {
                Some(chip) => (probe.attach(chip, Permissions::new())?, true),
                None => (
                    probe.attach(
                        hubris.arch.as_ref().unwrap().get_generic_chip(),
                        Permissions::new(),
                    )?,
                    false,
                ),
            };

            crate::msg!("attached via {}", name);

            Ok(Box::new(ProbeCore::new(
                session,
                probe_info.identifier.clone(),
                probe_info.vendor_id,
                probe_info.product_id,
                probe_info.serial_number,
                hubris.unhalted_reads(),
                can_flash,
            )))
        }

        "ocd" => {
            let mut core = OpenOCDCore::new()?;
            let version = core.sendcmd("version")?;

            if !version.contains("Open On-Chip Debugger") {
                bail!("version string unrecognized: \"{}\"", version);
            }

            crate::msg!("attached via OpenOCD");

            Ok(Box::new(core))
        }

        "auto" => {
            if let Ok(probe) = attach_to_chip("ocd", hubris, chip) {
                return Ok(probe);
            }

            if let Ok(probe) = attach_to_chip("jlink", hubris, chip) {
                return Ok(probe);
            }

            // Try the two most common qemu ports
            if let Ok(probe) = attach_to_chip("qemu-1234", hubris, chip) {
                return Ok(probe);
            }

            if let Ok(probe) = attach_to_chip("qemu-3333", hubris, chip) {
                return Ok(probe);
            }

            attach_to_chip("usb", hubris, chip)
        }

        "ocdgdb" => {
            let core = GDBCore::new(GDBServer::OpenOCD)?;
            crate::msg!("attached via OpenOCD's GDB server");

            Ok(Box::new(core))
        }

        "jlink" => {
            let core = GDBCore::new(GDBServer::JLink)?;
            crate::msg!("attached via JLink");

            Ok(Box::new(core))
        }

        "qemu" => {
            let core = GDBCore::new(GDBServer::Qemu(
                dev_specifier.unwrap_or(3333) as u16,
            ))?;
            crate::msg!("attached via {:?} GDB server", core.server);

            Ok(Box::new(core))
        }

        _ => match TryInto::<probe_rs::DebugProbeSelector>::try_into(probe) {
            Ok(selector) => {
                let vidpid = probe;

                let vid = selector.vendor_id;
                let pid = selector.product_id;
                let serial = selector.serial_number.clone();

                let probe = probe_rs::Probe::open(selector)?;
                let name = probe.get_name();

                //
                // See the block comment in the generic "usb" attach for
                // why we use armv7m here.
                //
                let (session, can_flash) = match chip {
                    Some(chip) => {
                        (probe.attach(chip, Permissions::new())?, true)
                    }
                    None => (
                        probe.attach(
                            hubris.arch.as_ref().unwrap().get_generic_chip(),
                            Permissions::new(),
                        )?,
                        false,
                    ),
                };

                crate::msg!("attached to {} via {}", vidpid, name);

                Ok(Box::new(ProbeCore::new(
                    session,
                    name,
                    vid,
                    pid,
                    serial,
                    hubris.unhalted_reads(),
                    can_flash,
                )))
            }
            Err(_) => Err(anyhow!("unrecognized probe: {}", probe)),
        },
    }
}

pub fn attach_for_flashing(
    probe: &str,
    hubris: &HubrisArchive,
    chip: &str,
) -> Result<Box<dyn Core>> {
    attach_to_chip(probe, hubris, Some(chip))
}

pub fn attach(probe: &str, hubris: &HubrisArchive) -> Result<Box<dyn Core>> {
    attach_to_chip(probe, hubris, None)
}

pub fn attach_dump(
    dump: &str,
    hubris: &HubrisArchive,
) -> Result<Box<dyn Core>> {
    let core = DumpCore::new(dump, hubris)?;
    crate::msg!("attached to dump");
    Ok(Box::new(core))
}
