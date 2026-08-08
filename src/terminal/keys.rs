//! Keystroke to PTY byte encoding.
//!
//! GPUI reports keystrokes, not terminal escape sequences. Everything here is
//! hand-written; there is no GPUI or `alacritty_terminal` helper for it.

use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

pub fn encode(keystroke: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    let m = &keystroke.modifiers;
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let modifier = 1 + u8::from(m.shift) + 2 * u8::from(m.alt) + 4 * u8::from(m.control);
    let modified = m.shift || m.alt || m.control;
    let cursor_key = |code: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{modifier}{code}").into_bytes()
        } else if app_cursor {
            format!("\x1bO{code}").into_bytes()
        } else {
            format!("\x1b[{code}").into_bytes()
        }
    };
    let tilde_key = |code: u8| -> Vec<u8> {
        if modified {
            format!("\x1b[{code};{modifier}~").into_bytes()
        } else {
            format!("\x1b[{code}~").into_bytes()
        }
    };
    let function_key = |code: char| -> Vec<u8> {
        if modified {
            format!("\x1b[1;{modifier}{code}").into_bytes()
        } else {
            format!("\x1bO{code}").into_bytes()
        }
    };

    let bytes = match keystroke.key.as_str() {
        "enter" if m.shift => b"\n".to_vec(),
        "enter" => b"\r".to_vec(),
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "backspace" if m.alt => b"\x1b\x7f".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "insert" => tilde_key(2),
        "delete" => tilde_key(3),
        "escape" => b"\x1b".to_vec(),
        "up" => cursor_key('A'),
        "down" => cursor_key('B'),
        "right" => cursor_key('C'),
        "left" => cursor_key('D'),
        "home" => cursor_key('H'),
        "end" => cursor_key('F'),
        "pageup" => tilde_key(5),
        "pagedown" => tilde_key(6),
        "f1" => function_key('P'),
        "f2" => function_key('Q'),
        "f3" => function_key('R'),
        "f4" => function_key('S'),
        "f5" => tilde_key(15),
        "f6" => tilde_key(17),
        "f7" => tilde_key(18),
        "f8" => tilde_key(19),
        "f9" => tilde_key(20),
        "f10" => tilde_key(21),
        "f11" => tilde_key(23),
        "f12" => tilde_key(24),
        "space" if m.control => vec![0],
        key => {
            if m.control && key.len() == 1 {
                let ch = key.chars().next()?.to_ascii_lowercase();
                match ch {
                    'a'..='z' => vec![(ch as u8) - b'a' + 1],
                    '[' => vec![27],
                    '\\' => vec![28],
                    ']' => vec![29],
                    _ => return None,
                }
            } else {
                // Printable text never comes from here. macOS delivers it
                // through the input handler (`replace_text_in_range`), which is
                // also the only path that carries Vietnamese composition.
                // Encoding `key_char` as well would write every character
                // twice.
                return None;
            }
        }
    };

    Some(bytes)
}
