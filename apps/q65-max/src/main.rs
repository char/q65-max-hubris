#![no_std]
#![no_main]

use stm32f4::stm32f401 as pac;

#[cortex_m_rt::pre_init]
unsafe fn pre_init() {
    // clear the bootloader reset flag
    let flag = board::REBOOT_FLAG as *mut u32;
    if unsafe { flag.read_volatile() } == board::ENTER_BOOTLOADER {
        unsafe { flag.write_volatile(0) };
        enter_bootloader();
    }

    // clean up interrupts left by the DFU ROM
    // look at my sending-the-system-backwards bro we're never getting holistic boot
    cortex_m::interrupt::disable();
    unsafe {
        let scb = &*cortex_m::peripheral::SCB::PTR;
        scb.vtor.write(0x0800_0000);
        let syst = &*cortex_m::peripheral::SYST::PTR;
        syst.csr.write(0);
        syst.cvr.write(0);
        let nvic = &*cortex_m::peripheral::NVIC::PTR;
        let banks = ((&*cortex_m::peripheral::ICB::PTR).ictr.read() & 0xf) + 1;
        for bank in 0..banks as usize {
            nvic.icer[bank].write(u32::MAX);
            nvic.icpr[bank].write(u32::MAX);
        }
        scb.icsr.write((1 << 25) | (1 << 27));
        cortex_m::register::basepri::write(0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
}

/// hop
fn enter_bootloader() -> ! {
    const SYSTEM_MEMORY: *const u32 = 0x1fff_0000 as *const u32;
    unsafe {
        let stack = SYSTEM_MEMORY.read_volatile();
        let entry = SYSTEM_MEMORY.add(1).read_volatile();
        cortex_m::asm::bootstrap(stack as *const u32, entry as *const u32)
    }
}

/// configure the clocks for the board - we match the clock tree of the qmk impl for ease
fn configure_clocks(p: &pac::Peripherals) {
    // dfu has its own PLL setup so clear that - we can't turn it off while it drives SYSCLK obvs
    p.RCC.cr.modify(|_, w| w.hsion().set_bit());
    while p.RCC.cr.read().hsirdy().bit_is_clear() {}
    p.RCC.cfgr.modify(|_, w| w.sw().hsi());
    while !p.RCC.cfgr.read().sws().is_hsi() {}
    p.RCC.cr.modify(|_, w| w.pllon().clear_bit());
    while p.RCC.cr.read().pllrdy().bit_is_set() {}

    // alright buddy get in the crystal
    p.RCC.apb1enr.modify(|_, w| w.pwren().set_bit());
    let _ = p.RCC.apb1enr.read(); // wait around for 1× read latency

    // raise voltage / latencies *before* we raise clock rate so nothing acts mercurial
    p.PWR.cr.modify(|_, w| unsafe { w.vos().bits(0b10) });
    p.FLASH.acr.modify(|_, w| {
        w.latency().ws1();
        w.prften().set_bit();
        w.icen().set_bit();
        w.dcen().set_bit()
    });

    p.RCC.cr.modify(|_, w| w.hseon().set_bit());
    while p.RCC.cr.read().hserdy().bit_is_clear() {}
    /* qmk_firmware/keyboards/keychron/q65_max/mcuconf.h:
    #define STM32_HSECLK 16000000
    #define STM32_PLLM_VALUE 8
    #define STM32_PLLN_VALUE 96
    #define STM32_PLLP_VALUE 4
    #define STM32_PLLQ_VALUE 4 */
    p.RCC.pllcfgr.write(|w| unsafe {
        w.pllm().bits(8);
        w.plln().bits(96);
        w.pllp().div4();
        w.pllq().bits(4);
        w.pllsrc().hse()
    });
    p.RCC.cfgr.modify(|_, w| {
        w.hpre().div1();
        w.ppre1().div2();
        w.ppre2().div1()
    });

    p.RCC.cr.modify(|_, w| w.pllon().set_bit());
    while p.RCC.cr.read().pllrdy().bit_is_clear() {}

    p.RCC.cfgr.modify(|_, w| w.sw().pll());
    while !p.RCC.cfgr.read().sws().is_pll() {}
}

/// configure clocks for otg and stuff
fn enable_peripherals(p: &pac::Peripherals) {
    p.RCC.ahb1enr.modify(|_, w| {
        w.gpioaen().set_bit();
        w.gpioben().set_bit();
        w.gpiocen().set_bit();
        w.gpioden().set_bit()
    });
    p.RCC.ahb2enr.modify(|_, w| w.otgfsen().set_bit());
    p.RCC.apb2enr.modify(|_, w| w.spi1en().set_bit());
    let _ = p.RCC.apb2enr.read();
    // reset the OTG core because the DFU bootloader uses it
    p.RCC.ahb2rstr.modify(|_, w| w.otgfsrst().set_bit());
    p.RCC.ahb2rstr.modify(|_, w| w.otgfsrst().clear_bit());
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let p = pac::Peripherals::take().unwrap();
    configure_clocks(&p);
    enable_peripherals(&p);

    unsafe { cortex_m::interrupt::enable() };
    unsafe { kern::startup::start_kernel(48_000) }
}
