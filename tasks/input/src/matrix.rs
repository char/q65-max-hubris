use keyboard::{COLUMNS, Matrix, ROWS};
use util::Reg;

const GPIOA: usize = 0x4002_0000;
const GPIOB: usize = 0x4002_0400;
const GPIOC: usize = 0x4002_0800;
const GPIOD: usize = 0x4002_0c00;

#[derive(Clone, Copy)]
struct Pin(usize, u8);

const ROW_PINS: [Pin; ROWS] = [
    Pin(GPIOD, 2),
    Pin(GPIOB, 3),
    Pin(GPIOB, 4),
    Pin(GPIOB, 5),
    Pin(GPIOB, 6),
];

const COLUMN_PINS: [Pin; COLUMNS] = [
    Pin(GPIOC, 6),
    Pin(GPIOC, 7),
    Pin(GPIOC, 8),
    Pin(GPIOA, 14),
    Pin(GPIOA, 15),
    Pin(GPIOC, 10),
    Pin(GPIOC, 11),
    Pin(GPIOC, 13),
    Pin(GPIOC, 14),
    Pin(GPIOC, 15),
    Pin(GPIOC, 0),
    Pin(GPIOC, 1),
    Pin(GPIOC, 2),
    Pin(GPIOC, 3),
    Pin(GPIOA, 0),
    Pin(GPIOA, 1),
];

const ENCODER_PINS: [Pin; 2] = [Pin(GPIOB, 15), Pin(GPIOB, 14)];

const MODER: usize = 0x00;
const OTYPER: usize = 0x04;
const OSPEEDR: usize = 0x08;
const PUPDR: usize = 0x0c;
const IDR: usize = 0x10;
const BSRR: usize = 0x18;

const INPUT: u32 = 0b00;
const OUTPUT: u32 = 0b01;
const PULL_UP: u32 = 0b01;
const OPEN_DRAIN: u32 = 1;

impl Pin {
    fn reg(self, offset: usize) -> Reg {
        Reg(self.0 + offset)
    }

    fn field(self, offset: usize, value: u32) {
        let shift = u32::from(self.1) * 2;
        self.reg(offset)
            .modify(|bits| bits & !(0b11 << shift) | value << shift);
    }

    fn is_low(self) -> bool {
        self.reg(IDR).read() & 1 << self.1 == 0
    }

    fn drive_low(self) {
        self.reg(BSRR).write(1 << (self.1 + 16));
    }

    fn release(self) {
        self.reg(BSRR).write(1 << self.1);
    }
}

pub fn init() {
    for pin in ROW_PINS.iter().chain(&COLUMN_PINS).chain(&ENCODER_PINS) {
        pin.field(MODER, INPUT);
        pin.field(PUPDR, PULL_UP);
    }
    for pin in COLUMN_PINS {
        pin.release();
        pin.reg(OTYPER).modify(|bits| bits | OPEN_DRAIN << pin.1);
        pin.field(OSPEEDR, 0);
        pin.field(MODER, OUTPUT);
    }
}

pub fn scan() -> Matrix {
    let mut matrix = Matrix::default();
    for (column, pin) in COLUMN_PINS.into_iter().enumerate() {
        pin.drive_low();
        settle();
        for (row, pin) in ROW_PINS.into_iter().enumerate() {
            if pin.is_low() {
                matrix[row] |= 1 << column;
            }
        }
        pin.release();
        settle();
    }
    matrix
}

fn settle() {
    // cortex_m delays us for 3 cycles per "cycle" ??
    cortex_m::asm::delay(48 * 10 / 3);
}

pub fn encoder_state() -> u8 {
    ENCODER_PINS
        .iter()
        .enumerate()
        .map(|(bit, pin)| u8::from(!pin.is_low()) << bit)
        .sum()
}
