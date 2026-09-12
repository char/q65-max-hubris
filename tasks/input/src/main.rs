#![no_std]
#![no_main]

mod matrix;

use idol_runtime::{NotificationHandler, RequestError};
use keyboard::{Command, Debouncer, Encoder, Keymap, Matrix};
use usb_api::{Reports, Usb};
use userlib::{RecvMessage, sys_get_timer, sys_set_timer, task_slot};

task_slot!(JEFE, jefe);
task_slot!(USB, usb);

#[unsafe(export_name = "main")]
fn main() -> ! {
    // The usb task muxes its own GPIOA pins during its start-up, which at priority 1 has finished
    // before we first run; from here on nobody but us touches GPIO configuration.
    matrix::init();
    let mut input = Input {
        debouncer: Debouncer::default(),
        keymap: Keymap::default(),
        encoder: Encoder::new(matrix::encoder_state()),
        usb: Usb::from(USB.get_task_id()),
        keys: Matrix::default(),
    };
    let mut incoming = [0; idl::INCOMING_SIZE];
    sys_set_timer(Some(sys_get_timer().now), notifications::TIMER_MASK);
    loop {
        idol_runtime::dispatch(&mut incoming, &mut input);
    }
}

struct Input {
    debouncer: Debouncer,
    keymap: Keymap,
    encoder: Encoder,
    usb: Usb,
    keys: Matrix,
}

impl NotificationHandler for Input {
    fn current_notification_mask(&self) -> u32 {
        notifications::TIMER_MASK
    }

    fn handle_notification(&mut self, _: userlib::NotificationBits) {
        let start = sys_get_timer().now;
        self.keys = self.debouncer.update(matrix::scan(), start);
        let send = |reports: Reports| self.usb.set_reports(reports);
        if let Some(Command::EnterBootloader) = self.keymap.update(self.keys, start, send) {
            jefe_api::enter_bootloader(JEFE.get_task_id());
        }
        if let Some(rotation) = self.encoder.update(matrix::encoder_state()) {
            self.keymap.turn(rotation, send);
        }
        // One scan per millisecond. If we ever fall behind, skip the missed ticks rather than
        // scanning in a burst to catch up.
        // The deadline must be in the future or an overrun can starve the RGB task.
        let next = sys_get_timer().now + 1;
        sys_set_timer(Some(next), notifications::TIMER_MASK);
    }
}

impl idl::InOrderInputImpl for Input {
    fn keys(&mut self, _: &RecvMessage) -> Result<Matrix, RequestError<core::convert::Infallible>> {
        Ok(self.keys)
    }
}

mod idl {
    use keyboard::Matrix;

    include!(concat!(env!("OUT_DIR"), "/server_stub.rs"));
}

include!(concat!(env!("OUT_DIR"), "/notifications.rs"));
