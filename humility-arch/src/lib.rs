// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use anyhow::{bail, Result};
use capstone::prelude::*;
use humility_arch_reg::Register;
use std::collections::BTreeMap;
use std::fmt::Debug;

pub mod arm;
use arm::ARMArch;

#[derive(Copy, Clone, Debug)]
pub enum InstrTarget {
    Direct(u32),
    Indirect,
    Call(u32),
    IndirectCall,
    Return,
}

pub trait Arch: Send {
    ///
    /// Return elf header `e_machine` for this architecture
    ///
    fn get_e_machine(&self) -> u16;

    ///
    /// Return elf header byte `EI_CLASS` from `e_ident` for this architecture
    ///
    fn get_ei_class(&self) -> u8;

    ///
    /// Returns the register size in bits
    ///
    fn get_abi_size(&self) -> u8;

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
    /// Returns all the registers that are saved for a task
    ///
    fn get_saved_task_registers(&self) -> Vec<Register> {
        self.get_all_gpr()
    }

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
        read_reg: &dyn Fn(&str) -> Result<u32>,
        read_mem: &mut dyn FnMut(u32, &mut [u8]) -> Result<()>,
        target: &str,
    ) -> Result<BTreeMap<Register, u32>>;

    fn make_capstone(&self) -> Result<Capstone>;

    ///
    /// On some architectures, function pointers contain special bits.
    /// These must be cleared to get a true address.
    /// (arm has the thumb bit)
    ///
    fn extract_fn_pointer(&self, data: u32) -> u32 {
        data
    }

    fn instr_branch_target(
        &self,
        cs: &Capstone,
        isntr: &capstone::Insn,
    ) -> Option<InstrTarget>;

    fn instr_operands(
        &self,
        cs: &Capstone,
        instr: &capstone::Insn,
    ) -> Vec<Register>;

    /// Returns a list of possible GDB executable names for this architeture
    fn get_gdbs(&self) -> &[&str];
}

impl Debug for dyn Arch {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "dyn Arch")
    }
}

pub fn get_arch(arch: u16) -> Box<dyn Arch> {
    match arch {
        goblin::elf::header::EM_ARM => Box::new(ARMArch::new()),
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
            let source = source_id.map(|id| Register::Arm((*id).into()));
            let target = target_id.map(|id| Register::Arm((*id).into()));
            (source, target)
        }
        _ => unimplemented!(),
    };

    Ok((source, target))
}
