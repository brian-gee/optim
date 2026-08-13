# optim

Minimal, fast, native app launcher for Windows. No runtime, no webview,
no plugins — a single ~300 KB exe that idles around 11 MB.

- **Alt+Space** → instant popup
- Type to fuzzy-search every installed app (classic and Store/MSIX alike)
- Math in the box (`2+2*4`) → result row, **Enter** copies it
- **↑/↓** navigate, **Enter** launches, **Esc** hides
- Launch counts boost your frequent apps
- Index refreshes itself when apps are installed or removed — it never goes stale
- Tray icon → Open Config / Reload Config / Refresh Apps / Exit

## Config

`%APPDATA%\optim\config.ini` — colors (hex), font family, hotkey, window
width, max rows. Edit, save, tray → Reload Config. The file is created
with commented defaults on first run.

## Build

```
cargo build --release
```

Requires the MSVC toolchain. The only dependency is the official
[`windows`](https://crates.io/crates/windows) crate.

## Autostart

```
optim.exe --install-autostart    # HKCU Run entry pointing at this exe
optim.exe --uninstall-autostart
```

## Known limitations

- MSIX-only changes (no Start Menu shortcut) are picked up by the 5-minute
  popup-time refresh or tray → Refresh Apps, not instantly.
- Astral-plane characters (emoji) in queries aren't handled yet.
