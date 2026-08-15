# optim

Minimal, fast, native app launcher for Windows. No runtime, no webview,
no plugins — a single exe that idles around 11 MB.

- **Alt+D** → instant popup
- Type to fuzzy-search every installed app (classic and Store/MSIX alike)
- Math in the box (`2+2*4`, trailing `=` ok) → result row, **Enter** copies it
- **↑/↓** navigate, **Enter** launches, **Ctrl+Shift+Enter** launches elevated
  (also works for `>` commands), **Esc** hides
- **Hide an app from the list** — **Ctrl+H** on the selected row, or
  **Shift+Delete**, the same reflex as a browser address bar. Stops apps you
  never launch from being suggested. Hidden apps stay hidden across restarts, kept in
  `%LOCALAPPDATA%\optim\hidden.tsv` and keyed on the shell's parsing name so a
  rename can't quietly un-hide one. Bring them back from the tray icon's
  *Hidden Apps* submenu (click one to unhide) or with `optim: Unhide All Apps`.
- Launch counts boost your frequent apps
- Built-in commands ride the same search, prefixed so they stand out:
  `optim: Open Config`, `optim: Reload Config`, `optim: Refresh Apps`,
  `optim: Watch Queue`, `optim: Clear Watch Queue`, `optim: Watch Queue Folder`,
  `optim: Unhide All Apps`, `optim: Quit`
- **Service commands** for the background tools that have no tray icon and are
  otherwise only reachable from a terminal: `komorebi: Start` / `Stop` /
  `Restart` / `Reload Config` / `Toggle Pause` / `Retile`, and `whkd: Restart`
  for when only the keybinds changed. `Reload Config` applies komorebi's static
  config in place, so windows keep their workspaces; the config path comes from
  komorebi's own lookup (`KOMOREBI_CONFIG_HOME`, else the user profile) rather
  than anything baked in.
- System commands: `Restart`, `Shut Down`, `Sleep`, `Lock`, `Sign Out`
- `>ipconfig` runs a shell command in Windows Terminal (falls back to cmd)
- Paste a video URL → **watch in mpv**: the file downloads to `%TEMP%\watchqueue`
  in the background (scrubbable, survives expiring links) and appends to one
  shared mpv window as a playlist "tab" — playback starts as soon as ~1 MB has
  landed, while the rest downloads behind the scrubber. Paste as many as you
  like; three download at a time and the rest queue up. Streaming sites fall
  back to direct playback via yt-dlp.
- **The queue is persistent.** Every video is remembered in
  `%LOCALAPPDATA%\optim\watch-history.tsv` and replayed as tabs each time the
  player opens, so closing mpv or rebooting doesn't lose your place in the pile.
  Nothing expires on a timer or a size cap: a video leaves only when you say so,
  with `X`/`Del`/`Ctrl+W` on a tab or `optim: Clear Watch Queue` (which asks
  first, then empties the history and the cache directory together).
  `optim: Watch Queue` reopens the player with everything still in it.
- [`extras/mpv/tabs.lua`](extras/mpv/tabs.lua) (drop into `%APPDATA%\mpv\scripts`)
  provides the tab UI: TAB opens a menu with mouse hover/click, `X` or
  right-click forgets a tab, `C` twice forgets everything, Ctrl+TAB or
  Ctrl+wheel cycles, `D` detaches a tab to its own window. Forgetting shells
  back out to `optim --watch-forget`, so the history file has exactly one writer.
  The same keys work with the menu closed — `X`/`DEL` forget, `D`/`d` detach,
  `C` `C` forget all, `1`-`9` switch, `j`/`k` move — because otherwise they fall
  through to mpv's stock bindings and do something else entirely (`x` is
  subtitle delay, `d` deinterlacing, `1`-`8` the colour controls).
- Ships with [Iosevka](https://typeof.net/Iosevka/) (SIL OFL) as the default
  font — or set any installed family via `font =`
- **Game mode**: won't pop over a focused fullscreen app (`game_mode = auto`,
  per-monitor aware). `optim: Game Mode` (or the tray menu) forces blocking
  everywhere. Pressing the hotkey 3× within 2s always opens the window — and
  switches forced mode back off.
- Ctrl+A selects the query; typing replaces it
- Index refreshes itself when apps are installed or removed — it never goes stale
- Tray icon → Open Config / Reload Config / Refresh Apps / Game Mode /
  Hidden Apps / Exit

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
