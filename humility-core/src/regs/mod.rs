// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

use num_traits::cast::ToPrimitive;

pub mod rv;
use rv::RVRegister;
pub mod arm;
use arm::ARMRegister;

#[derive(Copy, Clone, Debug)]
pub struct RegisterField {
    pub highbit: u16,
    pub lowbit: u16,
    pub name: &'static str,
}

impl RegisterField {
    pub fn field(highbit: u16, lowbit: u16, name: &'static str) -> Self {
        Self { highbit, lowbit, name }
    }
    pub fn bit(bit: u16, name: &'static str) -> Self {
        Self { highbit: bit, lowbit: bit, name }
    }
}
#[allow(non_camel_case_types)]
#[derive(Copy, Clone, Debug, Hash, PartialEq, Eq, Ord, PartialOrd)]
pub enum Register {
    Arm(ARMRegister),
    RiscV(RVRegister),
}

impl Register {
    pub fn is_pc(&self) -> bool {
        match self {
            Register::Arm(reg) => *reg == ARMRegister::PC,
            Register::RiscV(reg) => *reg == RVRegister::PC,
        }
    }
    pub fn is_general_purpose(&self) -> bool {
        match self {
            Register::Arm(reg) => reg.is_general_purpose(),
            Register::RiscV(reg) => reg.is_general_purpose(),
        }
    }
    pub fn is_special(&self) -> bool {
        match self {
            Register::Arm(reg) => reg.is_special(),
            Register::RiscV(reg) => reg.is_special(),
        }
    }
    pub fn is_floating_point(&self) -> bool {
        match self {
            Register::Arm(reg) => reg.is_floating_point(),
            Register::RiscV(reg) => reg.is_floating_point(),
        }
    }
    pub fn fields(&self) -> Option<Vec<RegisterField>> {
        match self {
            Register::Arm(reg) => reg.fields(),
            Register::RiscV(reg) => reg.fields(),
        }
    }
    pub fn to_gdb_id(&self) -> u32 {
        match self {
            Register::Arm(reg) => reg.to_gdb_id(),
            Register::RiscV(reg) => reg.to_gdb_id(),
        }
    }
}

impl ToPrimitive for Register {
    fn to_u64(&self) -> Option<u64> {
        match self {
            Register::Arm(reg) => ARMRegister::to_u64(reg),
            Register::RiscV(reg) => RVRegister::to_u64(reg),
        }
    }
    fn to_i64(&self) -> Option<i64> {
        match self {
            Register::Arm(reg) => ARMRegister::to_i64(reg),
            Register::RiscV(reg) => RVRegister::to_i64(reg),
        }
    }
    fn to_u32(&self) -> Option<u32> {
        match self {
            Register::Arm(reg) => ARMRegister::to_u32(reg),
            Register::RiscV(reg) => RVRegister::to_u32(reg),
        }
    }
    fn to_u16(&self) -> Option<u16> {
        match self {
            Register::Arm(reg) => ARMRegister::to_u16(reg),
            Register::RiscV(reg) => RVRegister::to_u16(reg),
        }
    }
}

impl std::fmt::Display for Register {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Register::Arm(reg) => formatter.pad(&format!("{:?}", reg)),
            Register::RiscV(reg) => formatter.pad(&format!("{:?}", reg)),
        }
    }
}
