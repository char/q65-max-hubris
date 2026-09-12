// vibed

//! Device-mode driver for the STM32F401's OTG FS core (a Synopsys DWC2).
//!
//! Endpoint 0 carries control transfers; the report endpoints are interrupt IN. The core is
//! IRQ-driven: after each interrupt, `next_event` is drained and the returned events are handed to
//! whoever implements the device. Everything the device needs to do to the hardware in response is
//! one of the commands at the bottom.

use stm32f4::stm32f401 as pac;
use userlib::hl::sleep_for;

pub const MAX_PACKET: usize = 64;

/// The largest single IN transfer; the endpoint 0 TX FIFO is sized to hold it in one go, so the
/// core splits it into packets and we never have to.
pub const MAX_TRANSFER: usize = CONTROL_TX_WORDS as usize * 4;

// FIFO layout in 32-bit words; the core has 320.
const RX_WORDS: u16 = 128;
const CONTROL_TX_WORDS: u16 = 32;
const REPORT_TX_WORDS: u16 = 16;

pub enum Event {
    Reset,
    Setup([u8; 8]),
    /// Data, or a zero-length status packet, on endpoint 0.
    Out(Packet),
    /// The IN transfer on this endpoint was taken by the host.
    InComplete(u8),
    Suspend,
    Resume,
}

pub struct Packet {
    bytes: [u8; MAX_PACKET],
    len: usize,
}

impl core::ops::Deref for Packet {
    type Target = [u8];
    fn deref(&self) -> &[u8] {
        &self.bytes[..self.len]
    }
}

pub struct Otg {
    global: &'static pac::otg_fs_global::RegisterBlock,
    device: &'static pac::otg_fs_device::RegisterBlock,
    /// SETUP and OUT packets come out of the RX FIFO before the endpoint reports the transfer
    /// done, and the core may pop several (a host retrying SETUP) before that; only the last one
    /// standing when the completion fires is real.
    setup: Option<[u8; 8]>,
    out: Option<Packet>,
}

impl Otg {
    pub fn new() -> Self {
        let global = unsafe { &*pac::OTG_FS_GLOBAL::ptr() };
        let device = unsafe { &*pac::OTG_FS_DEVICE::ptr() };
        let pwrclk = unsafe { &*pac::OTG_FS_PWRCLK::ptr() };

        global.gahbcfg.write(|w| w.gint().clear_bit());
        global.gusbcfg.modify(|_, w| w.physel().set_bit());
        while global.grstctl.read().ahbidl().bit_is_clear() {}
        global.grstctl.write(|w| w.csrst().set_bit());
        while global.grstctl.read().csrst().bit_is_set() {}
        // PA9 isn't wired to VBUS on this board, so disable sensing. NOVBUSSENS is bit 21; the PAC
        // doesn't know about it.
        global
            .gccfg
            .write(|w| unsafe { w.bits(1 << 21) }.pwrdwn().set_bit());
        // Force device mode; TRDT = 6 is the turnaround time for a 48 MHz AHB.
        global.gusbcfg.modify(|_, w| {
            w.fdmod().set_bit();
            w.fhmod().clear_bit();
            unsafe { w.trdt().bits(6) }
        });
        sleep_for(50); // the mode change takes up to 25 ms to stick

        pwrclk.pcgcctl.write(|w| unsafe { w.bits(0) });
        device.dctl.modify(|_, w| w.sdis().set_bit());
        device.dcfg.write(|w| unsafe { w.dspd().bits(0b11) }); // full speed
        device.diepmsk.write(|w| w.xfrcm().set_bit());
        device
            .doepmsk
            .write(|w| w.stupm().set_bit().xfrcm().set_bit());
        global.gintsts.write(|w| unsafe { w.bits(u32::MAX) });
        global.gintmsk.write(|w| {
            w.usbrst().set_bit();
            w.enumdnem().set_bit();
            w.rxflvlm().set_bit();
            w.iepint().set_bit();
            w.oepint().set_bit();
            w.usbsuspm().set_bit();
            w.wuim().set_bit()
        });
        global.gahbcfg.write(|w| w.gint().set_bit());
        device.dctl.modify(|_, w| w.sdis().clear_bit());

        Self {
            global,
            device,
            setup: None,
            out: None,
        }
    }

    pub fn next_event(&mut self) -> Option<Event> {
        loop {
            let interrupts = self.global.gintsts.read();
            if interrupts.usbrst().bit_is_set() {
                self.global.gintsts.write(|w| w.usbrst().set_bit());
                self.reset();
                return Some(Event::Reset);
            }
            if interrupts.enumdne().bit_is_set() {
                self.global.gintsts.write(|w| w.enumdne().set_bit());
                self.enumeration_done();
                continue;
            }
            if interrupts.usbsusp().bit_is_set() {
                self.global.gintsts.write(|w| w.usbsusp().set_bit());
                return Some(Event::Suspend);
            }
            if interrupts.wkupint().bit_is_set() {
                self.global.gintsts.write(|w| w.wkupint().set_bit());
                return Some(Event::Resume);
            }
            if interrupts.rxflvl().bit_is_set() {
                self.pop_rx_fifo();
                continue;
            }

            let in_endpoints = self.device.daint.read().iepint().bits();
            for ep in 0..=2 {
                let endpoint = InEndpoint(ep);
                if in_endpoints & (1 << ep) != 0 && endpoint.int().read() & XFRC != 0 {
                    endpoint.int().write(XFRC);
                    return Some(Event::InComplete(ep));
                }
            }

            // Completions are ordered so the device sees a transfer's status stage before the
            // SETUP that follows it, when both are pending.
            let out_interrupts = self.device.doepint0.read();
            if out_interrupts.xfrc().bit_is_set() {
                self.device.doepint0.write(|w| w.xfrc().set_bit());
                if let Some(packet) = self.out.take() {
                    self.arm_control_out();
                    return Some(Event::Out(packet));
                }
            }
            if out_interrupts.stup().bit_is_set() {
                self.device.doepint0.write(|w| w.stup().set_bit());
                self.arm_control_out();
                if let Some(setup) = self.setup.take() {
                    return Some(Event::Setup(setup));
                }
            }
            return None;
        }
    }

    fn reset(&mut self) {
        self.setup = None;
        self.out = None;
        for ep in 1..=2 {
            self.close_in(ep);
        }
        self.device.dcfg.modify(|_, w| unsafe { w.dad().bits(0) });
        self.device
            .daintmsk
            .write(|w| unsafe { w.iepm().bits(1).oepm().bits(1) });

        self.global
            .grxfsiz
            .write(|w| unsafe { w.rxfd().bits(RX_WORDS) });
        self.global
            .dieptxf0()
            .write(|w| unsafe { w.tx0fsa().bits(RX_WORDS).tx0fd().bits(CONTROL_TX_WORDS) });
        self.global.dieptxf1.write(|w| unsafe {
            w.ineptxsa()
                .bits(RX_WORDS + CONTROL_TX_WORDS)
                .ineptxfd()
                .bits(REPORT_TX_WORDS)
        });
        self.global.dieptxf2.write(|w| unsafe {
            w.ineptxsa()
                .bits(RX_WORDS + CONTROL_TX_WORDS + REPORT_TX_WORDS)
                .ineptxfd()
                .bits(REPORT_TX_WORDS)
        });
        self.flush_tx(ALL_TX_FIFOS);
        self.global.grstctl.write(|w| w.rxfflsh().set_bit());
        while self.global.grstctl.read().rxfflsh().bit_is_set() {}

        InEndpoint(0).ctl().write(SNAK);
        self.device.doepctl0.write(|w| w.snak().set_bit());
    }

    fn enumeration_done(&self) {
        // Only full speed is possible here, so endpoint 0's max packet is 64 (MPSIZ = 0).
        self.device
            .diepctl0
            .write(|w| unsafe { w.mpsiz().bits(0) }.snak().set_bit());
        self.device.dctl.modify(|_, w| w.cginak().set_bit());
        self.arm_control_out();
    }

    /// Endpoint 0 OUT stays ready for either a SETUP or one data packet at all times.
    fn arm_control_out(&self) {
        self.device.doeptsiz0.write(|w| unsafe {
            w.stupcnt()
                .bits(3)
                .pktcnt()
                .set_bit()
                .xfrsiz()
                .bits(MAX_PACKET as u8)
        });
        self.device
            .doepctl0
            .modify(|_, w| w.cnak().set_bit().epena().set_bit());
    }

    fn pop_rx_fifo(&mut self) {
        const OUT_DATA: u8 = 0b0010;
        const SETUP_DATA: u8 = 0b0110;
        let status = self.global.grxstsp_device().read();
        let mut packet = Packet {
            bytes: [0; MAX_PACKET],
            len: usize::from(status.bcnt().bits()),
        };
        // Other statuses (global NAK, transfer complete markers) carry no data.
        if !matches!(status.pktsts().bits(), OUT_DATA | SETUP_DATA) {
            return;
        }
        for word in 0..packet.len.div_ceil(4) {
            let bytes = InEndpoint(0).fifo().read().to_le_bytes();
            let offset = word * 4;
            if offset < MAX_PACKET {
                let end = (offset + 4).min(MAX_PACKET);
                packet.bytes[offset..end].copy_from_slice(&bytes[..end - offset]);
            }
        }
        if status.epnum().bits() != 0 {
            return;
        }
        if status.pktsts().bits() == SETUP_DATA {
            if packet.len == 8 {
                self.setup = Some(packet.bytes[..8].try_into().unwrap());
            }
        } else {
            self.out = Some(packet);
        }
    }

    pub fn set_address(&self, address: u8) {
        // Unlike the software-visible state change, the core wants this before the status IN.
        self.device
            .dcfg
            .modify(|_, w| unsafe { w.dad().bits(address) });
    }

    /// Queue an IN transfer. Reports are one packet by construction; control responses of up to
    /// `MAX_TRANSFER` bytes are split into packets by the core.
    #[expect(
        clippy::unused_self,
        reason = "a method so callers must hold an initialised core"
    )]
    pub fn send(&self, ep: u8, bytes: &[u8]) {
        assert!(bytes.len() <= MAX_TRANSFER);
        let endpoint = InEndpoint(ep);
        let packets = bytes.len().div_ceil(MAX_PACKET).max(1) as u32;
        endpoint
            .tsiz()
            .write(packets << PKTCNT_SHIFT | bytes.len() as u32);
        endpoint.ctl().set(CNAK | EPENA);
        for chunk in bytes.chunks(4) {
            let mut word = [0; 4];
            word[..chunk.len()].copy_from_slice(chunk);
            endpoint.fifo().write(u32::from_le_bytes(word));
        }
    }

    /// Reject the current control transfer. The core un-stalls endpoint 0 itself on the next SETUP.
    pub fn stall_control(&self) {
        self.device.diepctl0.modify(|_, w| w.stall().set_bit());
        self.device.doepctl0.modify(|_, w| w.stall().set_bit());
    }

    /// Activate an interrupt IN endpoint with its data toggle reset.
    pub fn open_in(&self, ep: u8, max_packet: u8) {
        let endpoint = InEndpoint(ep);
        self.close_in(ep);
        endpoint.ctl().write(
            u32::from(max_packet)
                | USBAEP
                | EPTYP_INTERRUPT
                | u32::from(ep) << TXFNUM_SHIFT
                | SD0PID
                | SNAK,
        );
        self.device
            .daintmsk
            .modify(|r, w| unsafe { w.iepm().bits(r.iepm().bits() | 1 << ep) });
    }

    pub fn close_in(&self, ep: u8) {
        let endpoint = InEndpoint(ep);
        if endpoint.ctl().read() & EPENA != 0 {
            // The disable sequence the core insists on: NAK, wait for it to take, then disable.
            endpoint.ctl().set(SNAK);
            wait_for(endpoint.int(), INEPNE);
            endpoint.ctl().set(EPDIS | SNAK);
            wait_for(endpoint.int(), EPDISD);
        }
        endpoint.int().write(u32::MAX);
        endpoint.ctl().write(0);
        self.flush_tx(ep);
        self.device
            .daintmsk
            .modify(|r, w| unsafe { w.iepm().bits(r.iepm().bits() & !(1 << ep)) });
    }

    fn flush_tx(&self, fifo: u8) {
        self.global
            .grstctl
            .write(|w| unsafe { w.txfnum().bits(fifo) }.txfflsh().set_bit());
        while self.global.grstctl.read().txfflsh().bit_is_set() {}
    }
}

const ALL_TX_FIFOS: u8 = 0b10000;

/// The PAC gives every IN endpoint register its own type, which gets in the way of driving the
/// endpoints uniformly, so those we address by hand. They sit 0x20 apart; FIFOs are 0x1000 apart.
#[derive(Clone, Copy)]
struct InEndpoint(u8);

impl InEndpoint {
    fn ctl(self) -> Reg {
        Reg(0x5000_0900 + usize::from(self.0) * 0x20)
    }

    fn int(self) -> Reg {
        Reg(0x5000_0908 + usize::from(self.0) * 0x20)
    }

    fn tsiz(self) -> Reg {
        Reg(0x5000_0910 + usize::from(self.0) * 0x20)
    }

    fn fifo(self) -> Reg {
        Reg(0x5000_1000 + usize::from(self.0) * 0x1000)
    }
}

// DIEPCTL
const EPENA: u32 = 1 << 31;
const EPDIS: u32 = 1 << 30;
const SD0PID: u32 = 1 << 28;
const SNAK: u32 = 1 << 27;
const CNAK: u32 = 1 << 26;
const TXFNUM_SHIFT: u32 = 22;
const EPTYP_INTERRUPT: u32 = 0b11 << 18;
const USBAEP: u32 = 1 << 15;
// DIEPINT
const XFRC: u32 = 1 << 0;
const EPDISD: u32 = 1 << 1;
const INEPNE: u32 = 1 << 6;
// DIEPTSIZ
const PKTCNT_SHIFT: u32 = 19;

#[derive(Clone, Copy)]
struct Reg(usize);

impl Reg {
    fn read(self) -> u32 {
        unsafe { core::ptr::read_volatile(self.0 as *const u32) }
    }

    fn write(self, value: u32) {
        unsafe { core::ptr::write_volatile(self.0 as *mut u32, value) }
    }

    fn set(self, bits: u32) {
        self.write(self.read() | bits);
    }
}

fn wait_for(reg: Reg, bit: u32) {
    for _ in 0..100_000 {
        if reg.read() & bit != 0 {
            return;
        }
    }
    panic!("OTG endpoint did not respond");
}
