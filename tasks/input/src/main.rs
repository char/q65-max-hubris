#![no_std]
#![no_main]

mod matrix;

use keyboard::{Debouncer, Encoder, Keymap};
use usb_api::{Reports, Usb};
use userlib::{hl::sleep_until, sys_get_timer, task_slot};

task_slot!(USB, usb);

#[unsafe(export_name = "main")]
fn main() -> ! {
    // The usb task muxes its own GPIOA pins during its start-up, which at priority 1 has finished
    // before we first run; from here on nobody but us touches GPIO configuration.
    matrix::init();
    let mut debouncer = Debouncer::default();
    let mut keymap = Keymap::default();
    let mut encoder = Encoder::new(matrix::encoder_state());
    let usb = Usb::from(USB.get_task_id());
    let send = |reports: Reports| usb.set_reports(reports);

    loop {
        let start = sys_get_timer().now;
        let keys = debouncer.update(matrix::scan(), start);
        keymap.update(keys, start, send);
        if let Some(rotation) = encoder.update(matrix::encoder_state()) {
            keymap.turn(rotation, send);
        }
        // One scan per millisecond. If we ever fall behind, skip the missed ticks rather than
        // scanning in a burst to catch up.
        sleep_until((start + 1).max(sys_get_timer().now));
    }
}
