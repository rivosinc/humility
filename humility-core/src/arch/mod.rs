// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::arm::ARMArch;
use crate::arch::rv::RVArch;
use crate::hubris::{HubrisArchive, HubrisStruct, HubrisTarget};
use crate::regs::Register;
use anyhow::{bail, Result};
use capstone::prelude::*;
use std::collections::BTreeMap;
use std::fmt::Debug;

pub mod arm;
pub mod rv;
pub mod uhsize;
use uhsize::UhSize;

pub trait Arch {
    ///
    /// Return elf header `e_machine` for this architecture
    ///
    fn get_e_machine(&self) -> u16;

    ///
    /// Return elf header byte `EI_CLASS` from `e_ident` for this architecture
    ///
    fn get_ei_class(&self) -> u8;

    /// 
    /// Return the number of bits in a word
    ///
    fn get_bits(&self) -> usize;

    ///
    /// Returns the instruction used to trigger a syscall
    ///
    fn get_syscall_insn(&self) -> u32;
    ///
    /// Return the register used as the return addr for function callsj
    ///
    fn get_ret_reg(&self) -> Register;
    ///
    /// Return the stack pointer register
    ///
    fn get_sp(&self) -> Register;
    ///
    /// Return the pc register
    ///
    fn get_pc(&self) -> Register;

    ///
    /// Returns all the general purpose/ integer registers
    ///
    fn get_all_gpr(&self) -> Vec<Register>;

    ///
    /// Return all the registers Humlility supports
    ///
    fn get_all_registers(&self) -> Vec<Register>;

    ///
    /// Convert a dwarf id into a Register object
    ///
    fn register_from_dwarf_id(&self, id: u32) -> Result<Register>;

    fn register_from_id(&self, id: u32) -> Result<Register>;

    fn get_syscall_register(&self, arg_number: u8) -> Result<Register>;

    fn get_generic_chip(&self) -> String;

    fn presyscall_pushes(
        &self,
        cs: &Capstone,
        instrs: &[capstone::Insn],
    ) -> Result<Vec<Register>>;

    fn read_saved_task_regs(
        &self,
        regs: &[u8],
        state: &HubrisStruct,
        hubris: &HubrisArchive,
        core: &mut dyn crate::core::Core,
    ) -> Result<BTreeMap<Register, u32>>;

    fn make_capstone(&self) -> Result<Capstone>;

    ///
    /// On some architectures, function pointers contain special bits.
    /// These must be cleared to get a true address.
    /// (arm has the thumb bit)
    ///
    fn extract_fn_pointer(&self, data: &mut UhSize) {}

    fn instr_operands(
        &self,
        cs: &Capstone,
        instr: &capstone::Insn,
    ) -> Vec<Register>;

    fn instr_branch_target(
        &self,
        cs: &Capstone,
        isntr: &capstone::Insn,
    ) -> Option<HubrisTarget>;
}

impl Debug for dyn Arch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "dyn Arch")
    }
}

pub fn get_arch(arch: u16, abi_size: u8) -> Box<dyn Arch> {
    match arch {
        goblin::elf::header::EM_ARM => Box::new(ARMArch::new()),
        goblin::elf::header::EM_RISCV => Box::new(RVArch::new(abi_size)),
        _ => unimplemented!(),
    }
}

pub fn instr_source_target(
    cs: &Capstone,
    instr: &capstone::Insn,
) -> Result<(Option<Register>, Option<Register>)> {
    let detail = cs.insn_detail(instr).unwrap();

    // Extract source register, there should only be one
    let source_ids = detail.regs_read();
    let source_id = match source_ids.len() {
        0 => None,
        1 => source_ids.first(),
        _ => bail!("multiple source registers"),
    };

    // Extract target register, there should only be one
    let target_ids = detail.regs_write();
    let target_id = match target_ids.len() {
        0 => None,
        1 => source_ids.first(),
        _ => bail!("multiple source registers"),
    };

    // Map RegId onto the Register enum
    let (source, target) = match detail.arch_detail() {
        ArchDetail::ArmDetail(_detail) => {
            let source = source_id.map(|id| Register::Arm(id.into()));
            let target = target_id.map(|id| Register::Arm(id.into()));
            (source, target)
        }
        ArchDetail::RiscVDetail(_detail) => {
            let source = source_id.map(|id| Register::RiscV(id.into()));
            let target = target_id.map(|id| Register::RiscV(id.into()));
            (source, target)
        }
        _ => unimplemented!(),
    };

    Ok((source, target))
}

pub fn readreg(rname: &str, regs: &[u8], state: &HubrisStruct) -> Result<u32> {
    let o = state.lookup_member(rname)?.offset as usize;
    Ok(u32::from_le_bytes(regs[o..o + 4].try_into().unwrap()))
}
