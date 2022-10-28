// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::arch::{instr_source_target, readreg, Arch};
use crate::hubris::{HubrisArchive, HubrisStruct, HubrisTarget};
use crate::regs::arm::{register_from_id, ARMRegister};
use crate::regs::Register;
use anyhow::{anyhow, bail, Result};
use capstone::arch::arm::{
    ArchExtraMode, ArchMode, ArmInsn, ArmOperandType, ArmReg,
};
use capstone::arch::ArchOperand;
use capstone::prelude::*;
use capstone::Capstone;
use capstone::{InsnGroupId, InsnGroupType, InsnId, RegId};
use num_traits::cast::ToPrimitive;
use num_traits::FromPrimitive;
use std::collections::{BTreeMap, HashMap};
use strum::IntoEnumIterator;

pub struct ARMArch {}

impl ARMArch {
    pub fn new() -> Self {
        Self {}
    }

    //
    // ARM normally requires the stack to be 8-byte aligned on function entry.
    // However, because exceptions are asynchronous, on exception entry the
    // stack may need to be aligned by the CPU.  If this is needed, bit 9 is
    // set in the PSR to indicate this.  In our system call stubs, we can
    // make our code simpler (and shave a cycle or two) by knowing that the
    // CPU will do this -- but we must be sure to take that into account when
    // unwinding the stack!  (And we know that we will only be off by 4 bytes;
    // if the bit is set, the needed realignment is 4 -- not 1 or 2.)
    //
    fn exception_stack_realign(regs: &BTreeMap<Register, u32>) -> u32 {
        if let Some(psr) = regs.get(&Register::Arm(ARMRegister::PSR)) {
            if (psr & (1 << 9)) != 0 {
                return 4;
            }
        }

        0
    }
}

impl Default for ARMArch {
    fn default() -> Self {
        Self::new()
    }
}

impl Arch for ARMArch {
    fn get_e_machine(&self) -> u16 {
        goblin::elf::header::EM_ARM
    }

    fn get_ei_class(&self) -> u8 {
        goblin::elf::header::ELFCLASS32
    }

    fn get_abi_size(&self) -> u8 {
        32
    }

    fn get_syscall_insn(&self) -> u32 {
        ArmInsn::ARM_INS_SVC as u32
    }

    fn get_ret_reg(&self) -> Register {
        Register::Arm(ARMRegister::LR)
    }

    fn get_sp(&self) -> Register {
        Register::Arm(ARMRegister::SP)
    }

    fn get_pc(&self) -> Register {
        Register::Arm(ARMRegister::PC)
    }

    fn get_all_gpr(&self) -> Vec<Register> {
        ARMRegister::iter()
            .filter(ARMRegister::is_general_purpose)
            .map(Register::Arm)
            .collect()
    }

    fn get_all_registers(&self) -> Vec<Register> {
        ARMRegister::iter().map(Register::Arm).collect()
    }

    fn register_from_dwarf_id(&self, id: u32) -> Result<Register> {
        ARMRegister::from_u32(id)
            .map(Register::Arm)
            .ok_or_else(|| anyhow!("unsupported dwarf id"))
    }

    fn register_from_id(&self, id: u32) -> Result<Register> {
        ARMRegister::from_u32(id)
            .map(Register::Arm)
            .ok_or_else(|| anyhow!("unsupported id"))
    }

    fn get_syscall_register(&self, arg_number: u8) -> Result<Register> {
        if arg_number > 8 {
            bail!("invalid syscall register number");
        }

        let base_syscall_arg: u32 =
            ARMRegister::to_u32(&ARMRegister::R4).unwrap();

        self.register_from_id(base_syscall_arg + arg_number as u32)
    }

    fn get_generic_chip(&self) -> String {
        "armv7m".to_string()
    }

    fn instr_branch_target(
        &self,
        cs: &Capstone,
        instr: &capstone::Insn,
    ) -> Option<HubrisTarget> {
        // Currently only valid for arm, and the results are only used for ETM
        // No plan to port this for riscv

        let detail = cs.insn_detail(instr).ok()?;

        let mut jump = false;
        let mut call = false;
        let mut brel = None;

        const BREL: u8 = InsnGroupType::CS_GRP_BRANCH_RELATIVE as u8;
        const JUMP: u8 = InsnGroupType::CS_GRP_JUMP as u8;
        const CALL: u8 = InsnGroupType::CS_GRP_CALL as u8;
        const ARM_REG_PC: u16 = ArmReg::ARM_REG_PC as u16;
        const ARM_REG_LR: u16 = ArmReg::ARM_REG_LR as u16;
        const ARM_INSN_POP: u32 = ArmInsn::ARM_INS_POP as u32;

        for g in detail.groups() {
            match g {
                InsnGroupId(BREL) => {
                    let arch = detail.arch_detail();
                    let ops = arch.operands();

                    let op = ops.last().unwrap_or_else(|| {
                        panic!("missing operand!");
                    });

                    if let ArchOperand::ArmOperand(op) = op {
                        if let ArmOperandType::Imm(a) = op.op_type {
                            brel = Some(a as u32);
                        }
                    }
                }

                InsnGroupId(JUMP) => {
                    jump = true;
                }

                InsnGroupId(CALL) => {
                    call = true;
                }
                _ => {}
            }
        }

        if let Some(addr) = brel {
            if call {
                return Some(HubrisTarget::Call(addr));
            } else {
                return Some(HubrisTarget::Direct(addr));
            }
        }

        if call {
            return Some(HubrisTarget::IndirectCall);
        }

        //
        // If this is a JUMP that isn't a CALL, check to see if one of
        // its operands is LR -- in which case it's a return (or could be
        // a return).
        //
        if jump {
            for op in detail.arch_detail().operands() {
                if let ArchOperand::ArmOperand(op) = op {
                    if let ArmOperandType::Reg(RegId(ARM_REG_LR)) = op.op_type {
                        return Some(HubrisTarget::Return);
                    }
                }
            }

            return Some(HubrisTarget::Indirect);
        }

        //
        // Capstone doesn't have a group denoting returns (they are control
        // transfers, but not considered in the JUMP group), so explicitly
        // look for a pop instruction that writes to the PC.
        //
        if let InsnId(ARM_INSN_POP) = instr.id() {
            for op in detail.arch_detail().operands() {
                if let ArchOperand::ArmOperand(op) = op {
                    if let ArmOperandType::Reg(RegId(ARM_REG_PC)) = op.op_type {
                        return Some(HubrisTarget::Return);
                    }
                }
            }
        }

        None
    }

    //
    // On ARM, our stub frames (that is, those frames that contain system call
    // instructions) have no DWARF information that describes how to unwind
    // through them; for these frames we do some (very crude) analysis of the
    // program text to determine what registers are pushed and how they are
    // manipulated so we can properly determine register state before the system
    // call.  Note that this made slightly more challenging by ARMv6-M, which
    // doesn't have as rich push instructions as ARMv7-M/ARMv8-M:  in order for it
    // to push R8 through R11, it must first move them into the lower registers
    // (R0 through R7).  We therefore have to track moves in addition to pushes to
    // determine what landed where -- and yes, this heuristic is incomplete!
    //
    fn presyscall_pushes(
        &self,
        cs: &Capstone,
        instrs: &[capstone::Insn],
    ) -> Result<Vec<Register>> {
        const ARM_INSN_PUSH: u32 = ArmInsn::ARM_INS_PUSH as u32;
        const ARM_INSN_MOV: u32 = ArmInsn::ARM_INS_MOV as u32;
        const ARM_INSN_POP: u32 = ArmInsn::ARM_INS_POP as u32;

        let mut map = HashMap::new();
        let mut rval = vec![];

        for instr in instrs {
            match instr.id() {
                InsnId(ARM_INSN_MOV) => {
                    let (source, target) = instr_source_target(cs, instr)?;

                    if let (Some(source), Some(target)) = (source, target) {
                        map.insert(target, source);
                    }
                }

                InsnId(ARM_INSN_PUSH) => {
                    for op in self.instr_operands(cs, instr).iter().rev() {
                        rval.push(if let Some(source) = map.get(op) {
                            *source
                        } else {
                            *op
                        });
                    }
                }

                InsnId(ARM_INSN_POP) => {
                    for _ in self.instr_operands(cs, instr).iter() {
                        rval.pop();
                    }
                }

                _ => {}
            }
        }

        //
        // What we have now is the order that registers were pushed onto the
        // stack.  The addressing order is naturally the inverse of this, so
        // we reverse it before handing it back.
        //
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
        hubris: &HubrisArchive,
        core: &mut dyn crate::core::Core,
    ) -> Result<BTreeMap<Register, u32>> {
        let mut rval = BTreeMap::new();
        //
        // Load all of the syscall regs found in the structure.
        // Only these are stored in SavedState, the rest are on stack.
        //
        for r in 4..=11 {
            let rname = format!("r{}", r);
            let val = readreg(&rname, regs, state)?;
            let reg = register_from_id(r).unwrap();
            rval.insert(reg, val);
        }

        let sp = readreg("psp", regs, state)?;

        const NREGS_CORE: usize = 8;

        let mut stack: Vec<u8> = vec![];
        stack.resize_with(NREGS_CORE * 4, Default::default);
        core.read_8(sp, stack.as_mut_slice())?;

        //
        // R0-R3, and then R12, LR and the PSR are found on the stack
        //
        for r in 0..NREGS_CORE {
            let o = r * 4;
            let val = u32::from_le_bytes(stack[o..o + 4].try_into()?);

            //Why are these chosen??
            let reg = match r {
                0 | 1 | 2 | 3 => register_from_id(r.try_into()?).unwrap(),
                4 => Register::Arm(ARMRegister::R12),
                5 => Register::Arm(ARMRegister::LR),
                6 => Register::Arm(ARMRegister::PC),
                7 => Register::Arm(ARMRegister::PSR),
                _ => panic!("bad register value"),
            };

            rval.insert(reg, val);
        }

        //
        // Not all architectures have floating point -- and ARMv6 never has
        // it.  (Note that that the FP contents pushed onto the stack is
        // always 8-byte aligned; if we have our 17 floating point registers
        // here, we also have an unstored pad.)
        //
        let (nregs_fp, align) = if hubris.manifest.target.as_ref().unwrap()
            == "thumbv6m-none-eabi"
        {
            (0, 0)
        } else {
            (17, 1)
        };

        let nregs_frame: usize = NREGS_CORE + nregs_fp + align;

        //
        // We manually adjust our stack pointer to peel off the entire frame,
        // plus any needed re-alignment.
        //
        let adjust =
            (nregs_frame as u32) * 4 + ARMArch::exception_stack_realign(&rval);

        rval.insert(Register::Arm(ARMRegister::SP), sp + adjust);

        Ok(rval)
    }

    fn make_capstone(&self) -> Result<Capstone> {
        Ok(Capstone::new()
            .arm()
            .mode(ArchMode::Thumb)
            .extra_mode(std::iter::once(ArchExtraMode::MClass))
            .detail(true)
            .build()
            .unwrap())
    }

    fn extract_fn_pointer(&self, data: u32) -> u32 {
        data & !1
    }

    fn instr_operands(
        &self,
        cs: &Capstone,
        instr: &capstone::Insn,
    ) -> Vec<Register> {
        let detail = cs.insn_detail(instr).unwrap();
        let mut rval: Vec<Register> = Vec::new();

        for op in detail.arch_detail().operands() {
            if let ArchOperand::ArmOperand(op) = op {
                if let ArmOperandType::Reg(id) = op.op_type {
                    let reg: ARMRegister = (&id).into();
                    rval.push(Register::Arm(reg));
                }
            }
        }

        rval
    }
}

/// Looks up the jump target type of the previously-disassembled instruction
/// at `addr`. Returns `None` if the instruction was did not affect control
/// flow.
///
/// TODO: this also returns `None` if `addr` is not an instruction boundary,
/// which is probably wrong but we haven't totally thought it through yet.
pub fn arm_instr_target(
    hubris: &HubrisArchive,
    addr: u32,
) -> Option<HubrisTarget> {
    // Target is only used for ETM so no plan to port branch targets to riscv
    //
    hubris.instrs.get(&addr).and_then(|&(_, target)| target)
}

pub fn unhalted_read_regions() -> BTreeMap<u32, u32> {
    let mut map = BTreeMap::new();

    //
    // On ARM, the PPB is mapped at 0xe000_0000 and runs for 1MB.  This address
    // range contains the control registers that we need to read to determine
    // the state of the MCU; it can be read without halting the core on all
    // architectures.
    //
    map.insert(0xe000_0000, 1024 * 1024);

    map
}
