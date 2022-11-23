// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! ## `humility pmp`
//!
//! On riscv platforms the pmp is used to prevent umode access to certain memory regions.
//! This tool will decode the pmp csrs and output the memory regions and permissons.
//! Often paired with `humility map`.
//!
//! To better understand the memory that a task is allowed to access, one can
//! run the `humility pmp` command, which shows the memory regions that have
//! been granted u mode access.
//!
//! ```console
//! % humility pmp
//! humility: attached via OpenOCD
//! DESC       LOW          HIGH          SIZE ATTR  MODE
//! pmpaddr00   0x106000 -   0x107fff    2000 r-x-  NAPOT
//! pmpaddr01   0x141800 -   0x141bff     400 rw--  NAPOT
//! pmpaddr02        0x0 -       0x1f      20 ----  NAPOT
//! pmpaddr03        0x0 -       0x1f      20 ----  NAPOT
//! pmpaddr04        0x0 -       0x1f      20 ----  NAPOT
//! pmpaddr05        0x0 -       0x1f      20 ----  NAPOT
//! pmpaddr06        0x0 -       0x1f      20 ----  NAPOT
//! pmpaddr07        0x0 -       0x1f      20 ----  NAPOT
//! ```
//!

use anyhow::{bail, Result};
use bit_field::BitField;
use clap::Command as ClapCommand;
use clap::{CommandFactory, Parser};
use humility::regs::rv::RVRegister;
use humility_cmd::{Archive, Attach, Command, Validate};
use riscv::register::{Mode, PmpAddr, PmpCfg};
use std::iter::zip;

#[derive(Parser, Debug)]
#[clap(name = "pmp", about = env!("CARGO_PKG_DESCRIPTION"))]
struct PmpArgs {}

fn pmpcmd(context: &mut humility::ExecutionContext) -> Result<()> {
    let hubris = context.archive.as_ref().unwrap();
    let core = &mut **context.core.as_mut().unwrap();

    match hubris.arch.as_ref().unwrap().get_e_machine() {
        goblin::elf::header::EM_RISCV => (),
        _ => bail!("`humility pmp` only supports riscv"),
    }

    // place for all the pmpaddr
    let mut pmpaddrs = Vec::new();

    // read out all the pmpaddr csr using the csr number
    let base_addr: u32 = RVRegister::PMPADDR0 as u32;
    let end_addr: u32 = RVRegister::PMPADDR63 as u32;
    // try to read all of the pmpaddr registers
    for reg in base_addr..end_addr {
        let pmpaddr = core.read_reg(
            // unwrap should always pass since pmpaddr are continuous
            hubris.arch.as_ref().unwrap().register_from_id(reg).unwrap(),
        );
        // not all pmpaddrs will be implemented, so a read may fail
        // this means the csr is not implemented
        // so stop after the last valid csr.
        if let Err(_err) = pmpaddr {
            break;
        }
        let pmpaddr: PmpAddr = (pmpaddr? as usize).into();
        pmpaddrs.push(pmpaddr);
    }

    // repeat with pmpcfgs
    let mut pmpcfgcsrs = Vec::new();
    let base_addr: u32 = RVRegister::PMPCFG0 as u32;
    let end_addr: u32 = RVRegister::PMPCFG15 as u32;

    // add a flag so we can skip every other PMPCFGX csr
    let mut missed = false;

    // read all the pmpcfgs
    for reg in base_addr..end_addr {
        let csr = core.read_reg(
            hubris.arch.as_ref().unwrap().register_from_id(reg).unwrap(),
        );
        // if the pmpcfg is unavaliable, then we have reached the end of the implemented csrs
        match csr {
            Err(_err) => {
                // only break if we already missed once, this will support rv64 where only even PMPCFG
                // are implemented
                if missed {
                    break;
                }
                missed = true;
            }
            Ok(csr) => {
                pmpcfgcsrs.push(csr);
            }
        }
    }

    // unroll all the pmpcfgcsr into individual configs
    let mut pmpcfgs = Vec::new();
    for pmpcfgcsr in pmpcfgcsrs.iter() {
        // assumes 32bit system, so 4 cfgs per csr
        for j in 0..4 {
            // each config is 1byte.
            let bits: u8 =
                pmpcfgcsr.get_bits(j * 8..((j + 1) * 8)).try_into().unwrap();
            pmpcfgs.push(PmpCfg { byte: bits });
        }
    }

    println!(
        "{:9} {:10}   {:10} {:>7} {:5} {:5}",
        "DESC", "LOW", "HIGH", "SIZE", "ATTR", "MODE",
    );

    // iterate through each pmp with the corresponding config and decode it into a address range
    // with permissions
    for (i, (cfg, pmpaddr)) in zip(pmpcfgs, &pmpaddrs).enumerate() {
        let mode = cfg.get_mode();
        let (addr, size) = pmpaddr.decode(mode);
        match mode {
            Mode::NAPOT => println!(
                "pmpaddr{:02} {:#10x} - {:#10x} {:#7x} {}{}{}{:<2} {:#5?}",
                i,
                addr.unwrap(),
                addr.unwrap() + (size.unwrap().get()) - 1,
                size.unwrap(),
                if cfg.get_permission() as u8 & 0x1 != 0 { "r" } else { "-" },
                if cfg.get_permission() as u8 & 0x2 != 0 { "w" } else { "-" },
                if cfg.get_permission() as u8 & 0x4 != 0 { "x" } else { "-" },
                if cfg.check_locked() { "l" } else { "-" },
                mode,
            ),
            Mode::NA4 => println!(
                "pmpaddr{:02} {:#10x} - {:#10x} {:7x} {}{}{}{:<2} {:#5?}",
                i,
                addr.unwrap(),
                addr.unwrap() + 4 - 1,
                4,
                if cfg.get_permission() as u8 & 0x1 != 0 { "r" } else { "-" },
                if cfg.get_permission() as u8 & 0x2 != 0 { "w" } else { "-" },
                if cfg.get_permission() as u8 & 0x4 != 0 { "x" } else { "-" },
                if cfg.check_locked() { "l" } else { "-" },
                mode,
            ),
            Mode::TOR => {
                // top of range (TOR) uses the previous pmpaddr for the start
                let start = if i == 0 {
                    0
                } else {
                    // the 0 element is the address
                    pmpaddrs[i - 1].decode(Mode::TOR).0.unwrap()
                };
                println!(
                    "pmpaddr{:02} {:#10x} - {:#10x} {:7x} {}{}{}{:<2} {:#5?}",
                    i,
                    start,
                    addr.unwrap() - 1,
                    addr.unwrap() - start - 1,
                    if cfg.get_permission() as u8 & 0x1 != 0 {
                        "r"
                    } else {
                        "-"
                    },
                    if cfg.get_permission() as u8 & 0x2 != 0 {
                        "w"
                    } else {
                        "-"
                    },
                    if cfg.get_permission() as u8 & 0x4 != 0 {
                        "x"
                    } else {
                        "-"
                    },
                    if cfg.check_locked() { "l" } else { "-" },
                    mode,
                );
            }
            // no need to display pmps that are off
            Mode::OFF => (),
        }
    }

    Ok(())
}

/// This is some init right here
pub fn init() -> (Command, ClapCommand<'static>) {
    (
        Command::Attached {
            name: "pmp",
            archive: Archive::Required,
            attach: Attach::Any,
            validate: Validate::Booted,
            run: pmpcmd,
        },
        PmpArgs::command(),
    )
}
