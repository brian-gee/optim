//! Start, stop and reload the background tools that have no tray icon of
//! their own.
//!
//! A tiling window manager and its hotkey daemon are controlled entirely from
//! the command line, which leaves no way to restart them after a config edit
//! except opening a terminal. They are exactly the kind of thing a launcher
//! should reach — so they ride the same search as everything else.
//!
//! The status bar belongs here for a sharper reason. It hides explorer's
//! taskbar and hosts the tray itself, so the moment it exits there is nothing
//! left to click — its own Exit is the one thing guaranteed to be gone. A
//! launcher on a global hotkey is the natural place to reach it from.
//!
//! Nothing here hardcodes a machine: komorebi's own config lookup order
//! (`KOMOREBI_CONFIG_HOME`, else the user profile) decides which file gets
//! reloaded, the bar is located through the Run entry it registered for
//! itself, and every other executable is resolved from PATH.

/// One process to run. `wait` is false for anything that doesn't return —
/// waiting on the daemon itself would park the worker thread forever.
pub struct Step {
    exe: &'static str,
    args: &'static [&'static str],
    wait: bool,
}

const fn run(exe: &'static str, args: &'static [&'static str]) -> Step {
    Step { exe, args, wait: true }
}

const fn spawn(exe: &'static str, args: &'static [&'static str]) -> Step {
    Step { exe, args, wait: false }
}

/// Substituted at run time with komorebi's static config path — a literal
/// path in here would be one machine's, and this repo is public.
const CONFIG: &str = "{komorebi-config}";

const KOMOREBIC: &str = "komorebic.exe";

/// The status bar, resolved at run time by [`bar_exe`] — it lives wherever it
/// was built, which is not on PATH and not the same place on two machines.
const BAR: &str = "optim-bar.exe";

/// Steps run in order, so "restart" is simply stop-then-start.
pub const SERVICES: [&[Step]; 10] = [
    // 0: komorebi start (with its hotkey daemon)
    &[run(KOMOREBIC, &["start", "--whkd"])],
    // 1: komorebi stop
    &[run(KOMOREBIC, &["stop", "--whkd"])],
    // 2: komorebi restart
    &[
        run(KOMOREBIC, &["stop", "--whkd"]),
        run(KOMOREBIC, &["start", "--whkd"]),
    ],
    // 3: reload the static config in place — no restart, windows keep their
    //    workspaces.
    &[run(KOMOREBIC, &["replace-configuration", CONFIG])],
    // 4: pause/resume tiling
    &[run(KOMOREBIC, &["toggle-pause"])],
    // 5: re-apply the current layout
    &[run(KOMOREBIC, &["retile"])],
    // 6: whkd on its own, for when only the keybinds changed. komorebic has
    //    no whkd-only restart, and whkd never exits, so it is killed by name
    //    and re-spawned without waiting on it.
    &[
        run("taskkill.exe", &["/IM", "whkd.exe", "/F"]),
        spawn("whkd.exe", &[]),
    ],
    // 7: start the bar. A no-op if one is already running — the bar holds a
    //    single-instance mutex and a second copy exits on its own.
    &[spawn(BAR, &[])],
    // 8: restart the bar, the entry that matters after a rebuild. `--quit`
    //    blocks until the old process is gone, so the start below wins the
    //    mutex instead of quietly losing it. Waited on for the same reason.
    &[run(BAR, &["--quit"]), spawn(BAR, &[])],
    // 9: stop the bar and give explorer its shell back. The escape hatch for
    //    a bar that won't behave — without it the only route back is a
    //    terminal, on a desktop with no taskbar to open one from.
    &[run(BAR, &["--quit"]), run(BAR, &["--restore-tray"])],
];

/// komorebi's own lookup order for its static config.
fn komorebi_config() -> String {
    let dir = std::env::var("KOMOREBI_CONFIG_HOME")
        .or_else(|_| std::env::var("USERPROFILE"))
        .unwrap_or_default();
    if dir.is_empty() {
        "komorebi.json".into()
    } else {
        format!("{dir}\\komorebi.json")
    }
}

/// Where the bar lives, read from the `Run` entry it writes for itself.
///
/// That value is the bar's own `current_exe()` at the time it installed
/// autostart, so it is correct by construction and follows the build when it
/// moves — neither of which a path in this file could manage.
///
/// Falls back to the bare name, and so to a PATH lookup, for an install that
/// never set autostart up.
fn bar_exe() -> String {
    use windows::core::w;
    use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_SZ};
    let mut buf = [0u16; 512];
    let mut size = (buf.len() * 2) as u32;
    let rc = unsafe {
        RegGetValueW(
            HKEY_CURRENT_USER,
            w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run"),
            w!("optim-bar"),
            RRF_RT_REG_SZ,
            None,
            Some(buf.as_mut_ptr() as *mut std::ffi::c_void),
            Some(&mut size),
        )
    };
    if rc.is_err() {
        return BAR.to_string();
    }
    // `size` counts bytes and includes the terminator the API always writes.
    let len = (size as usize / 2).saturating_sub(1).min(buf.len());
    // Run entries are command lines, so a quoted path is legitimate even
    // though the bar registers a bare one.
    let path = String::from_utf16_lossy(&buf[..len])
        .trim()
        .trim_matches('"')
        .to_string();
    if path.is_empty() {
        BAR.to_string()
    } else {
        path
    }
}

/// Run one service's steps on a background thread. A step that fails doesn't
/// stop the rest: `stop` legitimately fails when nothing is running, and the
/// `start` after it is the part that matters.
pub fn start(index: usize) {
    let Some(steps) = SERVICES.get(index) else { return };
    std::thread::spawn(move || {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        for step in *steps {
            let args: Vec<String> = step
                .args
                .iter()
                .map(|a| {
                    if *a == CONFIG {
                        komorebi_config()
                    } else {
                        (*a).to_string()
                    }
                })
                .collect();
            let exe = if step.exe == BAR {
                bar_exe()
            } else {
                step.exe.to_string()
            };
            let child = std::process::Command::new(&exe)
                .args(&args)
                .creation_flags(CREATE_NO_WINDOW)
                .spawn();
            match child {
                Ok(mut c) if step.wait => {
                    let _ = c.wait();
                }
                _ => {}
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{komorebi_config, CONFIG, SERVICES};

    /// Every step must name an executable and, where a config path is needed,
    /// use the placeholder rather than a baked-in path.
    #[test]
    fn steps_are_well_formed() {
        for steps in SERVICES {
            assert!(!steps.is_empty());
            for step in steps {
                assert!(step.exe.ends_with(".exe"), "{}", step.exe);
                for arg in step.args {
                    assert!(
                        !arg.contains(":\\") || *arg == CONFIG,
                        "absolute path baked into a step: {arg}"
                    );
                }
            }
        }
    }

    /// Restarting is stop-then-start, in that order — the other way round
    /// would leave nothing running.
    #[test]
    fn restart_stops_before_it_starts() {
        let restart = SERVICES[2];
        assert_eq!(restart.len(), 2);
        assert_eq!(restart[0].args[0], "stop");
        assert_eq!(restart[1].args[0], "start");
    }

    /// whkd never returns, so the step that launches it must not be waited on.
    #[test]
    fn the_daemon_is_not_waited_for() {
        let whkd = SERVICES[6];
        assert!(whkd[0].wait, "the kill has to finish before the restart");
        assert!(!whkd.last().unwrap().wait);
    }

    /// Restarting the bar has to wait out the old process: it holds a
    /// single-instance mutex, so a start that overlaps it exits silently and
    /// leaves the desktop with no bar at all.
    #[test]
    fn the_bar_restart_waits_for_the_quit() {
        let restart = super::SERVICES[8];
        assert_eq!(restart.len(), 2);
        assert_eq!(restart[0].args, &["--quit"]);
        assert!(restart[0].wait, "the start would race the mutex");
        assert!(!restart[1].wait, "the bar never returns");
    }

    /// Stopping has to hand the shell back, or it leaves a desktop with no
    /// taskbar and no way to ask for one.
    #[test]
    fn stopping_the_bar_restores_explorer() {
        let stop = super::SERVICES[9];
        assert_eq!(stop.last().unwrap().args, &["--restore-tray"]);
    }

    /// Either branch is machine-specific, but both have to name the bar:
    /// the Run entry points at it, and the fallback is the bare name.
    #[test]
    fn the_bar_resolves_to_the_bar() {
        assert!(super::bar_exe().ends_with("optim-bar.exe"), "{}", super::bar_exe());
    }

    #[test]
    fn config_path_follows_komorebis_own_lookup() {
        let path = komorebi_config();
        assert!(path.ends_with("komorebi.json"), "{path}");
    }
}
