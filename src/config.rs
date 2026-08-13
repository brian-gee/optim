use std::path::PathBuf;

pub struct Config {
    pub bg: u32,
    pub fg: u32,
    pub dim: u32,
    pub sel: u32,
    pub font: String,
    pub hotkey_mods: u32, // MOD_* bits
    pub hotkey_vk: u32,
    pub width: f32,
    pub max_rows: usize,
    /// Editor executable for "Open Config"; empty = system default for .ini.
    pub editor: String,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            bg: 0x1B1B1D,
            fg: 0xECECEE,
            dim: 0x77777C,
            sel: 0x2C2C31,
            font: "Segoe UI Variable Text".into(),
            hotkey_mods: 0x0001, // MOD_ALT
            hotkey_vk: 0x20,     // VK_SPACE
            width: 640.0,
            max_rows: 8,
            editor: String::new(),
        }
    }
}

const DEFAULT_FILE: &str = "\
# optim configuration
# edit, save, then tray icon -> Reload Config

# colors (hex RRGGBB)
bg  = 1B1B1D   # window background
fg  = ECECEE   # primary text
dim = 77777C   # placeholder text
sel = 2C2C31   # selection pill

# font family
font = Segoe UI Variable Text

# global hotkey: ctrl/alt/shift/win + one of a-z, 0-9, space, f1-f12
# at least one modifier is required
hotkey = alt+space

# window width (logical px) and max visible result rows
width = 640
max_rows = 8

# editor for the Open Config command; empty = system default for .ini
# e.g.  editor = C:\\Program Files\\Sublime Text\\sublime_text.exe
editor =
";

pub fn path() -> PathBuf {
    PathBuf::from(std::env::var("APPDATA").unwrap_or_default())
        .join("optim")
        .join("config.ini")
}

/// Loads config, writing the commented default file on first run.
pub fn load() -> Config {
    let p = path();
    if !p.exists() {
        if let Some(dir) = p.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&p, DEFAULT_FILE);
    }
    let mut c = Config::default();
    let Ok(text) = std::fs::read_to_string(&p) else {
        return c;
    };
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        let Some((k, v)) = line.split_once('=') else {
            continue;
        };
        let (k, v) = (k.trim().to_lowercase(), v.trim());
        match k.as_str() {
            "bg" => set_hex(&mut c.bg, v),
            "fg" => set_hex(&mut c.fg, v),
            "dim" => set_hex(&mut c.dim, v),
            "sel" => set_hex(&mut c.sel, v),
            "font" if !v.is_empty() => c.font = v.to_string(),
            "editor" => c.editor = v.trim_matches('"').to_string(),
            "hotkey" => {
                if let Some((m, vk)) = parse_hotkey(v) {
                    c.hotkey_mods = m;
                    c.hotkey_vk = vk;
                }
            }
            "width" => {
                if let Ok(w) = v.parse::<f32>() {
                    c.width = w.clamp(400.0, 1600.0);
                }
            }
            "max_rows" => {
                if let Ok(n) = v.parse::<usize>() {
                    c.max_rows = n.clamp(1, 16);
                }
            }
            _ => {}
        }
    }
    c
}

fn set_hex(target: &mut u32, v: &str) {
    if v.len() == 6 {
        if let Ok(x) = u32::from_str_radix(v, 16) {
            *target = x;
        }
    }
}

fn parse_hotkey(v: &str) -> Option<(u32, u32)> {
    let v = v.to_lowercase();
    let parts: Vec<&str> = v.split('+').map(str::trim).collect();
    let (key, mods_parts) = parts.split_last()?;
    let mut mods = 0u32;
    for m in mods_parts {
        mods |= match *m {
            "alt" => 0x0001,
            "ctrl" | "control" => 0x0002,
            "shift" => 0x0004,
            "win" => 0x0008,
            _ => return None,
        };
    }
    if mods == 0 {
        return None; // unmodified keys would hijack normal typing
    }
    let vk = match *key {
        "space" => 0x20,
        k if k.len() == 1 && k.chars().next().unwrap().is_ascii_alphanumeric() => {
            k.to_uppercase().bytes().next().unwrap() as u32
        }
        k if k.starts_with('f') => {
            let n: u32 = k[1..].parse().ok()?;
            if (1..=12).contains(&n) {
                0x70 + n - 1
            } else {
                return None;
            }
        }
        _ => return None,
    };
    Some((mods, vk))
}

#[cfg(test)]
mod tests {
    use super::parse_hotkey;

    #[test]
    fn hotkeys() {
        assert_eq!(parse_hotkey("alt+space"), Some((1, 0x20)));
        assert_eq!(parse_hotkey("ctrl+alt+k"), Some((3, b'K' as u32)));
        assert_eq!(parse_hotkey("win+f2"), Some((8, 0x71)));
        assert_eq!(parse_hotkey("space"), None); // no modifier
        assert_eq!(parse_hotkey("alt+escape"), None);
    }
}
