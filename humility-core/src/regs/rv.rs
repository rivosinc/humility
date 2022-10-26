// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::regs::{Register, RegisterField};
use capstone::arch::riscv::RiscVReg::*;
use capstone::RegId;
use num_traits::ToPrimitive;
use strum::IntoEnumIterator;
use strum_macros::EnumIter;

#[allow(non_camel_case_types)]
#[derive(
    Copy,
    Clone,
    Debug,
    Hash,
    FromPrimitive,
    ToPrimitive,
    PartialEq,
    Eq,
    Ord,
    PartialOrd,
    EnumIter,
)]

///
/// See table 3.3 in the riscv debug spec.
/// (https://raw.githubusercontent.com/riscv/riscv-debug-spec/master/riscv-debug-stable.pdf#table.3.3)
/// The pc is read through the DPC register
///
pub enum RVRegister {
    CSR_START = 0x0,
    MSTATUS = 0x300,
    MISA,
    MEDELEG,
    MIDELEG,
    MIE,
    MTVEC,
    MSTATUSH = 0x310,
    MSCRATCH = 0x340,
    MEPC,
    MCAUSE,
    MTVAL,
    MIP,
    PMPCFG0 = 0x3a0,
    PMPCFG1,
    PMPCFG2,
    PMPCFG3,
    PMPCFG4,
    PMPCFG5,
    PMPCFG6,
    PMPCFG7,
    PMPCFG8,
    PMPCFG9,
    PMPCFG10,
    PMPCFG11,
    PMPCFG12,
    PMPCFG13,
    PMPCFG14,
    PMPCFG15,
    PMPADDR0 = 0x3b0,
    PMPADDR1,
    PMPADDR2,
    PMPADDR3,
    PMPADDR4,
    PMPADDR5,
    PMPADDR6,
    PMPADDR7,
    PMPADDR8,
    PMPADDR9,
    PMPADDR10,
    PMPADDR11,
    PMPADDR12,
    PMPADDR13,
    PMPADDR14,
    PMPADDR15,
    PMPADDR16,
    PMPADDR17,
    PMPADDR18,
    PMPADDR19,
    PMPADDR20,
    PMPADDR21,
    PMPADDR22,
    PMPADDR23,
    PMPADDR24,
    PMPADDR25,
    PMPADDR26,
    PMPADDR27,
    PMPADDR28,
    PMPADDR29,
    PMPADDR30,
    PMPADDR31,
    PMPADDR32,
    PMPADDR33,
    PMPADDR34,
    PMPADDR35,
    PMPADDR36,
    PMPADDR37,
    PMPADDR38,
    PMPADDR39,
    PMPADDR40,
    PMPADDR41,
    PMPADDR42,
    PMPADDR43,
    PMPADDR44,
    PMPADDR45,
    PMPADDR46,
    PMPADDR47,
    PMPADDR48,
    PMPADDR49,
    PMPADDR50,
    PMPADDR51,
    PMPADDR52,
    PMPADDR53,
    PMPADDR54,
    PMPADDR55,
    PMPADDR56,
    PMPADDR57,
    PMPADDR58,
    PMPADDR59,
    PMPADDR60,
    PMPADDR61,
    PMPADDR62,
    PMPADDR63,
    MSECCFG = 0x747,
    MSECCFGH = 0x757,
    DCSR = 0x7b0,
    PC = 0x7b1,
    CSR_END = 0xFFF,
    // ZERO is the start of GPR
    ZERO = 0x1000,
    RA,
    SP,
    GP,
    TP,
    T0,
    T1,
    T2,
    S0,
    S1,
    A0,
    A1,
    A2,
    A3,
    A4,
    A5,
    A6,
    A7,
    S2,
    S3,
    S4,
    S5,
    S6,
    S7,
    S8,
    S9,
    S10,
    S11,
    T3,
    T4,
    T5,
    T6,
    FPR_START = 0x1020,
    FPR_END = 0x103F,
    CUSTOM_START = 0xC000,
    CUSTOM_END = 0xFFFF,
}

impl RVRegister {
    pub fn is_general_purpose(&self) -> bool {
        self >= &RVRegister::ZERO && self <= &RVRegister::T6
    }

    pub fn is_special(&self) -> bool {
        self >= &RVRegister::CSR_START && self <= &RVRegister::CSR_END
    }

    //TODO currently humility does not use any Riscv floating point registers
    pub fn is_floating_point(&self) -> bool {
        self >= &RVRegister::FPR_START && self <= &RVRegister::FPR_END
    }

    pub fn fields(&self) -> Option<Vec<RegisterField>> {
        match self {
            RVRegister::MCAUSE => {
                Some(vec![RegisterField::bit(31, "INTERRUPT")])
            }
            RVRegister::MSTATUS => Some(vec![
                RegisterField::bit(31, "SD"),
                RegisterField::bit(22, "TSR"),
                RegisterField::bit(21, "TW"),
                RegisterField::bit(20, "TVM"),
                RegisterField::bit(19, "MXR"),
                RegisterField::bit(18, "SUM"),
                RegisterField::bit(17, "MPRV"),
                RegisterField::field(16, 15, "XS"),
                RegisterField::field(14, 13, "FS"),
                RegisterField::field(12, 11, "MPP"),
                RegisterField::field(10, 9, "VS"),
                RegisterField::bit(8, "SPP"),
                RegisterField::bit(7, "MPIE"),
                RegisterField::bit(6, "UBE"),
                RegisterField::bit(5, "SPIE"),
                RegisterField::bit(3, "MIE"),
                RegisterField::bit(1, "SIE"),
            ]),
            RVRegister::MSTATUSH => Some(vec![
                RegisterField::bit(4, "SBE"),
                RegisterField::bit(5, "MBE"),
            ]),
            RVRegister::MIP => Some(vec![
                RegisterField::bit(11, "MEIP"),
                RegisterField::bit(9, "SEIP"),
                RegisterField::bit(7, "MTIP"),
                RegisterField::bit(5, "STIP"),
                RegisterField::bit(3, "MSIP"),
                RegisterField::bit(1, "SSIP"),
            ]),
            RVRegister::MIE => Some(vec![
                RegisterField::bit(11, "MEIE"),
                RegisterField::bit(9, "SEIE"),
                RegisterField::bit(7, "MTIE"),
                RegisterField::bit(5, "STIE"),
                RegisterField::bit(3, "MSIE"),
                RegisterField::bit(1, "SSIE"),
            ]),
            RVRegister::DCSR => Some(vec![
                RegisterField::field(31, 28, "debugver"),
                RegisterField::bit(17, "ebreakvs"),
                RegisterField::bit(16, "ebreakvu"),
                RegisterField::bit(15, "ebreakm"),
                RegisterField::bit(13, "ebreaks"),
                RegisterField::bit(12, "ebreaku"),
                RegisterField::bit(11, "stepie"),
                RegisterField::bit(10, "spotcount"),
                RegisterField::bit(9, "stoptime"),
                RegisterField::field(8, 6, "cause"),
                RegisterField::bit(5, "v"),
                RegisterField::bit(4, "mprven"),
                RegisterField::bit(3, "nmip"),
                RegisterField::bit(2, "step"),
                RegisterField::field(1, 0, "priv"),
            ]),
            RVRegister::MTVEC => Some(vec![RegisterField::field(1, 0, "mode")]),
            _ => None,
        }
    }

    ///
    /// OpenOCD and GDB is a slightly modified version of https://github.com/riscv-non-isa/riscv-elf-psabi-doc/blob/master/riscv-dwarf.adoc
    /// The difference being that the CSR are offset by 65
    /// While the debug module itself expects https://raw.githubusercontent.com/riscv/riscv-debug-spec/master/riscv-debug-stable.pdf#table.3.3
    ///
    pub fn to_gdb_id(&self) -> u32 {
        // pc is at a special index
        if self == &RVRegister::PC {
            return 32;
        }

        let mut reg_id = RVRegister::to_u32(self).unwrap();
        if !self.is_special() {
            reg_id -= 0x1000;
        } else {
            // offsets all csr by 65
            // see https://github.com/openocd-org/openocd/blob/53556fcded056aa62ffdc6bf0c97bff87d891dab/src/target/riscv/gdb_regs.h#L80
            reg_id += 65
        }
        reg_id
    }
}

impl From<&RegId> for RVRegister {
    fn from(reg: &RegId) -> Self {
        match reg.0 as u32 {
            RISCV_REG_ZERO => RVRegister::ZERO,
            RISCV_REG_RA => RVRegister::RA,
            RISCV_REG_SP => RVRegister::SP,
            RISCV_REG_GP => RVRegister::TP,
            RISCV_REG_T0 => RVRegister::T0,
            RISCV_REG_T1 => RVRegister::T1,
            RISCV_REG_T2 => RVRegister::T2,
            RISCV_REG_T3 => RVRegister::T3,
            RISCV_REG_T4 => RVRegister::T4,
            RISCV_REG_T5 => RVRegister::T5,
            RISCV_REG_T6 => RVRegister::T6,
            RISCV_REG_TP => RVRegister::TP,
            RISCV_REG_S0 => RVRegister::S0,
            RISCV_REG_S1 => RVRegister::S1,
            RISCV_REG_S2 => RVRegister::S2,
            RISCV_REG_S3 => RVRegister::S3,
            RISCV_REG_S4 => RVRegister::S4,
            RISCV_REG_S5 => RVRegister::S5,
            RISCV_REG_S6 => RVRegister::S6,
            RISCV_REG_S7 => RVRegister::S7,
            RISCV_REG_S8 => RVRegister::S8,
            RISCV_REG_S9 => RVRegister::S9,
            RISCV_REG_S10 => RVRegister::S10,
            RISCV_REG_S11 => RVRegister::S11,
            _ => {
                panic!("unrecognized register {:x}", reg.0);
            }
        }
    }
}

pub fn get_all_registers() -> Vec<Register> {
    RVRegister::iter().map(Register::RiscV).collect()
}

impl std::fmt::Display for RVRegister {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.pad(&format!("RV: {:?}", self))
    }
}
