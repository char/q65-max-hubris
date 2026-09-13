// vibed

//! The two SNLED27351 LED drivers, on SPI1 with a chip select each and a shared shutdown pin.
//!
//! Every SPI transaction is a page byte, a start register, then consecutive register values.
//! Pages: 0 = LED on/off control, 1 = PWM, 3 = function, 4 = current tune.

use keyboard::lighting::{DRIVERS, Frame};
use stm32f4::stm32f401 as pac;

const LED_CONTROL_PAGE: u8 = 0;
const PWM_PAGE: u8 = 1;
const FUNCTION_PAGE: u8 = 3;
const CURRENT_TUNE_PAGE: u8 = 4;
/// Command byte prefix: a write to the given page.
const WRITE: u8 = 0x20;

// Function page registers.
const CONFIGURATION: u8 = 0x00;
const PULL_DOWN_UP: u8 = 0x13;
const SCAN_PHASE: u8 = 0x14;
const SLEW_RATE_1: u8 = 0x15;
const SLEW_RATE_2: u8 = 0x16;
const SOFTWARE_SLEEP: u8 = 0x1a;

pub struct Drivers {
    spi: spi_api::Spi,
}

impl Drivers {
    /// Brings both drivers up configured the way QMK does for this board, with all LEDs enabled
    /// but everything at zero and the drivers in software shutdown until `enable`.
    pub fn init(spi: spi_api::Spi) -> Self {
        let gpiob = unsafe { &*pac::GPIOB::ptr() };
        gpiob.bsrr.write(|w| w.bs7().set_bit());
        gpiob.moder.modify(|_, w| w.moder7().output());
        let drivers = Self { spi };
        // Let the LED drivers wake from hardware shutdown before configuring them.
        userlib::hl::sleep_for(100);
        for driver in 0..DRIVERS {
            drivers.write(driver, FUNCTION_PAGE, CONFIGURATION, &[0]);
            drivers.write(driver, FUNCTION_PAGE, PULL_DOWN_UP, &[0xaa]);
            drivers.write(driver, FUNCTION_PAGE, SCAN_PHASE, &[0]); // all nine channels
            drivers.write(driver, FUNCTION_PAGE, SLEW_RATE_1, &[0x04]); // PWM delay phase
            drivers.write(driver, FUNCTION_PAGE, SLEW_RATE_2, &[0xc0]); // slew rate control
            drivers.write(driver, FUNCTION_PAGE, SOFTWARE_SLEEP, &[0]);
            drivers.write(driver, PWM_PAGE, 0, &[0; 192]);
            drivers.write(driver, CURRENT_TUNE_PAGE, 0, &[0x40; 12]);
            drivers.write(driver, LED_CONTROL_PAGE, 0, &[0xff; 24]);
        }
        drivers
    }

    /// Software shutdown: off is dark and low-power but keeps the configuration.
    pub fn enable(&self, on: bool) {
        for driver in 0..DRIVERS {
            self.write(driver, FUNCTION_PAGE, CONFIGURATION, &[u8::from(on)]);
        }
    }

    pub fn show(&self, frame: &Frame) {
        for (driver, pwm) in frame.iter().enumerate() {
            self.write(driver, PWM_PAGE, 0, pwm);
        }
    }

    fn write(&self, driver: usize, page: u8, register: u8, data: &[u8]) {
        // Bound priority inversion: each LED transaction clocks at most 34 bytes.
        for (index, chunk) in data.chunks(32).enumerate() {
            let mut bytes = [0; 34];
            bytes[0] = WRITE | page;
            bytes[1] = register + (index * 32) as u8;
            bytes[2..2 + chunk.len()].copy_from_slice(chunk);
            let deadline = userlib::sys_get_timer().now + 200;
            while !self
                .spi
                .write(driver as u8, &bytes[..2 + chunk.len()])
                .unwrap_or(false)
            {
                assert!(
                    userlib::sys_get_timer().now < deadline,
                    "SPI1 transfer failed"
                );
                // PA4 wake pulses temporarily prevent clocking either LED driver.
                userlib::hl::sleep_for(1);
            }
        }
    }
}
