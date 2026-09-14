use crate::power::Battery;
use crate::reports::ReportQueue;
use crate::{Ack, Connection, Event, Packet};
use hid::{ConsumerReport, KeyboardReport, LedReport, Reports};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum State {
    ResetLow,
    Booting,
    Name,
    Configure,
    Disabled,
    LowBattery,
    WakeLow,
    Waking,
    Connecting,
    Connected,
    Pairing,
    Releasing,
    DisconnectLow,
    Disconnecting,
}

#[derive(Debug)]
pub enum Action {
    Reset(bool),
    Pulse(u16),
    Send(Packet),
}

struct InFlight {
    packet: Packet,
    sequence: u8,
    command: u8,
    deadline: u64,
    attempts: u8,
}

pub struct Link {
    pub state: State,
    pub leds: LedReport,
    pub errors: u32,
    pub resets: u32,
    pub battery: Battery,
    next_battery: u64,
    enabled: bool,
    pairing: bool,
    deadline: u64,
    sequence: u8,
    connection_ack: bool,
    interval: u64,
    next_report: u64,
    pub reports: ReportQueue,
    sent_keyboard: Option<KeyboardReport>,
    sent_consumer: Option<ConsumerReport>,
    in_flight: Option<InFlight>,
}

impl Default for Link {
    fn default() -> Self {
        Self {
            state: State::ResetLow,
            leds: LedReport::default(),
            errors: 0,
            resets: 0,
            battery: Battery::default(),
            next_battery: 3000,
            enabled: false,
            pairing: false,
            deadline: 0,
            sequence: 0,
            connection_ack: false,
            interval: 1,
            next_report: 0,
            reports: {
                let mut queue = ReportQueue::default();
                queue.reconnect();
                queue
            },
            sent_keyboard: None,
            sent_consumer: None,
            in_flight: None,
        }
    }
}

impl Link {
    pub fn enable(&mut self, enabled: bool, now: u64) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        self.pairing = false;
        if enabled && self.state == State::Disabled {
            self.state = State::WakeLow;
            self.deadline = now;
        } else if !enabled && self.state == State::Connected {
            self.start_release(now);
        } else if !enabled && matches!(self.state, State::Connecting | State::Pairing) {
            self.state = State::DisconnectLow;
            self.deadline = now;
            self.in_flight = None;
        }
    }

    pub fn pair(&mut self, now: u64) {
        if self.enabled
            && matches!(
                self.state,
                State::Connected | State::Connecting | State::Pairing
            )
        {
            self.pairing = true;
            if self.state == State::Connected {
                self.start_release(now);
            } else {
                self.in_flight = None;
                self.state = State::WakeLow;
                self.deadline = now;
            }
        }
    }

    pub fn set_reports(&mut self, reports: Reports) {
        self.reports.submit(reports, self.state == State::Connected);
    }

    fn start_release(&mut self, now: u64) {
        self.reports.release();
        self.sent_keyboard = None;
        self.sent_consumer = None;
        self.in_flight = None;
        self.state = State::Releasing;
        self.deadline = now + 100;
    }

    pub fn failed(&mut self, now: u64) {
        self.errors = self.errors.saturating_add(1);
        self.state = State::ResetLow;
        self.deadline = now + 100;
        self.in_flight = None;
        self.connection_ack = false;
        self.leds = LedReport::default();
    }

    pub fn event(&mut self, event: Event, now: u64) {
        match event {
            Event::Reset => {
                self.resets = self.resets.saturating_add(1);
                self.in_flight = None;
                if !matches!(self.state, State::ResetLow | State::Booting) {
                    self.state = State::Name;
                    self.deadline = now + 3;
                }
            }
            Event::Connection { state, host } => {
                self.connection_ack = true;
                if matches!(
                    self.state,
                    State::ResetLow
                        | State::Booting
                        | State::Name
                        | State::Configure
                        | State::WakeLow
                        | State::Waking
                        | State::DisconnectLow
                        | State::Disconnecting
                        | State::Releasing
                ) {
                    return;
                }
                if !self.enabled {
                    if state != Connection::Disconnected && state != Connection::Sleeping {
                        self.state = State::DisconnectLow;
                        self.deadline = now;
                    }
                    return;
                }
                match (state, host) {
                    (Connection::Connected, crate::HOST_2P4) => {
                        if self.state != State::Connected {
                            self.in_flight = None;
                            self.sent_keyboard = None;
                            self.reports.reconnect();
                            self.sent_consumer = None;
                            self.leds = LedReport::default();
                        }
                        self.state = State::Connected;
                        self.pairing = false;
                    }
                    (Connection::Pairing, crate::HOST_2P4) => {
                        self.state = State::Pairing;
                        self.deadline = now + 180_000;
                    }
                    (Connection::Reconnecting, crate::HOST_2P4) => {
                        self.state = State::Connecting;
                        self.deadline = now + 10_000;
                    }
                    _ => {
                        self.in_flight = None;
                        self.leds = LedReport::default();
                        self.state = State::WakeLow;
                        self.deadline = now + 1000;
                    }
                }
            }
            Event::Leds(leds) => self.leds = LedReport(leds),
            Event::Interval(interval) => self.interval = u64::from(interval).max(1),
            Event::Ack {
                sequence,
                command,
                result,
            } => {
                if let Some(pending) = &mut self.in_flight
                    && pending.sequence == sequence
                    && pending.command == command
                {
                    match result {
                        Ack::Success | Ack::HalfFull => {
                            self.in_flight = None;
                            self.next_report =
                                now + self.interval + if result == Ack::HalfFull { 5 } else { 0 };
                        }
                        Ack::ChecksumError | Ack::Full => {
                            pending.deadline = now + self.interval + 10;
                        }
                    }
                }
            }
            Event::Battery(raw) => {
                self.battery.sample(raw, now);
                if self.battery.critical {
                    self.pairing = false;
                    if self.state == State::Connected {
                        self.start_release(now);
                    } else if matches!(
                        self.state,
                        State::Connecting | State::Pairing | State::WakeLow
                    ) {
                        self.state = State::DisconnectLow;
                        self.deadline = now;
                    }
                }
            }
        }
    }

    fn packet(&mut self, payload: &[u8], ack: bool) -> Packet {
        self.sequence = self.sequence.wrapping_add(1).max(1);
        Packet::command(self.sequence, payload, ack).unwrap()
    }

    #[expect(
        clippy::too_many_lines,
        reason = "keep timed state transitions together"
    )]
    pub fn step(&mut self, now: u64) -> Option<Action> {
        if self.state == State::LowBattery && !self.battery.critical {
            self.state = if self.enabled {
                State::WakeLow
            } else {
                State::Disabled
            };
            self.deadline = now;
        }
        // PA4 is held low during these states: clocking even an event read would
        // turn a wake pulse into an unintended radio transaction.
        if now < self.deadline
            && matches!(
                self.state,
                State::Booting | State::Waking | State::Disconnecting
            )
        {
            return None;
        }
        if self.connection_ack && self.can_read() {
            self.connection_ack = false;
            return Some(Action::Send(self.packet(&crate::CONNECTION_ACK, false)));
        }
        if now >= self.next_battery
            && self.in_flight.is_none()
            && matches!(
                self.state,
                State::Connected | State::Connecting | State::Pairing
            )
        {
            self.next_battery = now + 3000;
            return Some(Action::Send(self.packet(&crate::BATTERY_QUERY, false)));
        }
        if self.state == State::Releasing && now >= self.deadline {
            self.state = if self.pairing {
                State::WakeLow
            } else {
                State::DisconnectLow
            };
            self.in_flight = None;
        }
        if !matches!(self.state, State::Connected | State::Releasing) && now < self.deadline {
            return None;
        }
        match self.state {
            State::ResetLow => {
                self.state = State::Booting;
                self.deadline = now + 1;
                Some(Action::Reset(true))
            }
            State::Booting => {
                self.state = State::Name;
                // QMK also handles CKBT51 variants whose first boot event takes nearly a second.
                self.deadline = now + 1000;
                Some(Action::Reset(false))
            }
            State::Name => {
                self.state = State::Configure;
                self.deadline = now + 3;
                Some(Action::Send(self.packet(crate::NAME, false)))
            }
            State::Configure => {
                self.state = if self.enabled && !self.battery.critical {
                    State::WakeLow
                } else {
                    State::DisconnectLow
                };
                self.deadline = now + 3;
                Some(Action::Send(
                    self.packet(&crate::configuration(7200), false),
                ))
            }
            State::Disabled | State::LowBattery => None,
            State::WakeLow => {
                self.state = State::Waking;
                self.deadline = now + 310;
                Some(Action::Pulse(10))
            }
            State::Waking => {
                if !self.enabled || self.battery.critical {
                    self.state = State::DisconnectLow;
                    self.deadline = now;
                    return None;
                }
                self.state = if self.pairing {
                    State::Pairing
                } else {
                    State::Connecting
                };
                self.deadline = now + if self.pairing { 180_000 } else { 10_000 };
                Some(Action::Send(self.packet(
                    if self.pairing {
                        &crate::PAIR
                    } else {
                        &crate::CONNECT
                    },
                    true,
                )))
            }
            State::Connecting | State::Pairing => {
                self.pairing = false;
                self.state = State::WakeLow;
                self.deadline = now + 1000;
                None
            }
            State::DisconnectLow => {
                self.state = State::Disconnecting;
                self.deadline = now + 100;
                Some(Action::Pulse(100))
            }
            State::Disconnecting => {
                self.state = if self.battery.critical {
                    State::LowBattery
                } else if self.enabled {
                    State::WakeLow
                } else {
                    State::Disabled
                };
                self.deadline = now + 3;
                self.leds = LedReport::default();
                Some(Action::Send(self.packet(&crate::DISCONNECT, true)))
            }
            State::Connected | State::Releasing => self.send_report(now),
        }
    }

    fn send_report(&mut self, now: u64) -> Option<Action> {
        if let Some(pending) = &mut self.in_flight {
            if now < pending.deadline {
                return None;
            }
            if pending.attempts == 3 {
                self.failed(now);
                return None;
            }
            pending.attempts += 1;
            pending.deadline = now + self.interval + 10;
            return Some(Action::Send(pending.packet.clone()));
        }
        if now < self.next_report {
            return None;
        }
        while let Some(report) = self.reports.front() {
            let packet = if Some(report.keyboard) != self.sent_keyboard {
                self.sent_keyboard = Some(report.keyboard);
                self.packet(&crate::nkro(report.keyboard), true)
            } else if Some(report.consumer) != self.sent_consumer {
                self.sent_consumer = Some(report.consumer);
                self.packet(&crate::consumer(report.consumer), true)
            } else {
                self.reports.pop();
                continue;
            };
            self.in_flight = Some(InFlight {
                sequence: self.sequence,
                command: packet.bytes()[9],
                packet: packet.clone(),
                deadline: now + self.interval + 10,
                attempts: 1,
            });
            return Some(Action::Send(packet));
        }
        if self.state == State::Releasing {
            self.state = if self.pairing {
                State::WakeLow
            } else {
                State::DisconnectLow
            };
            self.deadline = now;
        }
        None
    }

    pub fn pulse_started(&mut self, now: u64) {
        // Account for time spent waiting for the SPI service to accept the pulse.
        self.deadline = now
            + if self.state == State::Waking {
                310
            } else {
                100
            };
    }

    #[must_use]
    pub fn poll_interval(&self, now: u64) -> u64 {
        match self.state {
            State::Disabled | State::LowBattery => 100,
            State::Connected
                if self.battery.flags(now) & crate::power::USB_POWER == 0
                    && self.in_flight.is_none()
                    && !self.connection_ack
                    && self.reports.front().is_none() =>
            {
                8
            }
            _ => 1,
        }
    }

    #[must_use]
    pub fn can_read(&self) -> bool {
        !matches!(
            self.state,
            State::ResetLow
                | State::Booting
                | State::Disabled
                | State::LowBattery
                | State::Waking
                | State::Disconnecting
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn startup_and_wake_do_not_block_or_transmit_during_reset() {
        let mut link = Link::default();
        link.enable(true, 0);
        assert!(matches!(link.step(0), Some(Action::Reset(true))));
        assert!(link.step(0).is_none());
        assert!(!link.can_read());
        assert!(matches!(link.step(1), Some(Action::Reset(false))));
        assert!(link.step(1000).is_none());
        assert!(matches!(link.step(1001), Some(Action::Send(_))));
        assert!(matches!(link.step(1004), Some(Action::Send(_))));
        assert!(matches!(link.step(1007), Some(Action::Pulse(10))));
        assert!(!link.can_read());
        assert!(link.step(1316).is_none());
        let Some(Action::Send(packet)) = link.step(1317) else {
            panic!()
        };
        assert_eq!(&packet.bytes()[9..13], &crate::CONNECT);
    }

    #[test]
    fn lost_ack_retries_identical_packet_then_recovers() {
        let mut link = Link {
            enabled: true,
            state: State::Connected,
            ..Link::default()
        };
        let Some(Action::Send(first)) = link.step(0) else {
            panic!()
        };
        let Some(Action::Send(retry)) = link.step(11) else {
            panic!()
        };
        assert_eq!(first, retry);
        assert!(matches!(link.step(22), Some(Action::Send(_))));
        assert!(link.step(33).is_none());
        assert_eq!(link.state, State::ResetLow);
        assert_eq!(link.errors, 1);
    }

    #[test]
    fn stale_ack_cannot_complete_a_new_report() {
        let mut link = Link {
            state: State::Connected,
            ..Link::default()
        };
        let Some(Action::Send(first)) = link.step(0) else {
            panic!()
        };
        link.event(
            Event::Ack {
                sequence: first.bytes()[8].wrapping_add(1),
                command: first.bytes()[9],
                result: Ack::Success,
            },
            1,
        );
        assert!(link.step(2).is_none());
        link.event(
            Event::Ack {
                sequence: first.bytes()[8],
                command: first.bytes()[9],
                result: Ack::Success,
            },
            3,
        );
        assert!(matches!(link.step(4), Some(Action::Send(_))));
    }

    #[test]
    fn keyboard_and_encoder_taps_survive_an_ack_delay() {
        let mut link = Link {
            enabled: true,
            state: State::Connected,
            ..Link::default()
        };
        let mut pressed = Reports::default();
        pressed.keyboard.press(hid::Key::A);
        pressed.consumer.press(hid::Consumer::VolumeUp);
        link.set_reports(pressed);
        link.set_reports(Reports::default());
        let mut keyboards = Vec::new();
        let mut consumers = Vec::new();
        for now in 0..100 {
            if let Some(Action::Send(packet)) = link.step(now) {
                let bytes = packet.bytes();
                match bytes[9] {
                    0x12 => keyboards.push(bytes[11]),
                    0x13 => consumers.push(bytes[10]),
                    _ => panic!("unexpected control command"),
                }
                link.event(
                    Event::Ack {
                        sequence: bytes[8],
                        command: bytes[9],
                        result: Ack::Success,
                    },
                    now + 3,
                );
            }
        }
        assert_eq!(keyboards, [0, 0x10, 0]);
        assert_eq!(consumers, [0, 0xe9, 0]);
    }

    #[test]
    fn leaving_wireless_releases_both_report_types_before_disconnect() {
        let mut link = Link {
            enabled: true,
            state: State::Connected,
            ..Link::default()
        };
        link.enable(false, 0);
        let mut releases = Vec::new();
        for now in 0..30 {
            match link.step(now) {
                Some(Action::Send(packet)) => {
                    let bytes = packet.bytes();
                    assert!(bytes[10..bytes.len() - 2].iter().all(|&b| b == 0));
                    releases.push(bytes[9]);
                    link.event(
                        Event::Ack {
                            sequence: bytes[8],
                            command: bytes[9],
                            result: Ack::Success,
                        },
                        now,
                    );
                }
                Some(Action::Pulse(100)) => {
                    assert_eq!(releases, [0x12, 0x13]);
                    return;
                }
                None => {}
                _ => panic!("unexpected action before release"),
            }
        }
        panic!("never disconnected");
    }

    #[test]
    fn disabling_during_wake_does_not_initiate_a_connection() {
        let mut link = Link {
            enabled: true,
            state: State::Waking,
            deadline: 310,
            ..Link::default()
        };
        link.enable(false, 20);
        assert!(link.step(309).is_none());
        assert!(link.step(310).is_none());
        assert!(matches!(link.step(311), Some(Action::Pulse(100))));
    }

    #[test]
    fn critical_battery_releases_then_stays_quiet_until_usb_returns() {
        let mut link = Link {
            enabled: true,
            state: State::Connected,
            ..Link::default()
        };
        link.event(Event::Battery(1500), 0);
        link.event(Event::Battery(1500), 60_000);
        let mut releases = Vec::new();
        for now in 60_000..60_200 {
            match link.step(now) {
                Some(Action::Send(packet)) => {
                    let bytes = packet.bytes();
                    if bytes[9] == 0x23 {
                        assert_eq!(releases, [0x12, 0x13]);
                    } else {
                        assert!(bytes[10..bytes.len() - 2].iter().all(|&b| b == 0));
                        releases.push(bytes[9]);
                    }
                    link.event(
                        Event::Ack {
                            sequence: bytes[8],
                            command: bytes[9],
                            result: Ack::Success,
                        },
                        now,
                    );
                }
                Some(Action::Pulse(100)) => link.pulse_started(now),
                None => {}
                _ => panic!("unexpected action"),
            }
        }
        assert_eq!(link.state, State::LowBattery);
        assert!(!link.can_read());
        assert!(link.step(70_000).is_none());
        link.battery.power(true, true);
        assert!(matches!(link.step(70_001), Some(Action::Pulse(10))));
    }

    #[test]
    fn quiet_connected_polling_slows_only_on_battery() {
        let mut link = Link {
            state: State::Connected,
            reports: ReportQueue::default(),
            ..Link::default()
        };
        assert_eq!(link.poll_interval(100), 8);
        link.battery.power(true, false);
        assert_eq!(link.poll_interval(100), 1);
        link.battery.power(false, false);
        assert_eq!(link.poll_interval(100), 8);

        link.event(
            Event::Connection {
                state: Connection::Connected,
                host: crate::HOST_2P4,
            },
            101,
        );
        assert_eq!(link.poll_interval(101), 1);
    }

    #[test]
    fn queued_reports_and_unacknowledged_retries_keep_polling_fast() {
        let mut link = Link {
            enabled: true,
            state: State::Connected,
            reports: ReportQueue::default(),
            ..Link::default()
        };
        assert_eq!(link.poll_interval(100), 8);
        let mut pressed = Reports::default();
        pressed.keyboard.press(hid::Key::A);
        link.set_reports(pressed);
        link.set_reports(Reports::default());
        assert_eq!(link.poll_interval(100), 1);
        let Some(Action::Send(first)) = link.step(100) else {
            panic!()
        };
        assert_eq!(link.poll_interval(105), 1);
        link.event(
            Event::Ack {
                sequence: first.bytes()[8],
                command: first.bytes()[9],
                result: Ack::Full,
            },
            105,
        );
        assert_eq!(link.poll_interval(110), 1);

        let mut keyboards = Vec::new();
        for now in 116..200 {
            if let Some(Action::Send(packet)) = link.step(now) {
                assert_eq!(link.poll_interval(now), 1);
                let bytes = packet.bytes();
                if bytes[9] == 0x12 {
                    keyboards.push(bytes[11]);
                }
                link.event(
                    Event::Ack {
                        sequence: bytes[8],
                        command: bytes[9],
                        result: Ack::Success,
                    },
                    now,
                );
            }
        }
        assert_eq!(keyboards, [0x10, 0]);
        assert_eq!(link.poll_interval(200), 8);
    }

    #[test]
    fn control_sequences_never_use_the_connected_idle_rate() {
        for state in [
            State::ResetLow,
            State::Booting,
            State::Name,
            State::Configure,
            State::WakeLow,
            State::Waking,
            State::Connecting,
            State::Pairing,
            State::Releasing,
            State::DisconnectLow,
            State::Disconnecting,
        ] {
            let link = Link {
                state,
                reports: ReportQueue::default(),
                ..Link::default()
            };
            assert_eq!(link.poll_interval(100), 1, "{state:?}");
        }
    }
}
