use std::collections::HashMap;
use std::path::PathBuf;

fn path() -> PathBuf {
    PathBuf::from(std::env::var("LOCALAPPDATA").unwrap_or_default())
        .join("optim")
        .join("frecency.tsv")
}

/// Launch counts per app display name. Capped at scoring time, so no decay
/// bookkeeping is needed.
pub fn load() -> HashMap<String, u32> {
    let mut map = HashMap::new();
    if let Ok(text) = std::fs::read_to_string(path()) {
        for line in text.lines() {
            if let Some((count, name)) = line.split_once('\t') {
                if let Ok(c) = count.parse() {
                    map.insert(name.to_string(), c);
                }
            }
        }
    }
    map
}

pub fn bump(map: &mut HashMap<String, u32>, name: &str) {
    *map.entry(name.to_string()).or_insert(0) += 1;
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut out = String::new();
    for (name, count) in map.iter() {
        out.push_str(&format!("{count}\t{name}\n"));
    }
    let _ = std::fs::write(p, out);
}
