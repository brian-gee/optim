//! "Paste a URL, watch it" — sends video URLs to a single shared mpv window.
//!
//! mpv runs with `--input-ipc-server=\\.\pipe\mpv-optim` (set in mpv.conf);
//! while that instance is alive, every new URL is appended to its playlist
//! ("tabs"), otherwise a fresh instance is started. Playback of stream URLs
//! (YouTube etc.) relies on yt-dlp being on PATH, which mpv picks up itself.

use std::io::Write;

const PIPE: &str = r"\\.\pipe\mpv-optim";

fn json_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Append to the running shared instance, if any.
fn send_ipc(url: &str) -> std::io::Result<()> {
    let mut pipe = std::fs::OpenOptions::new().read(true).write(true).open(PIPE)?;
    let cmd = format!("{{\"command\":[\"loadfile\",{},\"append-play\"]}}\n", json_escape(url));
    pipe.write_all(cmd.as_bytes())
}

fn mpv_exe() -> std::path::PathBuf {
    let winget = std::path::Path::new(r"C:\Program Files\MPV Player\mpv.exe");
    if winget.exists() {
        winget.to_path_buf()
    } else {
        std::path::PathBuf::from("mpv.exe") // PATH fallback
    }
}

const KEEP_HOURS: u64 = 24;

fn temp_dir() -> std::path::PathBuf {
    std::env::temp_dir().join("watchqueue")
}

/// Delete downloads older than KEEP_HOURS so temp stays temp.
fn prune_old() {
    let Ok(entries) = std::fs::read_dir(temp_dir()) else { return };
    for e in entries.flatten() {
        let stale = e
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|age| age.as_secs() > KEEP_HOURS * 3600)
            .unwrap_or(false);
        if stale {
            let _ = std::fs::remove_file(e.path());
        }
    }
}

/// Filename for a URL: honors `download_filename=` (common on file hosts),
/// else the last path segment, else a timestamp. Always sanitized.
fn dest_for(url: &str) -> std::path::PathBuf {
    let from_param = url
        .split("download_filename=")
        .nth(1)
        .and_then(|rest| rest.split(['&', '/']).next())
        .filter(|s| !s.is_empty());
    let from_path = url
        .split(['?', '#'])
        .next()
        .and_then(|p| p.trim_end_matches('/').rsplit('/').next())
        .filter(|s| s.contains('.') && s.len() > 4);
    let raw = from_param.or(from_path).unwrap_or("video.mp4");
    let mut name: String = raw
        .chars()
        .map(|c| if c.is_alphanumeric() || "-_. ".contains(c) { c } else { '_' })
        .take(80)
        .collect();
    if !name.contains('.') {
        name.push_str(".mp4");
    }
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0);
    temp_dir().join(format!("{stamp}-{name}"))
}

fn start_download(url: &str, dest: &std::path::Path) -> Option<std::process::Child> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("curl.exe")
        .args(["-L", "-s", "--retry", "5", "--retry-delay", "2", "-C", "-", "-o"])
        .arg(dest)
        .arg(url)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()
}

fn file_len(p: &std::path::Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// True while the shared instance's pipe answers.
fn pipe_alive() -> bool {
    std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(PIPE)
        .is_ok()
}

/// Tick of the last idle-window spawn, so two rapid pastes don't both
/// launch an mpv (the second would lose the pipe and orphan a window).
static SPAWNED_AT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Show the shared window immediately (idle, no file) so Enter feels
/// instant; the tab arrives once data is ready.
fn warm_window() {
    use std::sync::atomic::Ordering;
    if pipe_alive() {
        return;
    }
    let last = SPAWNED_AT.load(Ordering::Relaxed);
    let now = now_ms();
    if now.saturating_sub(last) < 5000 {
        return; // another paste just spawned it; its pipe is coming up
    }
    SPAWNED_AT.store(now, Ordering::Relaxed);
    let _ = std::process::Command::new(mpv_exe())
        .args(["--idle=yes", "--force-window=immediate"])
        .spawn();
}

/// Append to the shared window, waiting briefly for a freshly spawned
/// instance's pipe. Last resort: standalone window.
fn open_in_shared_window(target: &str) {
    for _ in 0..40 {
        if send_ipc(target).is_ok() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    let _ = std::process::Command::new(mpv_exe())
        .arg("--force-window=yes")
        .arg(target)
        .spawn();
}

/// Download the URL to temp (scrubbable, survives dead links) and open it
/// as a tab in the shared window as soon as ~1 MB has landed — mpv plays
/// the still-growing file while curl finishes behind it. Falls back to
/// direct streaming when the download fails outright (e.g. streaming sites
/// that need yt-dlp). Runs on a background thread — optim is resident, so
/// the download outlives the popup.
pub fn play(url: &str) {
    const EARLY_PLAY_BYTES: u64 = 1024 * 1024;
    let url = url.to_string();
    std::thread::spawn(move || {
        // The window appears NOW; data catches up to it.
        warm_window();
        let _ = std::fs::create_dir_all(temp_dir());
        prune_old();
        let dest = dest_for(&url);
        let Some(mut child) = start_download(&url, &dest) else {
            open_in_shared_window(&url); // no curl? stream it
            return;
        };
        let mut opened = false;
        let exit = loop {
            let done = child.try_wait().ok().flatten();
            if !opened && (file_len(&dest) >= EARLY_PLAY_BYTES || done.is_some()) {
                // Enough header bytes for mpv to start on the growing file
                // (assumes web-optimized mp4; worst case the tab errors and
                // plays fine once the download completes).
                if file_len(&dest) > 0 {
                    open_in_shared_window(&dest.to_string_lossy());
                    opened = true;
                }
            }
            if let Some(status) = done {
                break status;
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        };
        if !opened {
            // Download produced nothing — stream the URL directly instead.
            let _ = std::fs::remove_file(&dest);
            let _ = exit;
            open_in_shared_window(&url);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{dest_for, json_escape};

    #[test]
    fn escaping() {
        assert_eq!(json_escape("plain"), "\"plain\"");
        assert_eq!(json_escape("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    #[test]
    fn dest_names() {
        let d = dest_for("https://host/get_file/1/abc/1.mp4/?download_filename=cool.mp4&download=true");
        assert!(d.file_name().unwrap().to_string_lossy().ends_with("-cool.mp4"));
        let d = dest_for("https://host/path/clip.webm?token=x");
        assert!(d.file_name().unwrap().to_string_lossy().ends_with("-clip.webm"));
        let d = dest_for("https://host/watch?v=abc123");
        assert!(d.file_name().unwrap().to_string_lossy().ends_with(".mp4"));
    }
}
