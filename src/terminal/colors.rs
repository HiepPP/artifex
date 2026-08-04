//! ANSI colour resolution for the terminal grid.
//!
//! `alacritty_terminal` hands back `vte::ansi::Color`; GPUI wants `Hsla`. The
//! 16 base slots follow the Atelier palette so the terminal stays inside the
//! design system; slots 16..255 use the standard xterm cube.

use alacritty_terminal::vte::ansi::{Color as AnsiColor, NamedColor, Rgb};
use gpui::{Hsla, Rgba, rgb};

#[derive(Clone, Copy)]
pub struct TerminalPalette {
    pub background: Hsla,
    pub foreground: Hsla,
    pub cursor: Hsla,
    base: [Hsla; 16],
}

// Ported verbatim from atelier `TerminalPalette.light`/`.dark`. Slots 0..7 are
// the normal ANSI colors, 8..15 the bright set. Do not substitute the Git
// status colors here - they are a different, brighter palette.
const LIGHT_BASE: [u32; 16] = [
    0x4B4944, 0xB83A32, 0x4F7B55, 0x8A6C24, 0x3C6896, 0x864F78, 0x007A91, 0xC9C5BC, 0x726F68,
    0xD15345, 0x5F8F66, 0xA07E2D, 0x4E7CB0, 0x9B628E, 0x008DA5, 0xF1EEE7,
];

const DARK_BASE: [u32; 16] = [
    0x2C312F, 0xD97067, 0x7FBC89, 0xD0A75C, 0x72A8D4, 0xB88AC5, 0x65B9B2, 0xD8DDDA, 0x6E7572,
    0xEB8A82, 0x98D0A0, 0xE1BD75, 0x8BBBE0, 0xCAA0D4, 0x7ACEC6, 0xF0F3F1,
];

impl TerminalPalette {
    pub fn for_theme(dark: bool) -> Self {
        let raw = if dark { DARK_BASE } else { LIGHT_BASE };
        let mut base = [Hsla::default(); 16];
        for (slot, value) in raw.iter().enumerate() {
            base[slot] = hsla(*value);
        }
        Self {
            // Matches the `editor` token in DESIGN.md.
            background: hsla(if dark { 0x191B1E } else { 0xF8F7F4 }),
            // atelier `terminalForeground(usesDarkAppearance:)`.
            foreground: hsla(if dark { 0xE8E4DE } else { 0x292724 }),
            // atelier `accent`, reused as the terminal caret.
            cursor: hsla(if dark { 0xD79570 } else { 0xA44F32 }),
            base,
        }
    }

    pub fn resolve(&self, color: AnsiColor, foreground: bool) -> Hsla {
        match color {
            AnsiColor::Named(named) => self.named(named, foreground),
            AnsiColor::Spec(Rgb { r, g, b }) => {
                let raw: Rgba = rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32);
                raw.into()
            }
            AnsiColor::Indexed(index) => self.indexed(index, foreground),
        }
    }

    fn named(&self, named: NamedColor, foreground: bool) -> Hsla {
        match named {
            NamedColor::Foreground | NamedColor::BrightForeground => self.foreground,
            NamedColor::Background => self.background,
            NamedColor::Cursor => self.cursor,
            NamedColor::Black => self.base[0],
            NamedColor::Red => self.base[1],
            NamedColor::Green => self.base[2],
            NamedColor::Yellow => self.base[3],
            NamedColor::Blue => self.base[4],
            NamedColor::Magenta => self.base[5],
            NamedColor::Cyan => self.base[6],
            NamedColor::White => self.base[7],
            NamedColor::BrightBlack | NamedColor::DimBlack => self.base[8],
            NamedColor::BrightRed | NamedColor::DimRed => self.base[9],
            NamedColor::BrightGreen | NamedColor::DimGreen => self.base[10],
            NamedColor::BrightYellow | NamedColor::DimYellow => self.base[11],
            NamedColor::BrightBlue | NamedColor::DimBlue => self.base[12],
            NamedColor::BrightMagenta | NamedColor::DimMagenta => self.base[13],
            NamedColor::BrightCyan | NamedColor::DimCyan => self.base[14],
            NamedColor::BrightWhite | NamedColor::DimWhite => self.base[15],
            NamedColor::DimForeground => {
                let mut c = self.foreground;
                c.a *= 0.7;
                c
            }
        }
    }

    fn indexed(&self, index: u8, foreground: bool) -> Hsla {
        match index {
            0..=15 => self.base[index as usize],
            16..=231 => {
                let value = index - 16;
                let steps = [0u8, 95, 135, 175, 215, 255];
                let r = steps[(value / 36) as usize];
                let g = steps[((value % 36) / 6) as usize];
                let b = steps[(value % 6) as usize];
                let raw: Rgba = rgb(((r as u32) << 16) | ((g as u32) << 8) | b as u32);
                raw.into()
            }
            232..=255 => {
                let level = 8 + (index - 232) as u32 * 10;
                let raw: Rgba = rgb((level << 16) | (level << 8) | level);
                raw.into()
            }
        }
        .pipe(|c| {
            let _ = foreground;
            c
        })
    }
}

fn hsla(value: u32) -> Hsla {
    let raw: Rgba = rgb(value);
    raw.into()
}

trait Pipe: Sized {
    fn pipe<R>(self, f: impl FnOnce(Self) -> R) -> R {
        f(self)
    }
}
impl Pipe for Hsla {}
