//! Apps the user has hidden from the result list.
//!
//! Keyed by the shell's parsing name, not the display name: parsing names are
//! unique and stable, while two apps can share a display name and a rename
//! would silently un-hide one. The display name rides along so the unhide menu
//! can still name an app whose entry has since left the index.

use std::collections::HashMap;
use std::path::PathBuf;

fn path() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("optim")
        .join("hidden.tsv")
}

/// parsing key -> display name as it read when hidden.
pub fn load() -> HashMap<String, String> {
    let mut map = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(path()) {
        for line in text.lines() {
            if let Some((key, name)) = line.split_once('\t') {
                if !key.is_empty() {
                    map.insert(key.to_string(), name.to_string());
                }
            }
        }
    }
    map
}

fn save(map: &HashMap<String, String>) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut out = String::new();
    for (key, name) in map.iter() {
        out.push_str(&format!("{key}\t{name}\n"));
    }
    let _ = std::fs::write(p, out);
}

pub fn hide(map: &mut HashMap<String, String>, key: &str, name: &str) {
    map.insert(key.to_string(), name.to_string());
    save(map);
}

pub fn unhide(map: &mut HashMap<String, String>, key: &str) {
    map.remove(key);
    save(map);
}

pub fn clear(map: &mut HashMap<String, String>) {
    map.clear();
    save(map);
}

/// Hidden apps as (key, display name), ordered for a menu.
pub fn sorted(map: &HashMap<String, String>) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = map
        .iter()
        .map(|(k, n)| (k.clone(), n.clone()))
        .collect();
    v.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));
    v
}
