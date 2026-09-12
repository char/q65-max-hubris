#![no_std]
#![no_main]

use userlib::{TaskId, kipc, sys_recv_open, sys_reply};
use util::Reg;

const FAULT: u32 = 1;

#[unsafe(export_name = "main")]
fn main() -> ! {
    // never restart anything because we will NEVER crash.
    loop {
        let message = sys_recv_open(&mut [], FAULT);
        if message.sender == TaskId::KERNEL {
            continue;
        }
        if message.operation == u32::from(jefe_api::ENTER_BOOTLOADER) {
            Reg(board::REBOOT_FLAG).write(board::ENTER_BOOTLOADER);
            kipc::system_restart();
        }
        sys_reply(message.sender, 1, &[]);
    }
}
