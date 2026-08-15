//! Start, stop and reload the background tools that have no tray icon of
//! their own.
//!
//! A tiling window manager and its hotkey daemon are controlled entirely from
//! the command line, which leaves no way to restart them after a config edit
//! except opening a terminal. They are exactly the kind of thing a launcher
//! should reach — so they ride the same search as everything else.
//!
//! Nothing here hardcodes a machine: komorebi's own config lookup order
//! (`KOMOREBI_CONFIG_HOME`, else the user profile) decides which file gets
//! reloaded, and every executable is resolved from PATH.

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

/// Steps run in order, so "restart" is simply stop-then-start.
pub const SERVICES: [&[Step]; 7] = [
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
            let child = std::process::Command::new(step.exe)
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

    #[test]
    fn config_path_follows_komorebis_own_lookup() {
        let path = komorebi_config();
        assert!(path.ends_with("komorebi.json"), "{path}");
    }
}
