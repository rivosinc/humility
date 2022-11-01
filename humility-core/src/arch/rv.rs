// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::{readreg, Arch};
use crate::hubris::{HubrisArchive, HubrisStruct, HubrisTarget};
use crate::regs::rv::get_all_registers;
use crate::regs::rv::RVRegister;
use crate::regs::Register;
use anyhow::{anyhow, bail, Result};
use capstone::arch::riscv::{ArchExtraMode, ArchMode, RiscVInsn, RiscVOperand};
use capstone::arch::ArchOperand;
use capstone::prelude::*;
use capstone::{Capstone, InsnId};
use num_traits::cast::ToPrimitive;
use num_traits::FromPrimitive;
use std::collections::BTreeMap;
use strum::IntoEnumIterator;

pub struct RVArch {
    pub ei_class: u8,
}

impl RVArch {
    pub fn new(ei_class: u8) -> Self {
        Self { ei_class }
    }
}

impl Arch for RVArch {
    fn get_e_machine(&self) -> u16 {
        goblin::elf::header::EM_RISCV
    }

    fn get_ei_class(&self) -> u8 {
        self.ei_class
    }

    fn get_abi_size(&self) -> u8 {
        match self.ei_class {
            goblin::elf::header::ELFCLASS32 => 32,
            goblin::elf::header::ELFCLASS64 => 64,
            // this wont ever actually match, so we will just return 0...
            _ => unimplemented!(),
        }
    }

    fn get_syscall_insn(&self) -> u32 {
        RiscVInsn::RISCV_INS_ECALL as u32
    }

    fn get_ret_reg(&self) -> Register {
        Register::RiscV(RVRegister::RA)
    }

    fn get_sp(&self) -> Register {
        Register::RiscV(RVRegister::SP)
    }

    fn get_pc(&self) -> Register {
        Register::RiscV(RVRegister::PC)
    }

    fn get_all_gpr(&self) -> Vec<Register> {
        RVRegister::iter()
            .filter(RVRegister::is_general_purpose)
            .map(Register::RiscV)
            .collect()
    }

    fn get_all_registers(&self) -> Vec<Register> {
        RVRegister::iter().map(Register::RiscV).collect()
    }

    //
    // TODO: will first check for `CURRENT_TASK_PTR` then check mscratch/sscratch,
    // When using xscratch, there is a small time when the current task ptr is
    // actually in a0 when handling a trap, we can detect this and read the
    // correct register.
    //
    fn get_current_task_ptr(
        &self,
        hubris: &HubrisArchive,
        core: &mut dyn crate::core::Core,
    ) -> Result<u64> {
        match hubris.lookup_symword("CURRENT_TASK_PTR") {
            Ok(ptr) => Ok(core.read_word_32(ptr)? as u64),
            // Means current task is in mscratch or sscratch
            Err(_) => {
                let task_register = if hubris
                    .manifest
                    .features
                    .contains(&"s-mode".to_owned())
                {
                    log::trace!("using sscratch");
                    RVRegister::SSCRATCH
                } else {
                    log::trace!("using mscratch");
                    RVRegister::MSCRATCH
                };
                // TODO right now blindly check scratch, but should check that it is valid
                core.read_reg(Register::RiscV(task_register))
            }
        }
    }

    ///
    /// on RISCV platforms the dwarf id does not match the register id on the bus.
    /// see: https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-dwarf.adoc
    /// See table 3.3 in the riscv debug spec.
    /// (https://raw.githubusercontent.com/riscv/riscv-debug-spec/master/riscv-debug-stable.pdf#table.3.3)
    ///
    fn register_from_dwarf_id(&self, mut id: u32) -> Result<Register> {
        /* "Each CSR is assigned a DWARF register number corresponding to its specified CSR
         * number plus 4096."
         */
        if (4096..8192).contains(&id) {
            id -= 4096;
        }
        /* Integer registers, range [0,31], then fpr [32,63] */
        else if id < 64 {
            id += 0x1000;
        }
        // vector registers [96, 127]
        // TODO: the debug spec is not clear about where these will land,
        // https://raw.githubusercontent.com/riscv/riscv-debug-spec/master/riscv-debug-stable.pdf#table.3.3
        // and currently hubris does not use vector registers
        // This is just a placeholder
        // see jira https://rivosinc.atlassian.net/browse/SW-490
        else if (96..128).contains(&id) {
            unimplemented!();
        }

        RVRegister::from_u32(id)
            .map(Register::RiscV)
            .ok_or_else(|| anyhow!("unsupported dwarf id"))
    }

    fn register_from_id(&self, id: u32) -> Result<Register> {
        RVRegister::from_u32(id)
            .map(Register::RiscV)
            .ok_or_else(|| anyhow!("unsupported id"))
    }

    fn get_syscall_register(&self, arg_number: u8) -> Result<Register> {
        if arg_number > 8 {
            bail!("invalid syscall register number");
        }

        let base_syscall_arg: u32 =
            RVRegister::to_u32(&RVRegister::A0).unwrap();

        self.register_from_id(base_syscall_arg + arg_number as u32)
    }

    fn get_generic_chip(&self) -> String {
        "riscv".to_string()
    }

    fn instr_branch_target(
        &self,
        _cs: &Capstone,
        _isntr: &capstone::Insn,
    ) -> Option<HubrisTarget> {
        None
    }

    //
    // our stub frames (that is, those frames that contain system call
    // instructions) have no DWARF information that describes how to unwind
    // through them; for these frames we do some (very crude) analysis of the
    // program text to determine what registers are pushed and how they are
    // manipulated so we can properly determine register state before the system
    // call. This is currently incomplete as it assumes the registers are stored in order
    // TODO the conditions to push probably need more rigor,
    // but it is not used until the stack unwinding for rv32 is fixed.
    // See jira https://rivosinc.atlassian.net/browse/SW-23
    fn presyscall_pushes(
        &self,
        cs: &Capstone,
        instrs: &[capstone::Insn],
    ) -> Result<Vec<Register>> {
        const RV_INSN_SW: u32 = RiscVInsn::RISCV_INS_SW as u32;
        const RV_INSN_C_SW: u32 = RiscVInsn::RISCV_INS_C_SW as u32;

        let mut rval = vec![];
        for instr in instrs {
            match instr.id() {
                InsnId(RV_INSN_C_SW) | InsnId(RV_INSN_SW) => {
                    for op in self.instr_operands(cs, instr).iter().rev() {
                        rval.push(*op);
                    }
                }
                _ => {}
            }
        }

        rval.reverse();
        Ok(rval)
    }

    ///
    /// extract a task register state when in a syscall
    ///
    fn read_saved_task_regs(
        &self,
        regs: &[u8],
        state: &HubrisStruct,
        _hubris: &HubrisArchive,
        _core: &mut dyn crate::core::Core,
    ) -> Result<BTreeMap<Register, u32>> {
        //
        // Load all of the saved regs found in the structure.
        // On riscv, every register gets saved
        // 0 is the zero register, no need to check
        //
        let mut rval = BTreeMap::new();
        for reg in get_all_registers() {
            log::trace!("reading reg: {}", reg);
            let rname = reg.to_string().to_lowercase();
            let val = readreg(&rname, regs, state);
            if val.is_err() {
                continue;
            }
            let val = val.unwrap();

            rval.insert(reg, val);
        }
        Ok(rval)
    }

    fn make_capstone(&self) -> Result<Capstone> {
        match self.ei_class {
            goblin::elf::header::ELFCLASS32 => Ok(Capstone::new()
                .riscv()
                .mode(ArchMode::RiscV32)
                .extra_mode(std::iter::once(ArchExtraMode::RiscVC))
                .detail(true)
                .build()
                .unwrap()),

            goblin::elf::header::ELFCLASS64 => Ok(Capstone::new()
                .riscv()
                .mode(ArchMode::RiscV64)
                .extra_mode(std::iter::once(ArchExtraMode::RiscVC))
                .detail(true)
                .build()
                .unwrap()),

            _ => bail!("abi size not supported"),
        }
    }

    fn instr_operands(
        &self,
        cs: &Capstone,
        instr: &capstone::Insn,
    ) -> Vec<Register> {
        let detail = cs.insn_detail(instr).unwrap();
        let mut rval: Vec<Register> = Vec::new();

        for op in detail.arch_detail().operands() {
            if let ArchOperand::RiscVOperand(RiscVOperand::Reg(id)) = op {
                let reg: RVRegister = (&id).into();
                rval.push(Register::RiscV(reg));
            }
        }

        rval
    }
}
