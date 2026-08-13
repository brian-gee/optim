# optim

Minimal, fast, native app launcher for Windows. No runtime, no webview,
no plugins — a single exe that idles around 11 MB.

- **Alt+D** → instant popup
- Type to fuzzy-search every installed app (classic and Store/MSIX alike)
- Math in the box (`2+2*4`, trailing `=` ok) → result row, **Enter** copies it
- **↑/↓** navigate, **Enter** launches, **Ctrl+Shift+Enter** launches elevated
  (also works for `>` commands), **Esc** hides
- Launch counts boost your frequent apps
- Built-in commands ride the same search, prefixed so they stand out:
  `optim: Open Config`, `optim: Reload Config`, `optim: Refresh Apps`, `optim: Quit`
- System commands: `Restart`, `Shut Down`, `Sleep`, `Lock`, `Sign Out`
- `>ipconfig` runs a shell command in Windows Terminal (falls back to cmd)
- Paste a video URL → **watch in mpv**: the file downloads to `%TEMP%\watchqueue`
  in the background (scrubbable, survives expiring links; pruned after 24 h)
  and appends to one shared mpv window as a playlist "tab". Streaming sites
  fall back to direct playback via yt-dlp. Tab keybinds come from an mpv
  user script (TAB menu, Ctrl+TAB cycle, `D` detach).
- Ships with [Iosevka](https://typeof.net/Iosevka/) (SIL OFL) as the default
  font — or set any installed family via `font =`
- **Game mode**: won't pop over a focused fullscreen app (`game_mode = auto`,
  per-monitor aware). `optim: Game Mode` (or the tray menu) forces blocking
  everywhere. Pressing the hotkey 3× within 2s always opens the window — and
  switches forced mode back off.
- Ctrl+A selects the query; typing replaces it
- Index refreshes itself when apps are installed or removed — it never goes stale
- Tray icon → Open Config / Reload Config / Refresh Apps / Game Mode / Exit

## Themes

Dark only — light mode does not exist here, by design. Set `theme =` in the
config to one of:

`optim` `dracula` `one-dark` `tokyo-night` `catppuccin-mocha` `gruvbox`
`nord` `monokai` `solarized-dark` `github-dark` `ayu-dark`

Individual `bg/fg/dim/sel` keys override the theme's colors if you want to
tweak one.

## Config

`%APPDATA%\optim\config.ini` — theme, color overrides (hex), font family,
font size, hotkey, window width, max rows, editor. Edit, save, type `reload`
in optim. The file is created with commented defaults on first run.

Note: the default Alt+D hotkey shadows the browser "focus address bar"
shortcut system-wide. Rebind in the config (e.g. `hotkey = alt+space`) if
you'd rather keep it.

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
