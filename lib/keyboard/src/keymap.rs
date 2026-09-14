use crate::layout::{BASE, KNOB, Layer, TAPPING_TERM_MS};
use crate::{COLUMNS, Matrix, ROWS, Rotation};
use hid::Reports;
use hid::{Consumer, Key};

const KEYS: usize = ROWS * COLUMNS;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Empty,
    Transparent,
    Key(Key),
    Media(Consumer),
    /// momentary (while-held) layer activation
    Mo(Layer),
    /// toggle layer activation
    Tg(Layer),
    /// key when tapped, layer while held (200ms)
    TapHold(Key, Layer),
    Bootloader,
}

/// Something for the task to do that isn't a report.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    EnterBootloader,
}

use Binding::{Key as Kc, Media, Mo, TapHold, Tg};

pub type Grid = [[Binding; COLUMNS]; ROWS];

pub const fn layer(base: &Grid, entries: &[(Binding, Binding)]) -> Grid {
    let mut grid = [[Binding::Transparent; COLUMNS]; ROWS];
    let mut i = 0;
    while i < entries.len() {
        let (target, binding) = entries[i];
        let (row, column) = position_of(base, target);
        assert!(
            matches!(grid[row][column], Binding::Transparent),
            "position bound twice"
        );
        grid[row][column] = binding;
        i += 1;
    }
    grid
}

const fn position_of(grid: &Grid, binding: Binding) -> (usize, usize) {
    let mut row = 0;
    while row < ROWS {
        let mut column = 0;
        while column < COLUMNS {
            if same(grid[row][column], binding) {
                return (row, column);
            }
            column += 1;
        }
        row += 1;
    }
    panic!("not on the base layer");
}

// can't use PartialEq derivation in const fn so:
const fn same(a: Binding, b: Binding) -> bool {
    match (a, b) {
        (Kc(a), Kc(b)) => a as u8 == b as u8,
        (Media(a), Media(b)) => a as u8 == b as u8,
        (Mo(a), Mo(b)) | (Tg(a), Tg(b)) => a as u8 == b as u8,
        (TapHold(a, x), TapHold(b, y)) => a as u8 == b as u8 && x as u8 == y as u8,
        (Binding::Empty, Binding::Empty)
        | (Binding::Transparent, Binding::Transparent)
        | (Binding::Bootloader, Binding::Bootloader) => true,
        _ => false,
    }
}

#[derive(Clone, Copy, Default)]
struct Event {
    position: usize,
    pressed: bool,
}

#[derive(Clone, Copy)]
struct Pending {
    position: usize,
    since: u64,
    key: Key,
    layer: Layer,
}

/// while a tap-hold key is undecided, we buffer all events
#[derive(Default)]
struct Buffered {
    events: [Event; 16],
    len: usize,
}

impl Buffered {
    fn push(&mut self, event: Event) -> bool {
        let Some(slot) = self.events.get_mut(self.len) else {
            return false;
        };
        *slot = event;
        self.len += 1;
        true
    }

    fn drain(&mut self) -> impl Iterator<Item = Event> + use<> {
        let events = self.events;
        events.into_iter().take(core::mem::take(&mut self.len))
    }
}

pub struct Keymap {
    previous: Matrix,
    held: [Binding; KEYS],
    toggled: [bool; Layer::ALL.len()],
    // undecided tap/hold key
    pending: Option<Pending>,
    buffered: Buffered,
    command: Option<Command>,
}

impl Default for Keymap {
    fn default() -> Self {
        Self {
            previous: Matrix::default(),
            held: [Binding::Empty; KEYS],
            toggled: [false; Layer::ALL.len()],
            pending: None,
            buffered: Buffered::default(),
            command: None,
        }
    }
}

impl Keymap {
    #[must_use]
    pub fn is_idle(&self) -> bool {
        self.previous.iter().all(|&row| row == 0)
            && self.pending.is_none()
            && self.buffered.len == 0
    }

    fn layer_active(&self, layer: Layer) -> bool {
        self.toggled[layer as usize] || self.held.contains(&Mo(layer))
    }

    fn binding(&self, position: usize) -> Binding {
        let (row, column) = (position / COLUMNS, position % COLUMNS);
        Layer::ALL
            .into_iter()
            .rev()
            .filter(|&layer| self.layer_active(layer))
            .map(|layer| layer.grid()[row][column])
            .find(|&binding| binding != Binding::Transparent)
            .unwrap_or(BASE[row][column])
    }

    #[must_use]
    pub fn report(&self) -> Reports {
        let mut reports = Reports::default();
        for binding in self.held {
            match binding {
                Kc(key) => reports.keyboard.press(key),
                Media(control) => reports.consumer.press(control),
                _ => {}
            }
        }
        reports
    }

    fn apply(&mut self, event: Event) {
        let binding = if event.pressed {
            self.binding(event.position)
        } else {
            self.held[event.position]
        };
        self.held[event.position] = if event.pressed {
            binding
        } else {
            Binding::Empty
        };
        match binding {
            Tg(layer) if event.pressed => self.toggled[layer as usize] ^= true,
            Mo(released) if !event.pressed => {
                for layer in Layer::ALL {
                    if layer.ends_with() == Some(released) {
                        self.toggled[layer as usize] = false;
                    }
                }
            }
            Binding::Bootloader if event.pressed => self.command = Some(Command::EnterBootloader),
            _ => {}
        }
    }

    fn resolve(&mut self, hold: bool, emit: &mut impl FnMut(Reports)) {
        let Some(pending) = self.pending.take() else {
            return;
        };
        if hold {
            self.held[pending.position] = Mo(pending.layer);
        } else {
            self.held[pending.position] = Kc(pending.key);
            emit(self.report());
            self.held[pending.position] = Binding::Empty;
            emit(self.report());
        }
        for event in self.buffered.drain() {
            self.apply(event);
            emit(self.report());
        }
    }

    fn event(&mut self, event: Event, now: u64, emit: &mut impl FnMut(Reports)) {
        if let Some(pending) = self.pending {
            if event.position == pending.position && !event.pressed {
                self.resolve(false, emit);
                return;
            }
            if !event.pressed && matches!(self.held[event.position], Kc(_) | Media(_)) {
                self.apply(event);
                return;
            }
            if self.buffered.push(event) {
                return;
            }
            self.resolve(true, emit);
        }
        if event.pressed
            && let binding @ TapHold(key, layer) = self.binding(event.position)
        {
            self.held[event.position] = binding;
            self.pending = Some(Pending {
                position: event.position,
                since: now,
                key,
                layer,
            });
            return;
        }
        self.apply(event);
    }

    pub fn update(
        &mut self,
        matrix: Matrix,
        now: u64,
        mut emit: impl FnMut(Reports),
    ) -> Option<Command> {
        let mut last = self.report();
        let mut emit = |report: Reports| {
            if report != last {
                emit(report);
                last = report;
            }
        };
        if let Some(pending) = self.pending
            && now - pending.since >= TAPPING_TERM_MS
        {
            self.resolve(true, &mut emit);
        }
        for position in 0..KEYS {
            let bit = 1 << (position % COLUMNS);
            let pressed = matrix[position / COLUMNS] & bit != 0;
            if pressed != (self.previous[position / COLUMNS] & bit != 0) {
                self.event(Event { position, pressed }, now, &mut emit);
            }
        }
        self.previous = matrix;
        emit(self.report());
        self.command.take()
    }

    pub fn turn(&self, rotation: Rotation, mut emit: impl FnMut(Reports)) {
        let released = self.report();
        let mut pressed = released;
        pressed.consumer.press(match rotation {
            Rotation::Clockwise => KNOB.1,
            Rotation::CounterClockwise => KNOB.0,
        });
        emit(pressed);
        emit(released);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hid::Consumer::VolumeUp;
    use hid::Key::{A, CapsLock, F1, Insert, Left, Q, W};

    const DIGIT_1: (usize, usize) = (0, 2);
    const Q_KEY: (usize, usize) = (1, 2);
    const W_KEY: (usize, usize) = (1, 3);
    const CAPS: (usize, usize) = (2, 1);
    const A_KEY: (usize, usize) = (2, 2);
    const NAV_KEY: (usize, usize) = (4, 11);
    const F22_KEY: (usize, usize) = (4, 0);

    fn keys(keys: &[Key]) -> Reports {
        let mut reports = Reports::default();
        for &key in keys {
            reports.keyboard.press(key);
        }
        reports
    }

    fn scan(keymap: &mut Keymap, pressed: &[(usize, usize)], now: u64) -> Vec<Reports> {
        let mut matrix = Matrix::default();
        for &(row, column) in pressed {
            matrix[row] |= 1 << column;
        }
        let mut emitted = Vec::new();
        keymap.update(matrix, now, |report| emitted.push(report));
        emitted
    }

    #[test]
    fn keys_press_and_release() {
        let mut keymap = Keymap::default();
        assert_eq!(scan(&mut keymap, &[A_KEY], 0), [keys(&[A])]);
        assert_eq!(scan(&mut keymap, &[A_KEY], 1), []);
        assert_eq!(scan(&mut keymap, &[], 2), [keys(&[])]);
    }

    #[test]
    fn momentary_layer_applies_to_new_presses_only() {
        let mut keymap = Keymap::default();
        scan(&mut keymap, &[W_KEY], 0);
        assert_eq!(scan(&mut keymap, &[W_KEY, NAV_KEY], 1), []);
        assert_eq!(
            scan(&mut keymap, &[W_KEY, NAV_KEY, A_KEY], 2),
            [keys(&[W, Left])]
        );
        // Releasing Nav doesn't change what Left was.
        assert_eq!(scan(&mut keymap, &[W_KEY, A_KEY], 3), []);
        assert_eq!(scan(&mut keymap, &[], 4), [keys(&[])]);
    }

    #[test]
    fn caps_tapped_is_caps_lock() {
        let mut keymap = Keymap::default();
        assert_eq!(scan(&mut keymap, &[CAPS], 0), []);
        assert_eq!(scan(&mut keymap, &[], 100), [keys(&[CapsLock]), keys(&[])]);
    }

    #[test]
    fn caps_held_is_the_function_layer() {
        let mut keymap = Keymap::default();
        scan(&mut keymap, &[CAPS], 0);
        assert_eq!(scan(&mut keymap, &[CAPS], TAPPING_TERM_MS), []);
        assert_eq!(scan(&mut keymap, &[CAPS, DIGIT_1], 210), [keys(&[F1])]);
        assert_eq!(scan(&mut keymap, &[DIGIT_1], 220), []);
        assert_eq!(scan(&mut keymap, &[], 230), [keys(&[])]);
    }

    #[test]
    fn keys_pressed_while_caps_is_undecided_wait_for_the_decision() {
        let mut keymap = Keymap::default();
        scan(&mut keymap, &[CAPS], 0);
        assert_eq!(scan(&mut keymap, &[CAPS, A_KEY], 50), []);
        assert_eq!(
            scan(&mut keymap, &[A_KEY], 100),
            [keys(&[CapsLock]), keys(&[]), keys(&[A])]
        );
        let mut keymap = Keymap::default();
        scan(&mut keymap, &[CAPS], 0);
        scan(&mut keymap, &[CAPS, DIGIT_1], 50);
        assert_eq!(scan(&mut keymap, &[CAPS, DIGIT_1], 250), [keys(&[F1])]);
    }

    #[test]
    fn releasing_an_earlier_key_does_not_wait_on_caps() {
        let mut keymap = Keymap::default();
        scan(&mut keymap, &[A_KEY], 0);
        scan(&mut keymap, &[A_KEY, CAPS], 10);
        assert_eq!(scan(&mut keymap, &[CAPS], 20), [keys(&[])]);
        assert_eq!(scan(&mut keymap, &[], 30), [keys(&[CapsLock]), keys(&[])]);
    }

    #[test]
    fn ext_toggles_from_nav_and_ends_with_it() {
        let mut keymap = Keymap::default();
        scan(&mut keymap, &[NAV_KEY], 0);
        scan(&mut keymap, &[NAV_KEY, CAPS], 1);
        scan(&mut keymap, &[NAV_KEY], 2);
        assert_eq!(scan(&mut keymap, &[NAV_KEY, Q_KEY], 3), [keys(&[Insert])]);
        scan(&mut keymap, &[NAV_KEY], 4);
        scan(&mut keymap, &[], 5);
        assert_eq!(scan(&mut keymap, &[Q_KEY], 6), [keys(&[Q])]);
    }

    #[test]
    fn the_bootloader_key_is_a_command_not_a_report() {
        let mut keymap = Keymap::default();
        scan(&mut keymap, &[NAV_KEY], 0);
        scan(&mut keymap, &[NAV_KEY, CAPS], 1);
        scan(&mut keymap, &[NAV_KEY], 2);
        let mut matrix = Matrix::default();
        matrix[NAV_KEY.0] |= 1 << NAV_KEY.1;
        matrix[F22_KEY.0] |= 1 << F22_KEY.1;
        let mut emitted = Vec::new();
        let command = keymap.update(matrix, 3, |report| emitted.push(report));
        assert_eq!(command, Some(Command::EnterBootloader));
        assert_eq!(emitted, []);
        assert_eq!(scan(&mut keymap, &[F22_KEY], 4), []);
    }

    #[test]
    fn the_knob_taps_volume() {
        let mut keymap = Keymap::default();
        scan(&mut keymap, &[A_KEY], 0);
        let mut emitted = Vec::new();
        keymap.turn(Rotation::Clockwise, |report| emitted.push(report));
        let mut up = keys(&[A]);
        up.consumer.press(VolumeUp);
        assert_eq!(emitted, [up, keys(&[A])]);
    }

    #[test]
    fn tap_holds_and_layers_are_busy_even_without_a_hid_report() {
        let mut keymap = Keymap::default();
        let mut activity = crate::Activity::new(0, 3);
        assert!(keymap.is_idle());
        assert_eq!(
            activity.scan_interval(30_000, true, !keymap.is_idle(), 3),
            8
        );

        scan(&mut keymap, &[CAPS], 30_008);
        assert_eq!(keymap.report(), Reports::default());
        assert!(!keymap.is_idle());
        assert_eq!(
            activity.scan_interval(30_008, true, !keymap.is_idle(), 3),
            1
        );

        scan(&mut keymap, &[CAPS], 61_000);
        assert_eq!(keymap.report(), Reports::default());
        assert_eq!(
            activity.scan_interval(61_000, true, !keymap.is_idle(), 3),
            1
        );
        scan(&mut keymap, &[], 61_001);
        assert!(keymap.is_idle());

        scan(&mut keymap, &[CAPS], 62_000);
        scan(&mut keymap, &[CAPS, A_KEY], 62_001);
        assert_eq!(keymap.report(), Reports::default());
        assert!(!keymap.is_idle());
        scan(&mut keymap, &[], 62_002);
        assert!(keymap.is_idle());
    }
}
