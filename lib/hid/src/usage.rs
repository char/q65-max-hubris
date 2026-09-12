#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[rustfmt::skip]
pub enum Key {
    A = 0x04, B, C, D, E, F, G, H, I, J, K, L, M, N, O, P, Q, R, S, T, U, V, W, X, Y, Z,
    Digit1, Digit2, Digit3, Digit4, Digit5, Digit6, Digit7, Digit8, Digit9, Digit0,
    Enter, Escape, Backspace, Tab, Space,
    Minus, Equal, LeftBracket, RightBracket, Backslash, NonUsHash,
    Semicolon, Apostrophe, Grave, Comma, Dot, Slash, CapsLock,
    F1, F2, F3, F4, F5, F6, F7, F8, F9, F10, F11, F12,
    PrintScreen, ScrollLock, Pause,
    Insert, Home, PageUp, Delete, End, PageDown,
    Right, Left, Down, Up,
    NumLock, KeypadDivide, KeypadMultiply, KeypadSubtract, KeypadAdd, KeypadEnter,
    Keypad1, Keypad2, Keypad3, Keypad4, Keypad5, Keypad6, Keypad7, Keypad8, Keypad9, Keypad0,
    KeypadDecimal, NonUsBackslash, Application, Power, KeypadEqual,
    F13, F14, F15, F16, F17, F18, F19, F20, F21, F22, F23, F24,
    LeftControl = 0xe0, LeftShift, LeftAlt, LeftGui,
    RightControl, RightShift, RightAlt, RightGui,
}

impl Key {
    pub const ERROR_ROLLOVER: u8 = 0x01;
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[rustfmt::skip]
pub enum Consumer { PlayPause, VolumeUp, VolumeDown }

impl Consumer {
    #[must_use]
    pub const fn usage(self) -> u8 {
        match self {
            Self::PlayPause => 0xcd,
            Self::VolumeUp => 0xe9,
            Self::VolumeDown => 0xea,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
#[rustfmt::skip]
pub enum Led { NumLock = 0x01, CapsLock, ScrollLock, Compose, Kana }
