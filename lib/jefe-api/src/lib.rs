#![no_std]

use userlib::{TaskId, sys_send};

/// Reset into the ROM DFU bootloader
pub const ENTER_BOOTLOADER: u16 = 1;

pub fn enter_bootloader(jefe: TaskId) -> ! {
    sys_send(jefe, ENTER_BOOTLOADER, &[], &mut [], &[]);
    unreachable!("the supervisor should have reset the system")
}
