// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use crate::regs::{Register, RegisterField};
use capstone::arch::arm::ArmReg::*;
use capstone::RegId;
use num_traits::FromPrimitive;
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
/// The definition of an ARM register, as encoded in the Debug Core Register
/// Selector Register (DCRSR); see (e.g.) C1.6.3 in the ARM v7-M Architecture
/// Reference Manual.
///
pub enum ARMRegister {
    R0 = 0,
    R1,
    R2,
    R3,
    R4,
    R5,
    R6,
    R7,
    R8,
    R9,
    R10,
    R11,
    R12,
    SP = 0b000_1101,
    LR = 0b000_1110,
    PC = 0b000_1111,
    PSR = 0b001_0000,
    MSP = 0b001_0001,
    PSP = 0b001_0010,
    SPR = 0b001_0100,
    FPSCR = 0b010_0001,
    S0 = 0b100_0000,
    S1,
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
    S12,
    S13,
    S14,
    S15,
    S16,
    S17,
    S18,
    S19,
    S20,
    S21,
    S22,
    S23,
    S24,
    S25,
    S26,
    S27,
    S28,
    S29,
    S30,
    S31,
}

impl ARMRegister {
    pub fn to_gdb_id(&self) -> u32 {
        ARMRegister::to_u32(self).unwrap()
    }

    pub fn is_general_purpose(&self) -> bool {
        matches!(
            self,
            ARMRegister::R0
                | ARMRegister::R1
                | ARMRegister::R2
                | ARMRegister::R3
                | ARMRegister::R4
                | ARMRegister::R5
                | ARMRegister::R6
                | ARMRegister::R7
                | ARMRegister::R8
                | ARMRegister::R9
                | ARMRegister::R10
                | ARMRegister::R11
                | ARMRegister::R12
                | ARMRegister::SP
                | ARMRegister::PC
                | ARMRegister::LR
        )
    }

    pub fn is_special(&self) -> bool {
        matches!(
            self,
            ARMRegister::PSR
                | ARMRegister::MSP
                | ARMRegister::PSP
                | ARMRegister::SPR
                | ARMRegister::FPSCR
        )
    }

    pub fn is_floating_point(&self) -> bool {
        self.to_u16() >= ARMRegister::S0.to_u16()
    }

    pub fn fields(&self) -> Option<Vec<RegisterField>> {
        match self {
            ARMRegister::PSR => Some(vec![
                RegisterField::bit(31, "N"),
                RegisterField::bit(30, "Z"),
                RegisterField::bit(29, "C"),
                RegisterField::bit(28, "V"),
                RegisterField::bit(27, "Q"),
                RegisterField::field(26, 25, "IC/IT"),
                RegisterField::bit(24, "T"),
                RegisterField::field(19, 16, "GE"),
                RegisterField::field(15, 10, "IC/IT"),
                RegisterField::field(8, 0, "Exception"),
            ]),
            ARMRegister::SPR => Some(vec![
                RegisterField::bit(26, "CONTROL.FPCA"),
                RegisterField::bit(25, "CONTROL.SPSEL"),
                RegisterField::bit(24, "CONTROL.nPRIV"),
                RegisterField::bit(16, "FAULTMASK"),
                RegisterField::field(15, 8, "BASEPRI"),
                RegisterField::bit(0, "PRIMASK"),
            ]),
            _ => None,
        }
    }
}

impl From<&RegId> for ARMRegister {
    fn from(reg: &RegId) -> Self {
        match reg.0 as u32 {
            ARM_REG_R0 => ARMRegister::R0,
            ARM_REG_R1 => ARMRegister::R1,
            ARM_REG_R2 => ARMRegister::R2,
            ARM_REG_R3 => ARMRegister::R3,
            ARM_REG_R4 => ARMRegister::R4,
            ARM_REG_R5 => ARMRegister::R5,
            ARM_REG_R6 => ARMRegister::R6,
            ARM_REG_R7 => ARMRegister::R7,
            ARM_REG_R8 => ARMRegister::R8,
            ARM_REG_R9 => ARMRegister::R9,
            ARM_REG_R10 => ARMRegister::R10,
            ARM_REG_R11 => ARMRegister::R11,
            ARM_REG_R12 => ARMRegister::R12,
            ARM_REG_SP => ARMRegister::SP,
            ARM_REG_PC => ARMRegister::PC,
            ARM_REG_LR => ARMRegister::LR,
            _ => {
                panic!("unrecognized register {:x}", reg.0);
            }
        }
    }
}

pub fn register_from_id(id: u32) -> Option<Register> {
    ARMRegister::from_u32(id).map(Register::Arm)
}

pub fn get_all_registers() -> Vec<Register> {
    ARMRegister::iter().map(Register::Arm).collect()
}

impl std::fmt::Display for ARMRegister {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.pad(&format!("ARM_REG: {:?}", self))
    }
}
