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
use clap::{CommandFactory, Parser};
use humility_arch_reg::rv::RVRegister;
use humility_cli::ExecutionContext;
use humility_cmd::{Archive, Attach, Command, CommandKind, Validate};
use riscv::register::{Permission, Pmpcsr, Range};
use std::iter::zip;

#[derive(Parser, Debug)]
#[clap(name = "pmp", about = env!("CARGO_PKG_DESCRIPTION"))]
struct PmpArgs {}

fn pmpcmd(context: &mut ExecutionContext) -> Result<()> {
    let core = &mut **context.core.as_mut().unwrap();
    let hubris = context.archive.as_ref().unwrap();
    match hubris.arch.as_ref().unwrap().get_e_machine() {
        goblin::elf::header::EM_RISCV => (),
        _ => bail!("`humility pmp` only supports riscv"),
    }

    // place for all the pmpaddr
    let mut pmpaddrs = Vec::new();

    core.op_start()?;

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
        pmpaddrs.push(pmpaddr?);
    }

    // repeat with pmpcfgs
    let mut pmpcfgcsrs = Vec::new();
    let base_addr: u32 = RVRegister::PMPCFG0 as u32;
    let end_addr: u32 = RVRegister::PMPCFG15 as u32;

    // read all the pmpcfgs
    for reg in base_addr..end_addr {
        let csr = core.read_reg(
            hubris.arch.as_ref().unwrap().register_from_id(reg).unwrap(),
        );
        // if the pmpcfg is unavailable, then we have reached the end of the implemented csrs
        match csr {
            Err(_err) => {
                // assumes 32bit system, pmpcfg are contigus
                break;
            }
            Ok(csr) => {
                pmpcfgcsrs.push(csr);
            }
        }
    }
    core.op_done()?;

    // unroll all the pmpcfgcsr into individual configs
    let mut pmpcfgs = Vec::new();
    for pmpcfgcsr in pmpcfgcsrs.iter() {
        let bits: usize = (*pmpcfgcsr).try_into()?;
        let csr = Pmpcsr { bits };
        // assumes 32bit system, so 4 cfgs per csr
        for idx in 0..4 {
            pmpcfgs.push(csr.into_config(idx));
        }
    }

    println!(
        "{:9}  {:10}   {:10}  {:10}  {:5}  {:5}",
        "DESC", "LOW", "HIGH", "SIZE", "ATTR", "MODE",
    );

    let perm_to_str = |perm: Permission| match perm {
        Permission::NONE => "---",
        Permission::R => "r--",
        Permission::W => "-w-",
        Permission::RW => "rw-",
        Permission::X => "--x",
        Permission::RX => "r-x",
        Permission::WX => "-wx",
        Permission::RWX => "rwx",
    };

    // iterate through each pmp with the corresponding config and decode it into a address range
    // with permissions
    for (i, (cfg, pmpaddr)) in zip(pmpcfgs, &pmpaddrs).enumerate() {
        let mode = cfg.range;
        let addr = pmpaddr << 2;
        match mode {
            Range::NAPOT => {
                let size_shift = pmpaddr.trailing_ones();
                let mask = (1 << size_shift) - 1;
                let addr = (pmpaddr & !mask) << 2;
                let size = 1 << (size_shift + 2);
                println!(
                    "pmpaddr{:02}  {:#10x} - {:#10x}  {:<#10x}  {}{:<2}  {:#5?}",
                    i,
                    addr,
                    addr + size - 1,
                    size,
                    perm_to_str(cfg.permission),
                    if cfg.locked { "l" } else { "-" },
                    mode,
                )
            }
            Range::NA4 => println!(
                "pmpaddr{:02}  {:#10x} - {:#10x}  {:<#10x}  {}{:<2}  {:#5?}",
                i,
                addr,
                addr + 4 - 1,
                4,
                perm_to_str(cfg.permission),
                if cfg.locked { "l" } else { "-" },
                mode,
            ),
            Range::TOR => {
                // top of range (TOR) uses the previous pmpaddr for the start
                let start = if i == 0 {
                    0
                } else {
                    // the 0 element is the address
                    pmpaddrs[i - 1]
                };
                println!(
                    "pmpaddr{:02}  {:#10x} - {:#10x}  {:<#10x}  {}{:<2}  {:#5?}",
                    i,
                    start,
                    addr - 1,
                    addr - start - 1,
                    perm_to_str(cfg.permission),
                    if cfg.locked { "l" } else { "-" },
                    mode,
                );
            }
            // no need to display pmps that are off
            Range::OFF => (),
        }
    }

    Ok(())
}

/// This is some init right here
pub fn init() -> Command {
    Command {
        app: PmpArgs::command(),
        name: "pmp",
        run: pmpcmd,
        kind: CommandKind::Attached {
            archive: Archive::Required,
            attach: Attach::Any,
            validate: Validate::Booted,
        },
    }
}
