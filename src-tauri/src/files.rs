//! A small index of file and folder names for the panel's search: the user's
//! home folder (without AppData, hidden folders and build or package folders)
//! and the other fixed drives. Built in the background at start and every
//! 15 minutes; searching is a scan over names in memory.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;

const REBUILD_EVERY: Duration = Duration::from_secs(15 * 60);
/// Enough for a home folder full of projects; beyond it the index stops growing.
const MAX_ENTRIES: usize = 600_000;
const MAX_DEPTH: usize = 12;

/// Folders whose contents are never what one searches for by name: build and
/// package output, and the profiles and caches apps keep for themselves.
const SKIP: [&str; 33] = [
    "node_modules",
    "target",
    ".git",
    "dist",
    "build",
    "out",
    "obj",
    "bin",
    "__pycache__",
    "venv",
    ".venv",
    "env",
    "vendor",
    "packages",
    "bower_components",
    ".next",
    ".nuxt",
    ".cache",
    "coverage",
    "$recycle.bin",
    "system volume information",
    "windows",
    "ebwebview",
    "user data",
    "user-data",
    "cache",
    "gpucache",
    "code cache",
    "logs",
    "tmp",
    "temp",
    "artifacts",
    "program files",
];

struct Entry {
    /// Lowercased name, for matching.
    key: String,
    path: PathBuf,
    dir: bool,
    /// Unix seconds, for ranking equals.
    modified: u64,
}

static INDEX: RwLock<Option<Arc<Vec<Entry>>>> = RwLock::new(None);

fn roots() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        roots.push(home);
    }
    // Other fixed drives (D:, E:…); C: is Windows and programs, the home folder covers it.
    for letter in b'D'..=b'Z' {
        let root = PathBuf::from(format!("{}:\\", letter as char));
        if is_fixed(&root) {
            roots.push(root);
        }
    }
    roots
}

fn is_fixed(root: &Path) -> bool {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::GetDriveTypeW;
    // DRIVE_FIXED; removable and network drives can be slow or vanish.
    unsafe { GetDriveTypeW(&HSTRING::from(root.as_os_str())) == 3 }
}

fn skip_dir(name: &str, parent_is_home: bool) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with('.')
        || lower.starts_with('$')
        || SKIP.contains(&lower.as_str())
        || (parent_is_home && lower == "appdata")
}

fn hidden(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    // FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_SYSTEM
    meta.file_attributes() & (0x2 | 0x4) != 0
}

fn build() -> Vec<Entry> {
    let home = std::env::var_os("USERPROFILE").map(PathBuf::from);
    let mut out = Vec::new();
    let mut queue: VecDeque<(PathBuf, usize)> = roots().into_iter().map(|r| (r, 0)).collect();
    // Breadth first: when the cap is hit, what's left out is the deepest.
    while let Some((dir, depth)) = queue.pop_front() {
        let Ok(read) = std::fs::read_dir(&dir) else {
            continue;
        };
        let parent_is_home = home.as_deref() == Some(dir.as_path());
        for item in read.flatten() {
            if out.len() >= MAX_ENTRIES {
                return out;
            }
            let Ok(meta) = item.metadata() else { continue };
            let name = item.file_name().to_string_lossy().into_owned();
            if hidden(&meta) || meta.file_type().is_symlink() {
                continue;
            }
            let is_dir = meta.is_dir();
            if is_dir && skip_dir(&name, parent_is_home) {
                continue;
            }
            let modified = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map_or(0, |d| d.as_secs());
            let path = item.path();
            if is_dir && depth + 1 < MAX_DEPTH {
                queue.push_back((path.clone(), depth + 1));
            }
            out.push(Entry {
                key: name.to_lowercase(),
                path,
                dir: is_dir,
                modified,
            });
        }
    }
    out
}

pub fn init() {
    std::thread::Builder::new()
        .name("file-index".into())
        .spawn(|| loop {
            let started = Instant::now();
            let entries = build();
            eprintln!(
                "file index: {} entries in {:?}",
                entries.len(),
                started.elapsed()
            );
            *INDEX.write().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(entries));
            std::thread::sleep(REBUILD_EVERY);
        })
        .ok();
}

fn score(name: &str, q: &str) -> u32 {
    if name == q {
        return 1000;
    }
    let stem = name.rsplit_once('.').map_or(name, |(s, _)| s);
    if stem == q {
        return 950;
    }
    if name.starts_with(q) {
        return 800;
    }
    if name
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(q))
    {
        return 600;
    }
    match name.find(q) {
        Some(at) => 400u32.saturating_sub(at as u32),
        None => 0,
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Hit {
    name: String,
    path: String,
    /// The folder it's in, shortened with ~ for the home folder.
    folder: String,
    dir: bool,
}

fn search(entries: &[Entry], query: &str, limit: usize) -> Vec<Hit> {
    let q = query.trim().to_lowercase();
    if q.chars().count() < 2 {
        return Vec::new();
    }
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    let mut hits: Vec<(u32, &Entry)> = entries
        .iter()
        .filter_map(|e| {
            let s = score(&e.key, &q);
            // Things touched in the last month rank a little higher.
            (s > 0).then(|| {
                (
                    s + if now.saturating_sub(e.modified) < 30 * 86_400 {
                        60
                    } else {
                        0
                    },
                    e,
                )
            })
        })
        .collect();
    hits.sort_by(|a, b| b.0.cmp(&a.0).then(b.1.modified.cmp(&a.1.modified)));
    let home = std::env::var("USERPROFILE").unwrap_or_default();
    hits.into_iter()
        .take(limit)
        .map(|(_, e)| {
            let folder = e
                .path
                .parent()
                .map(|p| p.to_string_lossy().into_owned())
                .unwrap_or_default();
            let folder = match folder.strip_prefix(&home) {
                Some(rest) if !home.is_empty() => format!("~{rest}"),
                _ => folder,
            };
            Hit {
                name: e
                    .path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default(),
                path: e.path.to_string_lossy().into_owned(),
                folder,
                dir: e.dir,
            }
        })
        .collect()
}

/// Files and folders whose name matches; empty while the first index is being built.
#[tauri::command]
pub async fn files_search(query: String, limit: usize) -> Vec<Hit> {
    let Some(index) = INDEX.read().unwrap_or_else(|e| e.into_inner()).clone() else {
        return Vec::new();
    };
    let hits =
        tauri::async_runtime::spawn_blocking(move || search(&index, &query, limit.clamp(1, 20)))
            .await
            .unwrap_or_default();
    hits.iter()
        .for_each(|h| crate::trust::remember_path(&h.path));
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, modified: u64) -> Entry {
        Entry {
            key: name.to_lowercase(),
            path: PathBuf::from(format!("C:\\x\\{name}")),
            dir: false,
            modified,
        }
    }

    #[test]
    fn ranks_names() {
        let entries = [
            entry("Отчёт за сентябрь.docx", 0),
            entry("отчёт.docx", 0),
            entry("старый-отчёт.txt", 0),
            entry("прочее.txt", 0),
        ];
        let hits = search(&entries, "отчёт", 10);
        let names: Vec<&str> = hits.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            names,
            ["отчёт.docx", "Отчёт за сентябрь.docx", "старый-отчёт.txt"]
        );
        assert!(search(&entries, "о", 10).is_empty());
    }

    #[test]
    #[ignore]
    fn index_this_machine() {
        let started = Instant::now();
        let entries = build();
        println!("PROBE {} entries in {:?}", entries.len(), started.elapsed());
        for h in search(&entries, "readme", 5) {
            println!("PROBE hit {} in {}", h.name, h.folder);
        }
    }
}
