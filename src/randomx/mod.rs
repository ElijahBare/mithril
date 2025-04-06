pub mod common;
pub mod hash;
pub mod m128;
pub mod memory;
pub mod program;
pub mod superscalar;
pub mod vm;

use self::vm::Vm;

/// Trait defining the interface for a RandomX virtual machine
pub trait RandomXVM {
    /// Calculates a RandomX hash for the given input bytes
    fn calculate_hash(&mut self, input: &[u8]) -> blake2b_simd::Hash;
}

// Implement the RandomXVM trait for the Vm struct
impl RandomXVM for Vm {
    fn calculate_hash(&mut self, input: &[u8]) -> blake2b_simd::Hash {
        self.calculate_hash(input)
    }
}
