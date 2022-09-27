use std::ops::{Add, Sub};

#[derive(Debug, Copy, Clone, PartialEq)]
pub struct UhSize {
    pub data: u64,
    bits: usize,
}

impl UhSize {
    pub fn get_bytes_per_word(&self) -> usize {
        self.bits / 8 
    }
}

impl Sub for UhSize {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        assert_eq!(self.bits, other.bits);
        Self{ 
            data: self.data - other.data,
            bits: self.bits
        }
    }
}

impl Add for UhSize {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        assert_eq!(self.bits, other.bits);
        Self{ 
            data: self.data + other.data,
            bits: self.bits
        }
    }
}
