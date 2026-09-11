#![no_std]
#![no_main]

extern crate userlib;

#[unsafe(export_name = "main")]
fn main() -> ! {
    loop {
        cortex_m::asm::wfi();
    }
}
