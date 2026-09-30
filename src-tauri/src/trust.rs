//! What the webview may launch. `launch_app`, `launch_app_admin` and
//! `open_recent` end in `ShellExecuteW`, so they must not run whatever string
//! the page hands them. An id or path is accepted only if Rust itself produced
//! it (the AppsFolder listing, the file picker, file search, jump lists, a
//! drop from Explorer) or the user had it pinned when the panel started
//! (read from `prefs.json`).

use std::collections::HashSet;
use std::path::Path;
use std::sync::Mutex;

use serde_json::Value;

const FILE_PREFIX: &str = "file:";

#[derive(Default)]
struct Trusted {
    /// Ids and paths that Rust handed to the webview this session.
    seen: HashSet<String>,
    /// Pinned at start; survives restarts because the pins do.
    pinned: HashSet<String>,
}

static TRUSTED: Mutex<Option<Trusted>> = Mutex::new(None);

fn with<R>(f: impl FnOnce(&mut Trusted) -> R) -> R {
    let mut guard = TRUSTED.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(Trusted::default))
}

/// Windows paths compare case-insensitively and with either slash.
fn norm(path: &str) -> String {
    path.replace('/', "\\")
        .trim_end_matches('\\')
        .to_lowercase()
}

fn key(id: &str) -> String {
    match id.strip_prefix(FILE_PREFIX) {
        Some(path) => format!("{FILE_PREFIX}{}", norm(path)),
        None => id.to_string(),
    }
}

/// An app id (AppsFolder parsing name) or a `file:` id that Rust produced.
pub fn remember_id(id: &str) {
    let k = key(id);
    with(|t| t.seen.insert(k));
}

pub fn remember_ids<'a>(ids: impl IntoIterator<Item = &'a str>) {
    let keys: Vec<String> = ids.into_iter().map(key).collect();
    with(|t| t.seen.extend(keys));
}

/// A filesystem path Rust produced; trusted as a `file:` id and as a document.
pub fn remember_path(path: &str) {
    remember_id(&format!("{FILE_PREFIX}{path}"));
}

pub fn is_trusted(id: &str) -> bool {
    let k = key(id);
    with(|t| t.seen.contains(&k) || t.pinned.contains(&k))
}

pub fn is_trusted_path(path: &str) -> bool {
    is_trusted(&format!("{FILE_PREFIX}{path}"))
}

/// Everything that was pinned, from `prefs.json`: `pinned` and the items of `folders`.
fn pinned_ids(prefs: &Value) -> Vec<String> {
    let direct = prefs["pinned"].as_array().into_iter().flatten();
    let grouped = prefs["folders"]
        .as_object()
        .into_iter()
        .flat_map(|f| f.values())
        .flat_map(|f| f["items"].as_array().into_iter().flatten());
    direct
        .chain(grouped)
        .filter_map(|v| v.as_str())
        .map(str::to_string)
        .collect()
}

/// Reads the pins once at start, before the webview can change them.
pub fn load_pins(prefs_file: &Path) {
    let Ok(text) = std::fs::read_to_string(prefs_file) else {
        return;
    };
    let Ok(prefs) = serde_json::from_str::<Value>(&text) else {
        return;
    };
    let keys: HashSet<String> = pinned_ids(&prefs).iter().map(|id| key(id)).collect();
    with(|t| t.pinned = keys);
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collects_pins_and_folder_items() {
        let prefs = json!({
            "pinned": ["Microsoft.Notepad_8wekyb3d8bbwe!App", "file:C:\\Tools\\Far.lnk", "folder:abc"],
            "folders": { "abc": { "name": "Dev", "items": ["file:D:\\Code", "{F38BF404}\\git.exe"] } }
        });
        let ids = pinned_ids(&prefs);
        assert!(ids.contains(&"file:D:\\Code".to_string()));
        assert!(ids.contains(&"Microsoft.Notepad_8wekyb3d8bbwe!App".to_string()));
        assert_eq!(pinned_ids(&json!({})), Vec::<String>::new());
    }

    #[test]
    fn trusts_only_what_was_remembered() {
        assert!(!is_trusted("C:\\Windows\\System32\\cmd.exe"));
        assert!(!is_trusted("file:C:\\Windows\\System32\\cmd.exe"));
        assert!(!is_trusted("https://example.com"));
        remember_path("C:\\Users\\me\\Report.docx");
        assert!(is_trusted("file:c:/users/me/report.docx"));
        assert!(is_trusted_path("C:\\Users\\me\\Report.docx\\"));
        assert!(!is_trusted("file:C:\\Users\\me\\other.exe"));
        remember_id("Microsoft.Notepad_8wekyb3d8bbwe!App");
        assert!(is_trusted("Microsoft.Notepad_8wekyb3d8bbwe!App"));
        // An app id is not a file path and vice versa.
        assert!(!is_trusted("file:Microsoft.Notepad_8wekyb3d8bbwe!App"));
    }
}
