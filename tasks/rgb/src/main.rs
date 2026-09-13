#![no_std]
#![no_main]

mod snled27351;

use input_api::{Input, Led};
use keyboard::lighting::{Frame, Lighting};
use snled27351::Drivers;
use userlib::{hl::sleep_until, sys_get_timer, task_slot};

task_slot!(SPI, spi);
task_slot!(INPUT, input);

const FRAME_MS: u64 = 8;

#[unsafe(export_name = "main")]
fn main() -> ! {
    let input = Input::from(INPUT.get_task_id());
    let drivers = Drivers::init(spi_api::Spi::from(SPI.get_task_id()));
    let mut lighting = Lighting::default();
    // What the drivers are currently showing; `None` while they're shut down.
    let mut shown: Option<Frame> = None;

    loop {
        let now = sys_get_timer().now;
        let status = input.status();
        if status.awake != 0 {
            lighting.update(input.keys(), now);
            let frame = lighting.frame(now, status.leds.is_lit(Led::CapsLock));
            if shown != Some(frame) {
                drivers.show(&frame);
                if shown.is_none() {
                    drivers.enable(true);
                }
                shown = Some(frame);
            }
        } else if shown.is_some() {
            // Nobody's looking: go dark, and don't light up stale presses on wake.
            drivers.enable(false);
            lighting.clear();
            shown = None;
        }
        sleep_until(now + FRAME_MS);
    }
}
