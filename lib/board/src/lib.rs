#![no_std]

/// The last word of SRAM, kept out of the memory map in `memory.toml` so it survives a reset
/// untouched. The supervisor writes `ENTER_BOOTLOADER` here before resetting; the kernel's
/// `pre_init` reads and clears it.
pub const REBOOT_FLAG: usize = 0x2000_fffc;
pub const ENTER_BOOTLOADER: u32 = 0xb007_10ad;
