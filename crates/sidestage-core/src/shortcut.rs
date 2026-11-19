//! Keyboard shortcut strings ("Ctrl+Alt+Space") and their platform encodings.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Key {
    /// `a`–`z` or `0`–`9` (stored lower-case).
    Char(char),
    Space,
    Tab,
    Return,
    Escape,
    /// F1–F24.
    F(u8),
    Slash,
    Period,
    Comma,
    Minus,
    Equal,
    Semicolon,
    Grave,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    /// Super / Windows / Meta key.
    pub meta: bool,
    pub key: Key,
}

impl Shortcut {
    /// Parses strings like `Ctrl+Alt+Space`, `Meta+S`, `super+shift+f`.
    /// Requires a modifier unless the key is a function key.
    pub fn parse(text: &str) -> Result<Shortcut, String> {
        let mut s = Shortcut {
            ctrl: false,
            alt: false,
            shift: false,
            meta: false,
            key: Key::Space,
        };
        let mut key = None;
        let parts: Vec<&str> = text.split('+').map(str::trim).collect();
        if parts.iter().any(|p| p.is_empty()) {
            return Err("Use the form Ctrl+Alt+Space".into());
        }
        for part in parts {
            let lower = part.to_ascii_lowercase();
            match lower.as_str() {
                "ctrl" | "control" => s.ctrl = true,
                "alt" | "option" => s.alt = true,
                "shift" => s.shift = true,
                "meta" | "super" | "win" | "windows" | "cmd" | "logo" => s.meta = true,
                _ => {
                    if key.is_some() {
                        return Err(format!(
                            "Only one key besides modifiers is allowed (got “{part}”)"
                        ));
                    }
                    key = Some(parse_key(&lower).ok_or_else(|| format!("Unknown key “{part}”"))?);
                }
            }
        }
        s.key = key.ok_or("Add a key, e.g. Ctrl+Alt+Space")?;
        if !(s.ctrl || s.alt || s.meta || matches!(s.key, Key::F(_))) {
            return Err("Add Ctrl, Alt or Meta so normal typing isn't captured".into());
        }
        Ok(s)
    }

    /// Canonical text, e.g. `Ctrl+Alt+Space`.
    pub fn display(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.meta {
            parts.push("Meta".into());
        }
        if self.ctrl {
            parts.push("Ctrl".into());
        }
        if self.alt {
            parts.push("Alt".into());
        }
        if self.shift {
            parts.push("Shift".into());
        }
        parts.push(match self.key {
            Key::Char(c) => c.to_ascii_uppercase().to_string(),
            Key::Space => "Space".into(),
            Key::Tab => "Tab".into(),
            Key::Return => "Return".into(),
            Key::Escape => "Esc".into(),
            Key::F(n) => format!("F{n}"),
            Key::Slash => "/".into(),
            Key::Period => ".".into(),
            Key::Comma => ",".into(),
            Key::Minus => "-".into(),
            Key::Equal => "=".into(),
            Key::Semicolon => ";".into(),
            Key::Grave => "`".into(),
        });
        parts.join("+")
    }

    /// Qt key code with modifier flags (KDE KGlobalAccel).
    pub fn to_qt(&self) -> i32 {
        let mut k: i32 = match self.key {
            Key::Char(c) => c.to_ascii_uppercase() as i32,
            Key::Space => 0x20,
            Key::Tab => 0x0100_0001,
            Key::Return => 0x0100_0004,
            Key::Escape => 0x0100_0000,
            Key::F(n) => 0x0100_0030 + n as i32 - 1,
            Key::Slash => 0x2f,
            Key::Period => 0x2e,
            Key::Comma => 0x2c,
            Key::Minus => 0x2d,
            Key::Equal => 0x3d,
            Key::Semicolon => 0x3b,
            Key::Grave => 0x60,
        };
        if self.shift {
            k |= 0x0200_0000;
        }
        if self.ctrl {
            k |= 0x0400_0000;
        }
        if self.alt {
            k |= 0x0800_0000;
        }
        if self.meta {
            k |= 0x1000_0000;
        }
        k
    }

    /// GTK/Mutter accelerator string, e.g. `<Control><Alt>space`.
    pub fn to_gnome(&self) -> String {
        let mut s = String::new();
        if self.meta {
            s.push_str("<Super>");
        }
        if self.ctrl {
            s.push_str("<Control>");
        }
        if self.alt {
            s.push_str("<Alt>");
        }
        if self.shift {
            s.push_str("<Shift>");
        }
        s.push_str(&match self.key {
            Key::Char(c) => c.to_string(),
            Key::Space => "space".into(),
            Key::Tab => "Tab".into(),
            Key::Return => "Return".into(),
            Key::Escape => "Escape".into(),
            Key::F(n) => format!("F{n}"),
            Key::Slash => "slash".into(),
            Key::Period => "period".into(),
            Key::Comma => "comma".into(),
            Key::Minus => "minus".into(),
            Key::Equal => "equal".into(),
            Key::Semicolon => "semicolon".into(),
            Key::Grave => "grave".into(),
        });
        s
    }

    /// Win32 `RegisterHotKey` modifiers and virtual-key code.
    pub fn to_win32(&self) -> (u32, u32) {
        const MOD_ALT: u32 = 1;
        const MOD_CONTROL: u32 = 2;
        const MOD_SHIFT: u32 = 4;
        const MOD_WIN: u32 = 8;
        let mut m = 0;
        if self.alt {
            m |= MOD_ALT;
        }
        if self.ctrl {
            m |= MOD_CONTROL;
        }
        if self.shift {
            m |= MOD_SHIFT;
        }
        if self.meta {
            m |= MOD_WIN;
        }
        let vk = match self.key {
            Key::Char(c) => c.to_ascii_uppercase() as u32,
            Key::Space => 0x20,
            Key::Tab => 0x09,
            Key::Return => 0x0D,
            Key::Escape => 0x1B,
            Key::F(n) => 0x70 + n as u32 - 1,
            Key::Slash => 0xBF,
            Key::Period => 0xBE,
            Key::Comma => 0xBC,
            Key::Minus => 0xBD,
            Key::Equal => 0xBB,
            Key::Semicolon => 0xBA,
            Key::Grave => 0xC0,
        };
        (m, vk)
    }

    /// X11 modifier mask (Shift=1, Control=4, Mod1/Alt=8, Mod4/Super=64)
    /// and keysym.
    pub fn to_x11(&self) -> (u16, u32) {
        let mut m = 0u16;
        if self.shift {
            m |= 1;
        }
        if self.ctrl {
            m |= 4;
        }
        if self.alt {
            m |= 8;
        }
        if self.meta {
            m |= 64;
        }
        let sym = match self.key {
            Key::Char(c) => c as u32,
            Key::Space => 0x20,
            Key::Tab => 0xff09,
            Key::Return => 0xff0d,
            Key::Escape => 0xff1b,
            Key::F(n) => 0xffbe + n as u32 - 1,
            Key::Slash => 0x2f,
            Key::Period => 0x2e,
            Key::Comma => 0x2c,
            Key::Minus => 0x2d,
            Key::Equal => 0x3d,
            Key::Semicolon => 0x3b,
            Key::Grave => 0x60,
        };
        (m, sym)
    }
}

fn parse_key(k: &str) -> Option<Key> {
    Some(match k {
        "space" => Key::Space,
        "tab" => Key::Tab,
        "return" | "enter" => Key::Return,
        "esc" | "escape" => Key::Escape,
        "/" | "slash" => Key::Slash,
        "." | "period" => Key::Period,
        "," | "comma" => Key::Comma,
        "-" | "minus" => Key::Minus,
        "=" | "equal" => Key::Equal,
        ";" | "semicolon" => Key::Semicolon,
        "`" | "grave" => Key::Grave,
        _ if k.len() == 1 && k.chars().all(|c| c.is_ascii_alphanumeric()) => {
            Key::Char(k.chars().next()?)
        }
        _ if k.starts_with('f') => {
            let n: u8 = k[1..].parse().ok()?;
            if (1..=24).contains(&n) {
                Key::F(n)
            } else {
                return None;
            }
        }
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_display() {
        let s = Shortcut::parse("ctrl + alt+ space").unwrap();
        assert_eq!(s.display(), "Ctrl+Alt+Space");
        assert_eq!(
            Shortcut::parse("Super+Shift+f").unwrap().display(),
            "Meta+Shift+F"
        );
        assert_eq!(Shortcut::parse("F12").unwrap().display(), "F12");
        assert!(
            Shortcut::parse("Shift+A").is_err(),
            "plain typing must not be grabbed"
        );
        assert!(Shortcut::parse("Ctrl+Alt").is_err());
        assert!(Shortcut::parse("Ctrl+A+B").is_err());
        assert!(Shortcut::parse("Ctrl+Hyper").is_err());
        assert!(Shortcut::parse("Ctrl++").is_err());
        assert!(Shortcut::parse("F30").is_err());
    }

    #[test]
    fn platform_encodings() {
        let s = Shortcut::parse("Ctrl+Alt+Space").unwrap();
        assert_eq!(s.to_qt(), 0x0400_0000 | 0x0800_0000 | 0x20);
        assert_eq!(s.to_gnome(), "<Control><Alt>space");
        assert_eq!(s.to_win32(), (2 | 1, 0x20));
        assert_eq!(s.to_x11(), (4 | 8, 0x20));
        let m = Shortcut::parse("Meta+S").unwrap();
        assert_eq!(m.to_qt(), 0x1000_0000 | 0x53);
        assert_eq!(m.to_gnome(), "<Super>s");
        assert_eq!(m.to_win32(), (8, 0x53));
        assert_eq!(m.to_x11(), (64, 's' as u32));
        let f = Shortcut::parse("Shift+F3").unwrap();
        assert_eq!(f.to_qt(), 0x0200_0000 | 0x0100_0032);
        assert_eq!(f.to_win32().1, 0x72);
        assert_eq!(f.to_x11().1, 0xffc0);
    }
}
