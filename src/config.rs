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
    /// Result-row font size in logical px; the input line scales with it.
    pub font_size: f32,
    /// Auto game mode: don't pop over fullscreen apps (triple-press overrides).
    pub game_mode_auto: bool,
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
            font: "iosevka".into(),
            hotkey_mods: 0x0001, // MOD_ALT
            hotkey_vk: 0x44,     // 'D'
            width: 600.0,
            max_rows: 8,
            font_size: 15.0,
            game_mode_auto: true,
            editor: String::new(),
        }
    }
}

/// Built-in dark themes: (name, bg, fg, dim, sel). Dark only, by design.
pub const THEMES: [(&str, u32, u32, u32, u32); 11] = [
    ("optim", 0x1B1B1D, 0xECECEE, 0x77777C, 0x2C2C31),
    ("dracula", 0x282A36, 0xF8F8F2, 0x6272A4, 0x44475A),
    ("one-dark", 0x282C34, 0xABB2BF, 0x5C6370, 0x3E4451),
    ("tokyo-night", 0x1A1B26, 0xC0CAF5, 0x565F89, 0x292E42),
    ("catppuccin-mocha", 0x1E1E2E, 0xCDD6F4, 0x6C7086, 0x313244),
    ("gruvbox", 0x282828, 0xEBDBB2, 0x928374, 0x3C3836),
    ("nord", 0x2E3440, 0xD8DEE9, 0x4C566A, 0x3B4252),
    ("monokai", 0x272822, 0xF8F8F2, 0x75715E, 0x49483E),
    ("solarized-dark", 0x002B36, 0x839496, 0x586E75, 0x073642),
    ("github-dark", 0x0D1117, 0xC9D1D9, 0x8B949E, 0x21262D),
    ("ayu-dark", 0x0A0E14, 0xB3B1AD, 0x4D5566, 0x1F2430),
];

const DEFAULT_FILE: &str = "\
# optim configuration
# edit, save, then \"optim: Reload Config\" (or the tray menu)

# built-in theme, one of:
#   optim  dracula  one-dark  tokyo-night  catppuccin-mocha  gruvbox
#   nord  monokai  solarized-dark  github-dark  ayu-dark
theme = optim

# individual color overrides (hex RRGGBB) — uncomment to override the theme
# bg  = 1B1B1D   # window background
# fg  = ECECEE   # primary text
# dim = 77777C   # placeholder text
# sel = 2C2C31   # selection pill

# font family: 'iosevka' (bundled) or any installed font family name
font = iosevka

# global hotkey: ctrl/alt/shift/win + one of a-z, 0-9, space, f1-f12
# at least one modifier is required
hotkey = alt+d

# window width (logical px) and max visible result rows
width = 600
max_rows = 8

# result-row font size (logical px); input line and row heights scale with it
font_size = 15

# game mode: 'auto' blocks the popup while a fullscreen app has focus
# (press the hotkey 3x within 2s to force it open); 'off' disables the check.
# The forced toggle ('optim: Game Mode' command / tray menu) blocks everywhere.
game_mode = auto

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
    // Pass 1: apply the theme palette, so explicit color keys can override it.
    for line in text.lines() {
        let line = line.split('#').next().unwrap_or("");
        if let Some((k, v)) = line.split_once('=') {
            if k.trim().eq_ignore_ascii_case("theme") {
                let name = v.trim().to_lowercase();
                if let Some(&(_, bg, fg, dim, sel)) =
                    THEMES.iter().find(|(n, ..)| *n == name)
                {
                    (c.bg, c.fg, c.dim, c.sel) = (bg, fg, dim, sel);
                }
            }
        }
    }
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
            "font_size" => {
                if let Ok(s) = v.parse::<f32>() {
                    c.font_size = s.clamp(8.0, 32.0);
                }
            }
            "game_mode" => c.game_mode_auto = !v.eq_ignore_ascii_case("off"),
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
    use super::{parse_hotkey, THEMES};

    #[test]
    fn hotkeys() {
        assert_eq!(parse_hotkey("alt+space"), Some((1, 0x20)));
        assert_eq!(parse_hotkey("ctrl+alt+k"), Some((3, b'K' as u32)));
        assert_eq!(parse_hotkey("win+f2"), Some((8, 0x71)));
        assert_eq!(parse_hotkey("space"), None); // no modifier
        assert_eq!(parse_hotkey("alt+escape"), None);
    }

    #[test]
    fn theme_table_sane() {
        assert_eq!(THEMES.len(), 11);
        assert!(THEMES.iter().any(|(n, ..)| *n == "dracula"));
        // every theme is dark: background luminance below foreground's
        for (name, bg, fg, ..) in THEMES {
            let lum = |v: u32| (v >> 16 & 0xFF) + (v >> 8 & 0xFF) + (v & 0xFF);
            assert!(lum(bg) < lum(fg), "{name} is not dark");
        }
    }
}
