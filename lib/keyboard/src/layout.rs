use crate::keymap::{Binding, Grid, layer};
use hid::Consumer::{self, PlayPause, VolumeDown, VolumeUp};
#[allow(clippy::enum_glob_use, reason = "the grid should read like a keymap")]
use hid::Key::*;

/// threshold from caps lock press to function switch
pub const TAPPING_TERM_MS: u64 = 200;

/// (counter-clockwise, clockwise)
pub const KNOB: (Consumer, Consumer) = (VolumeDown, VolumeUp);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layer {
    Nav,
    Ext,
    FKey,
}

use Binding::{Key as Kc, Media, Mo, TapHold, Tg};
use Layer::{Ext, FKey, Nav};

impl Layer {
    pub const ALL: [Self; 3] = [Nav, Ext, FKey];

    pub const fn grid(self) -> &'static Grid {
        match self {
            Nav => &NAV,
            Ext => &EXT,
            FKey => &FKEY,
        }
    }

    pub const fn ends_with(self) -> Option<Self> {
        match self {
            Ext => Some(Nav),
            Nav | FKey => None,
        }
    }
}

const XXX: Binding = Binding::Empty;

#[rustfmt::skip]
pub const BASE: Grid = [
    [Media(PlayPause), Kc(Escape), Kc(Digit1), Kc(Digit2), Kc(Digit3), Kc(Digit4), Kc(Digit5), Kc(Digit6), Kc(Digit7), Kc(Digit8), Kc(Digit9), Kc(Digit0), Kc(Minus), Kc(Equal), Kc(Backspace), Kc(Delete)],
    [Kc(F19), Kc(Tab), Kc(Q), Kc(W), Kc(E), Kc(R), Kc(T), Kc(Y), Kc(U), Kc(I), Kc(O), Kc(P), Kc(LeftBracket), Kc(RightBracket), Kc(Backslash), Kc(Home)],
    [Kc(F20), TapHold(CapsLock, FKey), Kc(A), Kc(S), Kc(D), Kc(F), Kc(G), Kc(H), Kc(J), Kc(K), Kc(L), Kc(Semicolon), Kc(Apostrophe), Kc(Enter), Kc(End), XXX],
    [Kc(F21), Kc(LeftShift), XXX, Kc(Z), Kc(X), Kc(C), Kc(V), Kc(B), Kc(N), Kc(M), Kc(Comma), Kc(Dot), Kc(Slash), Kc(RightShift), Kc(Up), Kc(PrintScreen)],
    [Kc(F22), Kc(LeftControl), Kc(LeftGui), Kc(LeftAlt), XXX, XXX, XXX, Kc(Space), XXX, XXX, Kc(RightAlt), Mo(Nav), Kc(RightControl), Kc(Left), Kc(Down), Kc(Right)],
];

const NAV: Grid = layer(
    &BASE,
    &[
        (Kc(Escape), Kc(Grave)),
        (Kc(Backspace), Kc(Delete)),
        (Kc(Tab), Kc(PrintScreen)),
        (Kc(W), Kc(Up)),
        (TapHold(CapsLock, FKey), Tg(Ext)),
        (Kc(A), Kc(Left)),
        (Kc(S), Kc(Down)),
        (Kc(D), Kc(Right)),
    ],
);

const EXT: Grid = layer(
    &BASE,
    &[
        (Kc(Q), Kc(Insert)),
        (Kc(W), Kc(PageUp)),
        (Kc(E), Kc(ScrollLock)),
        (Kc(A), Kc(Home)),
        (Kc(S), Kc(PageDown)),
        (Kc(D), Kc(End)),
    ],
);

const FKEY: Grid = layer(
    &BASE,
    &[
        (Kc(Digit1), Kc(F1)),
        (Kc(Digit2), Kc(F2)),
        (Kc(Digit3), Kc(F3)),
        (Kc(Digit4), Kc(F4)),
        (Kc(Digit5), Kc(F5)),
        (Kc(Digit6), Kc(F6)),
        (Kc(Digit7), Kc(F7)),
        (Kc(Digit8), Kc(F8)),
        (Kc(Digit9), Kc(F9)),
        (Kc(Digit0), Kc(F10)),
        (Kc(Minus), Kc(F11)),
        (Kc(Equal), Kc(F12)),
    ],
);
