//! "Paste a URL, watch it" — a persistent watch queue on one shared mpv window.
//!
//! Every URL is downloaded into `%TEMP%\watchqueue` and recorded in a history
//! file (`%LOCALAPPDATA%\optim\watch-history.tsv`). The history *is* the
//! playlist: optim replays it into mpv whenever the shared window is opened,
//! so tabs survive closing the player and rebooting the machine.
//!
//! Nothing is ever evicted on a timer or a size cap. A video leaves the queue
//! only when it is explicitly dropped — `X` in the mpv tab menu (which runs
//! `optim --watch-forget <path>`) or "optim: Clear Watch History".
//!
//! mpv is started with `--input-ipc-server=\\.\pipe\mpv-optim`; while that
//! instance is alive every new video is appended to its playlist ("tabs").
//! Playback of stream URLs (YouTube etc.) relies on yt-dlp being on PATH.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Mutex, MutexGuard, OnceLock};
use std::time::Duration;

const PIPE: &str = r"\\.\pipe\mpv-optim";

/// Nothing this small is a playable video; it's a host's error page.
const MIN_MEDIA_BYTES: u64 = 64 * 1024;
/// A file this young may be a download still in flight on another thread —
/// it is small and new for a reason, so never judge or delete it.
const IN_FLIGHT_GRACE_SECS: u64 = 300;
/// Downloads run a few at a time. Ten parallel curls just split the line ten
/// ways and nothing becomes watchable; queueing means the first videos are
/// playable while the rest are still coming down.
const MAX_PARALLEL_DOWNLOADS: usize = 3;

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

// ---------------------------------------------------------------- mpv IPC

fn pipe_open() -> std::io::Result<std::fs::File> {
    std::fs::OpenOptions::new().read(true).write(true).open(PIPE)
}

/// True while the shared instance's pipe answers.
fn pipe_alive() -> bool {
    pipe_open().is_ok()
}

fn send(pipe: &mut std::fs::File, json: &str) -> std::io::Result<()> {
    pipe.write_all(json.as_bytes())?;
    pipe.write_all(b"\n")
}

/// One-shot command to the shared window; Err when nothing is listening.
fn ipc(json: &str) -> std::io::Result<()> {
    send(&mut pipe_open()?, json)
}

fn loadfile_cmd(target: &str, mode: &str) -> String {
    format!(
        "{{\"command\":[\"loadfile\",{},\"{}\"]}}",
        json_escape(target),
        mode
    )
}

/// A line of text on the player itself — the only place the user is looking
/// while videos are queueing up behind the one they're watching.
fn osd(text: &str) {
    let _ = ipc(&format!(
        "{{\"command\":[\"show-text\",{},2500]}}",
        json_escape(text)
    ));
}

/// Waits out a freshly spawned mpv's startup for the pipe to appear.
fn wait_for_pipe() -> Option<std::fs::File> {
    for _ in 0..40 {
        if let Ok(p) = pipe_open() {
            return Some(p);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    None
}

fn mpv_exe() -> PathBuf {
    let winget = Path::new(r"C:\Program Files\MPV Player\mpv.exe");
    if winget.exists() {
        winget.to_path_buf()
    } else {
        PathBuf::from("mpv.exe") // PATH fallback
    }
}

// ------------------------------------------------------------- the history

fn temp_dir() -> PathBuf {
    std::env::temp_dir().join("watchqueue")
}

fn state_dir() -> PathBuf {
    let base = std::env::var("LOCALAPPDATA").unwrap_or_else(|_| ".".into());
    PathBuf::from(base).join("optim")
}

fn history_path() -> PathBuf {
    state_dir().join("watch-history.tsv")
}

/// mpv's tab menu shells out to `optim --watch-forget`, so it needs to know
/// where this build lives; the lua script reads this file.
pub fn record_exe_path() {
    if let Ok(exe) = std::env::current_exe() {
        let _ = std::fs::create_dir_all(state_dir());
        let _ = std::fs::write(state_dir().join("optim-exe.txt"), exe.to_string_lossy().as_bytes());
    }
}

/// One remembered video: when it was queued, where it landed, where it came from.
#[derive(Clone, Debug, PartialEq)]
pub struct Entry {
    pub added: u64,
    pub path: String,
    pub url: String,
}

fn parse_line(line: &str) -> Option<Entry> {
    let mut f = line.split('\t');
    let added = f.next()?.trim().parse().ok()?;
    let path = f.next()?.trim().to_string();
    let url = f.next().unwrap_or("").trim().to_string();
    if path.is_empty() {
        return None;
    }
    Some(Entry { added, path, url })
}

fn format_line(e: &Entry) -> String {
    let clean = |s: &str| s.replace(['\t', '\r', '\n'], " ");
    format!("{}\t{}\t{}", e.added, clean(&e.path), clean(&e.url))
}

/// Windows paths compare case-insensitively, and mpv hands back exactly what
/// we gave it — but a `/` vs `\` mismatch shouldn't orphan an entry.
fn same_path(a: &str, b: &str) -> bool {
    let norm = |s: &str| s.replace('/', "\\").to_lowercase();
    norm(a) == norm(b)
}

/// Guards every read-modify-write of the history file. Poisoning is not
/// interesting here: the data is a list of paths, not an invariant.
fn hist_lock() -> MutexGuard<'static, ()> {
    static LOCK: Mutex<()> = Mutex::new(());
    LOCK.lock().unwrap_or_else(|e| e.into_inner())
}

fn read_history() -> Vec<Entry> {
    let text = std::fs::read_to_string(history_path()).unwrap_or_default();
    let mut list: Vec<Entry> = text.lines().filter_map(parse_line).collect();
    list.sort_by_key(|e| e.added); // queue order, not download-finish order
    list
}

fn write_history(list: &[Entry]) {
    let _ = std::fs::create_dir_all(state_dir());
    let body: String = list.iter().map(|e| format_line(e) + "\n").collect();
    // Write-then-rename: a half-written history would silently lose tabs.
    let tmp = history_path().with_extension("tsv.new");
    if std::fs::write(&tmp, body).is_ok() {
        let _ = std::fs::rename(&tmp, history_path());
    }
}

/// The remembered queue, oldest first.
pub fn history() -> Vec<Entry> {
    let _g = hist_lock();
    read_history()
}

fn remember(entry: Entry) {
    let _g = hist_lock();
    let mut list = read_history();
    if list.iter().any(|e| same_path(&e.path, &entry.path)) {
        return;
    }
    list.push(entry);
    write_history(&list);
}

fn find_by_url(url: &str) -> Option<Entry> {
    let _g = hist_lock();
    read_history().into_iter().find(|e| e.url == url)
}

/// Drop one video: out of the history and off the disk. Accepts either the
/// local path (what mpv knows a tab by) or the original URL.
pub fn forget(target: &str) -> bool {
    let _g = hist_lock();
    let mut list = read_history();
    let mut dropped = Vec::new();
    list.retain(|e| {
        let hit = same_path(&e.path, target) || e.url == target;
        if hit {
            dropped.push(e.path.clone());
        }
        !hit
    });
    if dropped.is_empty() {
        return false;
    }
    write_history(&list);
    for p in dropped {
        // Fails while mpv still holds the file open; the sweep gets it later.
        let _ = std::fs::remove_file(p);
    }
    true
}

/// How much the queue is holding: (files, bytes).
pub fn stats() -> (usize, u64) {
    let Ok(entries) = std::fs::read_dir(temp_dir()) else {
        return (0, 0);
    };
    entries
        .flatten()
        .filter_map(|e| e.metadata().ok())
        .filter(|m| m.is_file())
        .fold((0, 0), |(n, b), m| (n + 1, b + m.len()))
}

/// Wipe the queue: the history and every file in the cache directory.
/// Returns (files removed, bytes freed).
///
/// mpv is told to drop the playlist first — Windows won't delete a file the
/// player still has open, and "clear" that leaves 8 GB behind isn't a clear.
pub fn clear() -> (usize, u64) {
    let _ = ipc("{\"command\":[\"playlist-clear\"]}");
    let _ = ipc("{\"command\":[\"stop\"]}");
    std::thread::sleep(Duration::from_millis(400)); // let mpv close its handles

    let _g = hist_lock();
    write_history(&[]);
    let mut files = 0;
    let mut bytes = 0;
    if let Ok(entries) = std::fs::read_dir(temp_dir()) {
        for e in entries.flatten() {
            let Ok(meta) = e.metadata() else { continue };
            if !meta.is_file() {
                continue;
            }
            if std::fs::remove_file(e.path()).is_ok() {
                files += 1;
                bytes += meta.len();
            }
        }
    }
    (files, bytes)
}

/// Decides what a startup sweep does with each file sitting in the cache:
/// keep it (it's in the history or too young to judge), adopt it into the
/// history, or delete it as an orphan.
///
/// Pure so the policy is testable. `adopting` is the first run after the
/// history file appeared — the cache already holds real downloads then, and
/// deleting an evening's worth of them because no history mentions them yet
/// would be exactly the data loss this whole feature exists to prevent.
#[derive(Debug, PartialEq)]
enum Fate {
    Keep,
    Adopt,
    Delete,
}

fn fate(known: bool, age_secs: u64, size: u64, adopting: bool) -> Fate {
    if known {
        return Fate::Keep;
    }
    // Everything already in the cache when the history appeared is adopted
    // whatever its age. Merely keeping a young file here would hand it to the
    // *next* sweep as an unknown orphan — the one run where "wait and see"
    // means "delete it tomorrow".
    if adopting && size >= MIN_MEDIA_BYTES {
        return Fate::Adopt;
    }
    if age_secs < IN_FLIGHT_GRACE_SECS {
        return Fate::Keep; // too new to judge; may be downloading right now
    }
    // Not in the history and old enough to be settled: an aborted download or
    // an error page. Nothing in the queue points at it, so nothing loses it.
    Fate::Delete
}

/// Startup tidy-up. Everything the history knows about is untouchable — this
/// only collects files no entry points at (failed downloads, leftovers from
/// before the history existed).
pub fn sweep() {
    let _ = std::fs::create_dir_all(temp_dir());
    let _g = hist_lock();
    let adopting = !history_path().exists();
    let mut list = read_history();
    let Ok(entries) = std::fs::read_dir(temp_dir()) else {
        return;
    };
    let mut changed = adopting;
    for e in entries.flatten() {
        let Ok(meta) = e.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let path = e.path().to_string_lossy().to_string();
        let known = list.iter().any(|h| same_path(&h.path, &path));
        let age = meta
            .modified()
            .ok()
            .and_then(|t| t.elapsed().ok())
            .map(|d| d.as_secs())
            .unwrap_or(0);
        match fate(known, age, meta.len(), adopting) {
            Fate::Keep => {}
            Fate::Adopt => {
                list.push(Entry {
                    added: mtime_ms(&meta),
                    path,
                    url: String::new(),
                });
                changed = true;
            }
            Fate::Delete => {
                let _ = std::fs::remove_file(e.path());
            }
        }
    }
    if changed {
        list.sort_by_key(|e| e.added);
        write_history(&list);
    }
}

fn mtime_ms(meta: &std::fs::Metadata) -> u64 {
    meta.modified()
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as u64)
        .unwrap_or_else(now_ms)
}

// ------------------------------------------------------------ the window

/// Tick of the last idle-window spawn, so two rapid pastes don't both
/// launch an mpv (the second would lose the pipe and orphan a window).
static SPAWNED_AT: AtomicU64 = AtomicU64::new(0);

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Show the shared window immediately (idle, no file) so Enter feels instant;
/// tabs arrive once data is ready. Returns true when *this* call started it,
/// which is also the signal that its playlist is empty and wants the history.
fn warm_window() -> bool {
    if pipe_alive() {
        return false;
    }
    let last = SPAWNED_AT.load(Ordering::Relaxed);
    let now = now_ms();
    if now.saturating_sub(last) < 5000 {
        return false; // another paste just spawned it; its pipe is coming up
    }
    SPAWNED_AT.store(now, Ordering::Relaxed);
    // The IPC pipe is set here rather than in mpv.conf on purpose: an mpv
    // opened any other way must not grab the name optim talks to.
    let _ = std::process::Command::new(mpv_exe())
        .args([
            "--idle=yes",
            "--force-window=immediate",
            &format!("--input-ipc-server={PIPE}"),
        ])
        .spawn();
    true
}

/// Replay the remembered queue into a freshly opened window. Entries load
/// with `append` so nothing starts playing over the video the user is
/// actually waiting for; `play_last` is for reopening the queue on its own.
fn restore(play_last: bool) -> usize {
    let list = history();
    let Some(mut pipe) = wait_for_pipe() else {
        return 0;
    };
    let mut n = 0;
    for e in &list {
        if !Path::new(&e.path).exists() {
            continue; // deleted behind our back; the entry stays for the record
        }
        if send(&mut pipe, &loadfile_cmd(&e.path, "append")).is_err() {
            break;
        }
        n += 1;
    }
    if play_last && n > 0 {
        let _ = send(
            &mut pipe,
            &format!("{{\"command\":[\"set_property\",\"playlist-pos\",{}]}}", n - 1),
        );
    }
    n
}

/// Best-effort raise of the shared player. mpv registers its video window as
/// class "mpv"; if that ever changes this quietly does nothing.
fn raise_window() {
    use windows::core::w;
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };
    unsafe {
        if let Ok(hwnd) = FindWindowW(w!("mpv"), None) {
            if hwnd != HWND::default() {
                let _ = ShowWindow(hwnd, SW_RESTORE);
                let _ = SetForegroundWindow(hwnd);
            }
        }
    }
}

/// Append to the shared window, waiting briefly for a freshly spawned
/// instance's pipe. Last resort: a standalone window that takes over as the
/// shared one, so the next paste finds it.
fn open_in_shared_window(target: &str) {
    for _ in 0..40 {
        if ipc(&loadfile_cmd(target, "append-play")).is_ok() {
            return;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let _ = std::process::Command::new(mpv_exe())
        .arg("--force-window=yes")
        .arg(format!("--input-ipc-server={PIPE}"))
        .arg(target)
        .spawn();
}

/// Open the queue on its own: the shared window with every remembered video
/// as a tab, the most recent one playing.
pub fn open_history() {
    std::thread::spawn(|| {
        if warm_window() {
            let n = restore(true);
            if n == 0 {
                osd("watch queue is empty");
            }
        } else {
            raise_window();
            osd(&format!("{} in the queue (TAB for tabs)", history().len()));
        }
    });
}

// -------------------------------------------------------------- downloads

/// Filename for a URL: honors `download_filename=` (common on file hosts),
/// else the last path segment, else a timestamp. Always sanitized.
fn dest_for(url: &str) -> PathBuf {
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
    temp_dir().join(format!("{}-{name}", now_ms()))
}

/// Human label for a queued item, for OSD messages.
fn label_for(dest: &Path) -> String {
    let name = dest
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = name.split_once('-').map(|(_, t)| t.to_string()).unwrap_or(name);
    if name.chars().count() > 48 {
        format!("{}\u{2026}", name.chars().take(47).collect::<String>())
    } else {
        name
    }
}

fn start_download(url: &str, dest: &Path) -> Option<std::process::Child> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    std::process::Command::new("curl.exe")
        // `--fail`: without it a 403/404 body gets written to the .mp4 and
        // opened as a broken tab (that's where the 23-byte "videos" came from).
        .args(["-L", "-s", "--fail", "--retry", "5", "--retry-delay", "2", "-C", "-", "-o"])
        .arg(dest)
        .arg(url)
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()
        .ok()
}

fn file_len(p: &Path) -> u64 {
    std::fs::metadata(p).map(|m| m.len()).unwrap_or(0)
}

/// True when the bytes on disk are an error page rather than a video.
///
/// Deliberately a reject-list, not an allow-list: guessing at every container
/// magic number would eventually refuse a real file, whereas the failure mode
/// worth catching is a host answering 200 with HTML or JSON.
fn looks_like_error_page(head: &[u8]) -> bool {
    let start = head
        .iter()
        .position(|b| !b.is_ascii_whitespace())
        .unwrap_or(head.len());
    let body = &head[start..];
    body.starts_with(b"<") || body.starts_with(b"{") || body.starts_with(b"HTTP/")
}

fn head_bytes(p: &Path, n: usize) -> Vec<u8> {
    use std::io::Read;
    let Ok(mut f) = std::fs::File::open(p) else {
        return Vec::new();
    };
    let mut buf = vec![0u8; n];
    match f.read(&mut buf) {
        Ok(read) => {
            buf.truncate(read);
            buf
        }
        Err(_) => Vec::new(),
    }
}

/// Is this download worth handing to mpv?
fn is_playable(p: &Path) -> bool {
    file_len(p) >= MIN_MEDIA_BYTES && !looks_like_error_page(&head_bytes(p, 64))
}

static ACTIVE_DOWNLOADS: AtomicUsize = AtomicUsize::new(0);

fn inflight() -> MutexGuard<'static, HashSet<String>> {
    static SET: OnceLock<Mutex<HashSet<String>>> = OnceLock::new();
    SET.get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// A place in the download queue. Holding one means "I am one of the few
/// downloads allowed to run"; dropping it hands the slot to the next paste,
/// on every exit path including the failure ones.
struct Slot(String);

impl Slot {
    /// None when this URL is already downloading — pasting twice shouldn't
    /// fetch twice.
    fn claim(url: &str, label: &str) -> Option<Slot> {
        if !inflight().insert(url.to_string()) {
            return None;
        }
        let mut announced = false;
        loop {
            let n = ACTIVE_DOWNLOADS.load(Ordering::SeqCst);
            if n < MAX_PARALLEL_DOWNLOADS
                && ACTIVE_DOWNLOADS
                    .compare_exchange(n, n + 1, Ordering::SeqCst, Ordering::Relaxed)
                    .is_ok()
            {
                return Some(Slot(url.to_string()));
            }
            if !announced {
                osd(&format!("queued: {label}"));
                announced = true;
            }
            std::thread::sleep(Duration::from_millis(250));
        }
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        ACTIVE_DOWNLOADS.fetch_sub(1, Ordering::SeqCst);
        inflight().remove(&self.0);
    }
}

/// Queue a URL: download it to the cache and add it as a tab in the shared
/// window as soon as ~1 MB has landed — mpv plays the still-growing file
/// while curl finishes behind it. Falls back to direct streaming when the
/// download fails outright (e.g. sites that need yt-dlp). Runs on a
/// background thread: optim is resident, so downloads outlive the popup.
pub fn play(url: &str) {
    const EARLY_PLAY_BYTES: u64 = 1024 * 1024;
    let url = url.to_string();
    std::thread::spawn(move || {
        // The window appears NOW; data catches up to it.
        if warm_window() {
            restore(false);
        }
        let _ = std::fs::create_dir_all(temp_dir());

        // Already in the queue: raise the tab instead of spending the
        // bandwidth again. This is what makes re-pasting a link cheap.
        if let Some(e) = find_by_url(&url) {
            if Path::new(&e.path).exists() {
                open_in_shared_window(&e.path);
                return;
            }
        }

        let dest = dest_for(&url);
        let Some(_slot) = Slot::claim(&url, &label_for(&dest)) else {
            osd("already downloading that one");
            return;
        };
        // Queue position is claimed at paste time so the history keeps the
        // order videos were asked for, not the order they finished in.
        let added = now_ms();

        let Some(mut child) = start_download(&url, &dest) else {
            open_in_shared_window(&url); // no curl? stream it
            return;
        };
        let mut opened = false;
        let exit = loop {
            let done = child.try_wait().ok().flatten();
            if !opened && file_len(&dest) >= EARLY_PLAY_BYTES && is_playable(&dest) {
                // Enough header bytes for mpv to start on the growing file
                // (assumes web-optimized mp4; worst case the tab errors and
                // plays fine once the download completes).
                remember(Entry { added, path: dest.to_string_lossy().into(), url: url.clone() });
                open_in_shared_window(&dest.to_string_lossy());
                opened = true;
            }
            if let Some(status) = done {
                break status;
            }
            std::thread::sleep(Duration::from_millis(50));
        };
        // A finished download still has to be real media: hosts answer dead
        // links with a 200 and an error body, which used to become a broken
        // tab *and* sit in the cache forever.
        if !opened && exit.success() && is_playable(&dest) {
            remember(Entry { added, path: dest.to_string_lossy().into(), url: url.clone() });
            open_in_shared_window(&dest.to_string_lossy());
            opened = true;
        }
        if !opened {
            // Download produced nothing usable — stream the URL directly.
            let _ = std::fs::remove_file(&dest);
            open_in_shared_window(&url);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::{
        dest_for, fate, format_line, json_escape, label_for, looks_like_error_page, parse_line,
        same_path, Entry, Fate, IN_FLIGHT_GRACE_SECS, MIN_MEDIA_BYTES,
    };

    const GB: u64 = 1024 * 1024 * 1024;

    #[test]
    fn error_pages_are_not_videos() {
        assert!(looks_like_error_page(b"<html><body>410 gone"));
        assert!(looks_like_error_page(b"\n  {\"error\":\"expired\"}"));
        assert!(looks_like_error_page(b"HTTP/1.1 403 Forbidden"));
        // Real containers must survive: mp4 (ftyp at 4), matroska, and any
        // format we never thought to list.
        assert!(!looks_like_error_page(b"\0\0\0 ftypisom\0\0\x02\0"));
        assert!(!looks_like_error_page(&[0x1A, 0x45, 0xDF, 0xA3, 0x01, 0x00]));
        assert!(!looks_like_error_page(&[0x47, 0x40, 0x00, 0x10]));
        assert!(!looks_like_error_page(b""));
    }

    /// The whole point of the rewrite: age and size no longer evict anything
    /// the history knows about.
    #[test]
    fn remembered_videos_are_never_swept() {
        let ancient = 400 * 24 * 3600;
        assert_eq!(fate(true, ancient, 40 * GB, false), Fate::Keep);
        assert_eq!(fate(true, ancient, 40 * GB, true), Fate::Keep);
    }

    #[test]
    fn unknown_files_are_orphans_once_they_settle() {
        assert_eq!(fate(false, 3600, GB, false), Fate::Delete);
        // ...but a young unknown file may be a download in flight.
        assert_eq!(fate(false, IN_FLIGHT_GRACE_SECS - 60, 1024, false), Fate::Keep);
    }

    /// First run after the history landed: the cache is full of real videos
    /// that predate it. Keep them, drop the error-page junk.
    #[test]
    fn first_run_adopts_the_existing_cache() {
        assert_eq!(fate(false, 3600, GB, true), Fate::Adopt);
        assert_eq!(fate(false, 3600, 23, true), Fate::Delete);
        // Including one that landed seconds ago: skipping it here would leave
        // it unknown, and the next sweep deletes unknown files.
        assert_eq!(fate(false, 5, GB, true), Fate::Adopt);
        // Small and new is still unjudgeable — it may be a live download.
        assert_eq!(fate(false, 5, 23, true), Fate::Keep);
    }

    #[test]
    fn history_lines_round_trip() {
        let e = Entry {
            added: 1786750945164,
            path: r"C:\Temp\watchqueue\1-clip.mp4".into(),
            url: "https://host/clip.mp4?token=x".into(),
        };
        assert_eq!(parse_line(&format_line(&e)).unwrap(), e);
        // Junk lines are skipped rather than poisoning the queue.
        assert!(parse_line("").is_none());
        assert!(parse_line("not-a-number\tC:\\x.mp4\t").is_none());
        // A line written before urls were recorded still parses.
        assert_eq!(parse_line("5\tC:\\x.mp4").unwrap().url, "");
    }

    #[test]
    fn paths_compare_the_way_windows_does() {
        assert!(same_path(r"C:\Temp\A.MP4", r"c:/temp/a.mp4"));
        assert!(!same_path(r"C:\Temp\a.mp4", r"C:\Temp\b.mp4"));
    }

    #[test]
    fn labels_drop_the_timestamp_prefix() {
        assert_eq!(label_for(std::path::Path::new(r"C:\t\1786-clip.mp4")), "clip.mp4");
        let long = label_for(std::path::Path::new(&format!(r"C:\t\1786-{}.mp4", "x".repeat(80))));
        assert!(long.chars().count() <= 48);
    }

    /// Live check of the part no unit test can reach: opening the queue really
    /// starts mpv and really replays the tabs into it. Spawns a player window.
    /// `cargo test --release -- --ignored --exact watch::tests::live_restore`
    #[test]
    #[ignore]
    fn live_restore() {
        use std::io::{BufRead, BufReader, Write};
        let sandbox = std::env::temp_dir().join("optim-live-test");
        let _ = std::fs::remove_dir_all(&sandbox);
        std::env::set_var("TEMP", &sandbox);
        std::env::set_var("TMP", &sandbox);
        std::env::set_var("LOCALAPPDATA", sandbox.join("lad"));
        std::fs::create_dir_all(super::temp_dir()).unwrap();
        for n in ["1000-a.mp4", "2000-b.mp4", "3000-c.mp4"] {
            std::fs::write(super::temp_dir().join(n), vec![0u8; 200_000]).unwrap();
        }
        super::sweep();
        assert_eq!(super::history().len(), 3, "sweep should have adopted the cache");

        super::open_history();
        std::thread::sleep(std::time::Duration::from_secs(6));

        let mut pipe = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open(super::PIPE)
            .expect("mpv should be listening on the optim pipe");
        pipe.write_all(b"{\"command\":[\"get_property_string\",\"playlist-count\"]}\n")
            .unwrap();
        let mut line = String::new();
        BufReader::new(pipe.try_clone().unwrap())
            .read_line(&mut line)
            .unwrap();
        let _ = pipe.write_all(b"{\"command\":[\"quit\"]}\n");
        let _ = std::fs::remove_dir_all(&sandbox);
        assert!(line.contains("\"data\":\"3\""), "playlist was: {line}");
    }

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
        assert!(MIN_MEDIA_BYTES > 0);
    }
}
