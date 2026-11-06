//! Keyboard → terminal byte encoding (xterm-compatible) and paste encoding.

use iced::keyboard::key::Named;
use iced::keyboard::{Key, Modifiers};

/// xterm modifier parameter: 1 + shift(1) + alt(2) + ctrl(4).
fn mod_param(m: Modifiers) -> u8 {
    1 + u8::from(m.shift()) + 2 * u8::from(m.alt()) + 4 * u8::from(m.control())
}

fn has_mods(m: Modifiers) -> bool {
    m.shift() || m.alt() || m.control()
}

/// CSI/SS3 cursor-style key: `final_byte` with optional modifiers.
fn cursor_key(final_byte: u8, m: Modifiers, app_cursor: bool) -> Vec<u8> {
    if has_mods(m) {
        format!("\x1b[1;{}{}", mod_param(m), final_byte as char).into_bytes()
    } else if app_cursor {
        vec![0x1b, b'O', final_byte]
    } else {
        vec![0x1b, b'[', final_byte]
    }
}

/// `ESC [ n ~` style key with optional modifiers.
fn tilde_key(n: u8, m: Modifiers) -> Vec<u8> {
    if has_mods(m) {
        format!("\x1b[{n};{}~", mod_param(m)).into_bytes()
    } else {
        format!("\x1b[{n}~").into_bytes()
    }
}

fn function_key(n: u8, m: Modifiers) -> Option<Vec<u8>> {
    Some(match n {
        1..=4 => {
            let f = b'P' + (n - 1);
            if has_mods(m) {
                format!("\x1b[1;{}{}", mod_param(m), f as char).into_bytes()
            } else {
                vec![0x1b, b'O', f]
            }
        }
        5 => tilde_key(15, m),
        6 => tilde_key(17, m),
        7 => tilde_key(18, m),
        8 => tilde_key(19, m),
        9 => tilde_key(20, m),
        10 => tilde_key(21, m),
        11 => tilde_key(23, m),
        12 => tilde_key(24, m),
        _ => return None,
    })
}

fn with_alt(mut bytes: Vec<u8>, m: Modifiers) -> Vec<u8> {
    if m.alt() {
        bytes.insert(0, 0x1b);
    }
    bytes
}

/// Control byte for `Ctrl+<ch>`, if one exists.
fn ctrl_byte(ch: char) -> Option<u8> {
    match ch {
        'a'..='z' => Some(ch as u8 - b'a' + 1),
        'A'..='Z' => Some(ch as u8 - b'A' + 1),
        '@' | ' ' | '2' => Some(0),
        '[' | '3' => Some(0x1b),
        '\\' | '4' => Some(0x1c),
        ']' | '5' => Some(0x1d),
        '^' | '6' => Some(0x1e),
        '_' | '7' | '/' => Some(0x1f),
        '?' | '8' => Some(0x7f),
        _ => None,
    }
}

/// Encode a key press for the remote. `None` means "send nothing" (bare modifiers, unmapped keys).
///
/// `text` is the text the OS produced for the press (already shift/layout-aware).
pub fn encode_key(
    key: &Key,
    mods: Modifiers,
    text: Option<&str>,
    app_cursor: bool,
) -> Option<Vec<u8>> {
    if mods.logo() {
        return None;
    }
    match key.as_ref() {
        Key::Named(n) => encode_named(n, mods, text, app_cursor),
        Key::Character(c) => {
            if mods.control() {
                let ch = c.chars().next()?;
                return ctrl_byte(ch).map(|b| with_alt(vec![b], mods));
            }
            let s = text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control));
            let s = s.unwrap_or(c);
            Some(with_alt(s.as_bytes().to_vec(), mods))
        }
        Key::Unidentified => None,
    }
}

fn encode_named(n: Named, m: Modifiers, text: Option<&str>, app_cursor: bool) -> Option<Vec<u8>> {
    let bytes = match n {
        Named::Enter => vec![b'\r'],
        Named::Tab if m.shift() => b"\x1b[Z".to_vec(),
        Named::Tab => vec![b'\t'],
        Named::Backspace if m.control() => vec![0x08],
        Named::Backspace => vec![0x7f],
        Named::Escape => vec![0x1b],
        Named::Space if m.control() => vec![0],
        Named::Space => b" ".to_vec(),
        Named::ArrowUp => cursor_key(b'A', m, app_cursor),
        Named::ArrowDown => cursor_key(b'B', m, app_cursor),
        Named::ArrowRight => cursor_key(b'C', m, app_cursor),
        Named::ArrowLeft => cursor_key(b'D', m, app_cursor),
        Named::Home => cursor_key(b'H', m, app_cursor),
        Named::End => cursor_key(b'F', m, app_cursor),
        Named::Insert => tilde_key(2, m),
        Named::Delete => tilde_key(3, m),
        Named::PageUp => tilde_key(5, m),
        Named::PageDown => tilde_key(6, m),
        Named::F1 => function_key(1, m)?,
        Named::F2 => function_key(2, m)?,
        Named::F3 => function_key(3, m)?,
        Named::F4 => function_key(4, m)?,
        Named::F5 => function_key(5, m)?,
        Named::F6 => function_key(6, m)?,
        Named::F7 => function_key(7, m)?,
        Named::F8 => function_key(8, m)?,
        Named::F9 => function_key(9, m)?,
        Named::F10 => function_key(10, m)?,
        Named::F11 => function_key(11, m)?,
        Named::F12 => function_key(12, m)?,
        _ => {
            // Anything else only matters if it produced text.
            let t = text.filter(|t| !t.is_empty() && !t.chars().any(char::is_control))?;
            t.as_bytes().to_vec()
        }
    };
    // Modified sequences already carry Alt in their parameter; plain bytes get an ESC prefix.
    let is_csi = bytes.first() == Some(&0x1b) && bytes.len() > 1;
    Some(if is_csi { bytes } else { with_alt(bytes, m) })
}

/// Encode pasted text. Bracketed paste wraps it (stripping ESC so the payload cannot terminate the
/// bracket early); otherwise newlines become carriage returns like typed Enter.
pub fn encode_paste(text: &str, bracketed: bool) -> Vec<u8> {
    if bracketed {
        let clean: String = text.chars().filter(|&c| c != '\x1b').collect();
        let mut out = b"\x1b[200~".to_vec();
        out.extend_from_slice(clean.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        text.replace("\r\n", "\r").replace('\n', "\r").into_bytes()
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    fn ch(c: &str) -> Key {
        Key::Character(c.into())
    }
    fn enc(key: Key, mods: Modifiers, text: Option<&str>) -> Vec<u8> {
        encode_key(&key, mods, text, false).unwrap()
    }

    #[test]
    fn printable_text() {
        assert_eq!(enc(ch("a"), Modifiers::empty(), Some("a")), b"a");
        assert_eq!(enc(ch("a"), Modifiers::SHIFT, Some("A")), b"A");
        assert_eq!(enc(ch("é"), Modifiers::empty(), Some("é")), "é".as_bytes());
        assert_eq!(
            enc(Key::Named(Named::Space), Modifiers::empty(), Some(" ")),
            b" "
        );
    }

    #[test]
    fn ctrl_letters() {
        assert_eq!(enc(ch("c"), Modifiers::CTRL, None), [3]);
        assert_eq!(enc(ch("a"), Modifiers::CTRL, Some("\u{1}")), [1]);
        assert_eq!(enc(ch("z"), Modifiers::CTRL, None), [26]);
        assert_eq!(enc(ch("["), Modifiers::CTRL, None), [0x1b]);
        assert_eq!(enc(Key::Named(Named::Space), Modifiers::CTRL, None), [0]);
    }

    #[test]
    fn alt_is_esc_prefix() {
        assert_eq!(enc(ch("b"), Modifiers::ALT, Some("b")), b"\x1bb");
        assert_eq!(
            enc(ch("f"), Modifiers::ALT | Modifiers::CTRL, None),
            [0x1b, 6]
        );
        assert_eq!(
            enc(Key::Named(Named::Backspace), Modifiers::ALT, None),
            [0x1b, 0x7f]
        );
    }

    #[test]
    fn basic_named_keys() {
        let n = Modifiers::empty();
        assert_eq!(enc(Key::Named(Named::Enter), n, None), b"\r");
        assert_eq!(enc(Key::Named(Named::Backspace), n, None), [0x7f]);
        assert_eq!(enc(Key::Named(Named::Tab), n, None), b"\t");
        assert_eq!(
            enc(Key::Named(Named::Tab), Modifiers::SHIFT, None),
            b"\x1b[Z"
        );
        assert_eq!(enc(Key::Named(Named::Escape), n, None), [0x1b]);
        assert_eq!(enc(Key::Named(Named::Delete), n, None), b"\x1b[3~");
        assert_eq!(enc(Key::Named(Named::PageUp), n, None), b"\x1b[5~");
        assert_eq!(enc(Key::Named(Named::PageDown), n, None), b"\x1b[6~");
        assert_eq!(enc(Key::Named(Named::Insert), n, None), b"\x1b[2~");
    }

    #[test]
    fn arrows_respect_app_cursor() {
        let up = Key::Named(Named::ArrowUp);
        let n = Modifiers::empty();
        assert_eq!(encode_key(&up, n, None, false).unwrap(), b"\x1b[A");
        assert_eq!(encode_key(&up, n, None, true).unwrap(), b"\x1bOA");
        assert_eq!(
            encode_key(&Key::Named(Named::Home), n, None, true).unwrap(),
            b"\x1bOH"
        );
        assert_eq!(
            encode_key(&Key::Named(Named::End), n, None, false).unwrap(),
            b"\x1b[F"
        );
        // Modified arrows always use the CSI 1;m form.
        assert_eq!(
            encode_key(&up, Modifiers::CTRL, None, true).unwrap(),
            b"\x1b[1;5A"
        );
        assert_eq!(
            encode_key(&up, Modifiers::SHIFT, None, false).unwrap(),
            b"\x1b[1;2A"
        );
        assert_eq!(
            encode_key(&Key::Named(Named::ArrowLeft), Modifiers::ALT, None, false).unwrap(),
            b"\x1b[1;3D"
        );
    }

    #[test]
    fn function_keys() {
        let n = Modifiers::empty();
        assert_eq!(enc(Key::Named(Named::F1), n, None), b"\x1bOP");
        assert_eq!(enc(Key::Named(Named::F4), n, None), b"\x1bOS");
        assert_eq!(enc(Key::Named(Named::F5), n, None), b"\x1b[15~");
        assert_eq!(enc(Key::Named(Named::F10), n, None), b"\x1b[21~");
        assert_eq!(enc(Key::Named(Named::F12), n, None), b"\x1b[24~");
        assert_eq!(
            enc(Key::Named(Named::F5), Modifiers::CTRL, None),
            b"\x1b[15;5~"
        );
        assert_eq!(
            enc(Key::Named(Named::F1), Modifiers::SHIFT, None),
            b"\x1b[1;2P"
        );
    }

    #[test]
    fn unmapped_and_logo_send_nothing() {
        assert!(encode_key(&Key::Named(Named::Shift), Modifiers::SHIFT, None, false).is_none());
        assert!(encode_key(&Key::Unidentified, Modifiers::empty(), None, false).is_none());
        assert!(encode_key(&ch("a"), Modifiers::LOGO, Some("a"), false).is_none());
    }

    #[test]
    fn paste_encoding() {
        assert_eq!(encode_paste("a\nb", false), b"a\rb");
        assert_eq!(encode_paste("a\r\nb", false), b"a\rb");
        assert_eq!(encode_paste("x", true), b"\x1b[200~x\x1b[201~");
        assert_eq!(
            encode_paste("a\x1b[201~b", true),
            b"\x1b[200~a[201~b\x1b[201~"
        );
    }
}
