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

const LIGHT_BASE: [u32; 16] = [
    0x2B2724, 0xA13E37, 0x356B43, 0x8A5B21, 0x39618A, 0x7A4C86, 0x286E68, 0x6A6560, 0x4A4642,
    0xC25A50, 0x4B8B5B, 0xB07A2C, 0x4E7BAF, 0x9A63A8, 0x349288, 0x302E2B,
];

const DARK_BASE: [u32; 16] = [
    0x3A3F45, 0xE17B70, 0x7FC58C, 0xD4A45D, 0x7FA8D4, 0xC194D0, 0x63C3B8, 0xE8E4DE, 0x5A6069,
    0xF09B90, 0x9BD9A6, 0xE6BB7A, 0x9CC0E8, 0xD5AEE0, 0x86D8CE, 0xF6F3EE,
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
            foreground: hsla(if dark { 0xE9E5DF } else { 0x1E1C1A }),
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
