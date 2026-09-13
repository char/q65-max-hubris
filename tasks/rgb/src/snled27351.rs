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
    spi: &'static pac::spi1::RegisterBlock,
    gpiob: &'static pac::gpiob::RegisterBlock,
}

impl Drivers {
    /// Brings both drivers up configured the way QMK does for this board, with all LEDs enabled
    /// but everything at zero and the drivers in software shutdown until `enable`.
    #[expect(clippy::similar_names, reason = "those are the ports' names")]
    pub fn init() -> Self {
        let gpioa = unsafe { &*pac::GPIOA::ptr() };
        let gpiob = unsafe { &*pac::GPIOB::ptr() };
        let spi = unsafe { &*pac::SPI1::ptr() };

        // PA5 SCK and PA7 MOSI are SPI1 on alternate function 5.
        gpioa
            .otyper
            .modify(|_, w| w.ot5().push_pull().ot7().push_pull());
        gpioa
            .pupdr
            .modify(|_, w| w.pupdr5().floating().pupdr7().floating());
        // The reset-default 2 MHz slew rate is too slow for our 3 MHz SPI clock.
        gpioa
            .ospeedr
            .modify(|_, w| w.ospeedr5().medium_speed().ospeedr7().medium_speed());
        gpioa.afrl.modify(|_, w| w.afrl5().af5().afrl7().af5());
        gpioa
            .moder
            .modify(|_, w| w.moder5().alternate().moder7().alternate());
        // Chip selects idle high; PB7 high takes the drivers out of hardware shutdown.
        gpiob
            .bsrr
            .write(|w| w.bs7().set_bit().bs8().set_bit().bs9().set_bit());
        gpiob.moder.modify(|_, w| {
            w.moder7().output();
            w.moder8().output();
            w.moder9().output()
        });

        // Master, mode 0, 48 MHz / 16 = 3 MHz, transmit only, chip selects by hand.
        spi.cr1.write(|w| {
            w.mstr().master();
            w.br().div16();
            w.ssm().set_bit();
            w.ssi().set_bit();
            w.bidimode().set_bit();
            w.bidioe().set_bit();
            w.spe().set_bit()
        });

        let drivers = Self { spi, gpiob };
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
        self.gpiob.bsrr.write(|w| match driver {
            0 => w.br9().set_bit(),
            _ => w.br8().set_bit(),
        });
        for byte in [WRITE | page, register]
            .into_iter()
            .chain(data.iter().copied())
        {
            self.wait(|sr| sr.txe().bit_is_set());
            self.spi.dr.write(|w| w.dr().bits(u16::from(byte)));
        }
        self.wait(|sr| sr.txe().bit_is_set());
        self.wait(|sr| sr.bsy().bit_is_clear());
        self.gpiob.bsrr.write(|w| match driver {
            0 => w.bs9().set_bit(),
            _ => w.bs8().set_bit(),
        });
    }

    fn wait(&self, ready: impl Fn(&pac::spi1::sr::R) -> bool) {
        for _ in 0..10_000 {
            if ready(&self.spi.sr.read()) {
                return;
            }
        }
        panic!("SPI1 stuck");
    }
}
