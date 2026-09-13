use stm32f4::stm32f401 as pac;

pub struct Controller {
    spi: &'static pac::spi1::RegisterBlock,
    gpioa: &'static pac::gpioa::RegisterBlock,
    gpiob: &'static pac::gpiob::RegisterBlock,
}

impl Controller {
    pub fn init() -> Self {
        let gpioa = unsafe { &*pac::GPIOA::ptr() };
        let gpiob = unsafe { &*pac::GPIOB::ptr() };
        let spi = unsafe { &*pac::SPI1::ptr() };
        gpioa.bsrr.write(|w| w.bs4().set_bit());
        gpiob.bsrr.write(|w| w.bs8().set_bit().bs9().set_bit());
        gpioa
            .otyper
            .modify(|_, w| w.ot4().push_pull().ot5().push_pull().ot7().push_pull());
        gpioa.pupdr.modify(|_, w| {
            w.pupdr4()
                .floating()
                .pupdr5()
                .floating()
                .pupdr6()
                .floating()
                .pupdr7()
                .floating()
        });
        // The reset-default slew rate is too slow for the 3 MHz clock.
        gpioa
            .ospeedr
            .modify(|_, w| w.ospeedr5().medium_speed().ospeedr7().medium_speed());
        gpioa
            .afrl
            .modify(|_, w| w.afrl5().af5().afrl6().af5().afrl7().af5());
        gpioa.moder.modify(|_, w| {
            w.moder4()
                .output()
                .moder5()
                .alternate()
                .moder6()
                .alternate()
                .moder7()
                .alternate()
        });
        gpiob
            .moder
            .modify(|_, w| w.moder8().output().moder9().output());
        let controller = Self { spi, gpioa, gpiob };
        controller.configure();
        controller
    }

    fn configure(&self) {
        self.spi.cr1.write(|w| {
            w.mstr()
                .master()
                .br()
                .div16()
                .ssm()
                .set_bit()
                .ssi()
                .set_bit()
        });
        self.spi.cr2.reset();
        // Clear RXNE/OVR left by an aborted transfer (DR then SR).
        let _ = self.spi.dr.read();
        let _ = self.spi.sr.read();
        self.spi.cr1.modify(|_, w| w.spe().set_bit());
    }

    pub fn select(&self, device: u8, active: bool) {
        let pin = match device {
            0 => 9,
            1 => 8,
            _ => 4,
        };
        let bits = 1 << (pin + if active { 16 } else { 0 });
        if device == spi_api::RADIO {
            self.gpioa.bsrr.write(|w| unsafe { w.bits(bits) });
        } else {
            self.gpiob.bsrr.write(|w| unsafe { w.bits(bits) });
        }
    }

    pub fn transfer(&self, device: u8, bytes: &mut [u8]) -> bool {
        if device > spi_api::RADIO || bytes.is_empty() {
            return false;
        }
        self.select(device, true);
        let mut remaining = 100_000;
        let mut ok = true;
        for byte in bytes {
            if !self.wait(1 << 1, true, &mut remaining) {
                ok = false;
                break;
            }
            self.spi.dr.write(|w| w.dr().bits(u16::from(*byte)));
            if !self.wait(1, true, &mut remaining) {
                ok = false;
                break;
            }
            *byte = self.spi.dr.read().dr().bits() as u8;
        }
        ok &= self.wait(1 << 7, false, &mut remaining);
        if !ok {
            self.spi.cr1.modify(|_, w| w.spe().clear_bit());
        }
        self.select(device, false);
        if !ok {
            self.configure();
        }
        ok
    }

    fn wait(&self, mask: u32, set: bool, remaining: &mut u32) -> bool {
        while *remaining != 0 {
            *remaining -= 1;
            let sr = self.spi.sr.read().bits();
            if sr & ((1 << 5) | (1 << 6)) != 0 {
                return false;
            }
            if (sr & mask != 0) == set {
                return true;
            }
        }
        false
    }
}
