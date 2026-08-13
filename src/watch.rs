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

/// Play the URL in the shared window, starting mpv if needed.
pub fn play(url: &str) {
    if send_ipc(url).is_ok() {
        return;
    }
    let _ = std::process::Command::new(mpv_exe())
        .arg("--force-window=yes")
        .arg(url)
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::json_escape;

    #[test]
    fn escaping() {
        assert_eq!(json_escape("plain"), "\"plain\"");
        assert_eq!(json_escape("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }
}
