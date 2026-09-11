#![no_std]
#![no_main]

const FAULT: u32 = 1;

#[unsafe(export_name = "main")]
fn main() -> ! {
    // never restart anything because we will NEVER crash.
    loop {
        userlib::sys_recv_notification(FAULT);
    }
}
