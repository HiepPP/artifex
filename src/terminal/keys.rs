//! Keystroke to PTY byte encoding.
//!
//! GPUI reports keystrokes, not terminal escape sequences. Everything here is
//! hand-written; there is no GPUI or `alacritty_terminal` helper for it.

use alacritty_terminal::term::TermMode;
use gpui::Keystroke;

pub fn encode(keystroke: &Keystroke, mode: TermMode) -> Option<Vec<u8>> {
    let m = &keystroke.modifiers;
    let app_cursor = mode.contains(TermMode::APP_CURSOR);
    let arrow = |code: char| -> Vec<u8> {
        if app_cursor {
            format!("\x1bO{code}").into_bytes()
        } else {
            format!("\x1b[{code}").into_bytes()
        }
    };

    let bytes = match keystroke.key.as_str() {
        "enter" if m.shift => b"\n".to_vec(),
        "enter" => b"\r".to_vec(),
        "tab" if m.shift => b"\x1b[Z".to_vec(),
        "tab" => b"\t".to_vec(),
        "backspace" if m.alt => b"\x1b\x7f".to_vec(),
        "backspace" => b"\x7f".to_vec(),
        "delete" => b"\x1b[3~".to_vec(),
        "escape" => b"\x1b".to_vec(),
        "up" => arrow('A'),
        "down" => arrow('B'),
        "right" => arrow('C'),
        "left" => arrow('D'),
        "home" => b"\x1b[H".to_vec(),
        "end" => b"\x1b[F".to_vec(),
        "pageup" => b"\x1b[5~".to_vec(),
        "pagedown" => b"\x1b[6~".to_vec(),
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
