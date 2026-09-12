#![no_std]

pub const REBOOT_FLAG: usize = 0x2000_fffc;
pub const ENTER_BOOTLOADER: u32 = 0xb007_10ad;
