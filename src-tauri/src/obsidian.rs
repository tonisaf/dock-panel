//! Local Markdown adapter. Never follows links outside the selected vault.
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Component, Path, PathBuf},
    sync::{Mutex, OnceLock},
    time::UNIX_EPOCH,
};

pub(crate) static LOCK: Mutex<()> = Mutex::new(());
static READS: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
fn digest(s: &str) -> String {
    URL_SAFE_NO_PAD.encode(Sha256::digest(s.as_bytes()))
}
fn root() -> Result<PathBuf, String> {
    let s = crate::integrations::integrations_settings();
    if s.notes != "obsidian" && s.tasks != "obsidian" {
        return Err("Obsidian не выбран".into());
    }
    fs::canonicalize(s.vault).map_err(|e| e.to_string())
}
fn path(root: &Path, rel: &str) -> Result<PathBuf, String> {
    let rel = Path::new(rel);
    if rel.components().any(|c| !matches!(c, Component::Normal(_)))
        || rel.extension().and_then(|s| s.to_str()) != Some("md")
    {
        return Err("Недопустимый путь заметки".into());
    }
    let p = root.join(rel);
    let actual = fs::canonicalize(&p).map_err(|e| e.to_string())?;
    if !actual.starts_with(root) || !actual.is_file() {
        return Err("Файл вне хранилища".into());
    }
    Ok(actual)
}
fn id(rel: &str) -> String {
    format!(
        "obsidian:{}:{}",
        digest(&crate::integrations::integrations_settings().vault),
        URL_SAFE_NO_PAD.encode(rel)
    )
}
fn rel(id: &str) -> Result<String, String> {
    let (vault, encoded) = id
        .strip_prefix("obsidian:")
        .ok_or("Неверный источник заметки")?
        .split_once(':')
        .ok_or("Неверная заметка")?;
    if vault != digest(&crate::integrations::integrations_settings().vault) {
        return Err("Хранилище изменилось. Обновите список.".into());
    }
    String::from_utf8(URL_SAFE_NO_PAD.decode(encoded).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())
}
fn read(p: &Path) -> Result<String, String> {
    if fs::metadata(p).map_err(|e| e.to_string())?.len() > 2_000_000 {
        return Err("Заметка превышает 2 МБ".into());
    }
    fs::read_to_string(p).map_err(|e| e.to_string())
}
fn scan(root: &Path, dir: &Path, files: &mut Vec<String>, depth: usize) -> Result<(), String> {
    if depth > 32 {
        return Err("Слишком глубокое хранилище".into());
    }
    for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let ty = entry.file_type().map_err(|e| e.to_string())?;
        if ty.is_symlink() {
            continue;
        }
        let p = fs::canonicalize(entry.path()).map_err(|e| e.to_string())?;
        if !p.starts_with(root) {
            continue;
        }
        if ty.is_dir() {
            scan(root, &p, files, depth + 1)?;
        } else if p.extension().and_then(|s| s.to_str()) == Some("md") {
            files.push(
                p.strip_prefix(root)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .into(),
            );
            if files.len() > 5000 {
                return Err("Хранилище превышает 5000 Markdown-файлов".into());
            }
        }
    }
    Ok(())
}
fn url(root: &Path, rel: &str) -> String {
    use percent_encoding::{utf8_percent_encode, NON_ALPHANUMERIC};
    format!(
        "obsidian://open?path={}",
        utf8_percent_encode(&root.join(rel).to_string_lossy(), NON_ALPHANUMERIC)
    )
}
fn note(root: &Path, rel: &str) -> Result<Value, String> {
    let p = path(root, rel)?;
    let text = read(&p)?;
    let meta = fs::metadata(p).map_err(|e| e.to_string())?;
    let stamp = |t: std::io::Result<std::time::SystemTime>| {
        crate::notes::iso(
            t.ok()
                .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
                .map(|d| d.as_millis() as u64)
                .unwrap_or(0),
        )
    };
    Ok(
        json!({"id":id(rel),"url":url(root,rel),"title":Path::new(rel).file_stem().unwrap_or_default().to_string_lossy(),"icon":null,"tags":[],"pinned":false,"edited":stamp(meta.modified()),"created":stamp(meta.created()),"preview":text.chars().take(240).collect::<String>(),"local":false}),
    )
}
fn checkbox(line: &str) -> Option<(usize, bool, &str)> {
    let trimmed = line.trim_start();
    let offset = line.len() - trimmed.len();
    let s = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .or_else(|| trimmed.strip_prefix("+ "))?;
    let checked = match s.as_bytes().get(..4)? {
        b"[ ] " => false,
        b"[x] " | b"[X] " => true,
        _ => return None,
    };
    Some((offset + 3, checked, &s[4..]))
}
fn task_id(rel: &str, n: usize, title: &str) -> String {
    format!("{}:{}:{}", id(rel), n, digest(title))
}
fn task(root: &Path, rel: &str, n: usize, title: &str) -> Value {
    json!({"id":task_id(rel,n,title),"url":url(root,rel),"title":title,"status":null,"inProgress":false,"due":null,"priority":null,"tag":null})
}
fn task_lines(text: &str) -> Vec<(usize, bool, &str)> {
    let mut fence: Option<char> = None;
    let mut frontmatter = false;
    let mut result = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if n == 0 && line.trim() == "---" {
            frontmatter = true;
            continue;
        }
        if frontmatter {
            if line.trim() == "---" {
                frontmatter = false;
            }
            continue;
        }
        let s = line.trim_start();
        if s.starts_with("```") || s.starts_with("~~~") {
            let ch = s.chars().next().unwrap();
            if fence == Some(ch) {
                fence = None;
            } else if fence.is_none() {
                fence = Some(ch);
            }
            continue;
        }
        if fence.is_none() {
            if let Some((_, checked, title)) = checkbox(line) {
                result.push((n, checked, title));
            }
        }
    }
    result
}
fn write(p: &Path, before: &str, after: &str) -> Result<(), String> {
    if read(p)? != before {
        return Err("Файл изменился в Obsidian. Обновите список и повторите действие.".into());
    }
    let temp = p.with_extension(format!("md.{}.tmp", std::process::id()));
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp)
        .map_err(|e| e.to_string())?;
    let result = (|| {
        f.write_all(after.as_bytes()).map_err(|e| e.to_string())?;
        f.sync_all().map_err(|e| e.to_string())?;
        drop(f);
        if read(p)? != before {
            return Err("Файл изменился. Повторите после обновления.".into());
        }
        fs::rename(&temp, p).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_file(temp);
    }
    result
}
fn mutate_task(
    root: &Path,
    tid: &str,
    checked: Option<bool>,
    title: Option<&str>,
    remove: bool,
) -> Result<Value, String> {
    let mut parts = tid.rsplitn(3, ':');
    let hash = parts.next().ok_or("Неверная задача")?;
    let n: usize = parts
        .next()
        .ok_or("Неверная задача")?
        .parse()
        .map_err(|_| "Неверная строка")?;
    let rel = rel(parts.next().ok_or("Неверная задача")?)?;
    let p = path(root, &rel)?;
    let before = read(&p)?;
    let mut lines: Vec<String> = before.split_inclusive('\n').map(str::to_string).collect();
    let line = lines
        .get_mut(n)
        .ok_or("Задача переместилась. Обновите список.")?;
    let (offset, _, old_title) =
        checkbox(line.trim_end_matches(['\r', '\n'])).ok_or("Строка больше не задача")?;
    if digest(old_title) != hash || !task_lines(&before).iter().any(|(i, _, _)| *i == n) {
        return Err("Задача изменилась. Обновите список.".into());
    }
    let new_title = title.unwrap_or(old_title).to_string();
    if new_title.trim().is_empty() || new_title.contains(['\r', '\n']) {
        return Err("Название задачи должно занимать одну строку".into());
    }
    if remove {
        line.clear();
    } else {
        if title.is_some() {
            let ending = if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            };
            *line = format!("{}{}{}", &line[..offset + 3], new_title, ending);
        }
        if let Some(checked) = checked {
            line.replace_range(offset..offset + 1, if checked { "x" } else { " " });
        }
    }
    write(&p, &before, &lines.concat())?;
    Ok(task(root, &rel, n, &new_title))
}
fn page(root: &Path, rel: &str) -> Result<Value, String> {
    let text = read(&path(root, rel)?)?;
    let todos = task_lines(&text);
    let mut blocks = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut parsed = crate::notes::blocks::local_blocks(line, &format!("obs-line-{n}"));
        if let Some(b) = parsed.first_mut() {
            b.id = format!("{n}:{}", digest(line));
            if let Some((_, checked, title)) = todos.iter().find(|(i, _, _)| *i == n) {
                b.kind = "to_do".into();
                b.checked = Some(*checked);
                b.text = vec![crate::notes::blocks::Span {
                    text: (*title).into(),
                    ..Default::default()
                }];
            } else if b.kind == "to_do" {
                b.kind = "paragraph".into();
                b.checked = None;
                b.text = vec![crate::notes::blocks::Span {
                    text: line.into(),
                    ..Default::default()
                }];
            }
        }
        blocks.extend(parsed);
    }
    serde_json::to_value(blocks).map_err(|e| e.to_string())
}
fn create(root: &Path, title: &str, body: &str) -> Result<String, String> {
    if title.trim().is_empty()
        || title.contains(['/', '\\', ':', '*', '?', '"', '<', '>', '|', '\r', '\n'])
        || title.len() > 180
        || title.trim().trim_end_matches('.').is_empty()
    {
        return Err("Укажите название без запрещённых символов имени файла".into());
    }
    let dir = root.join("Dock Panel");
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    if !fs::canonicalize(&dir)
        .map_err(|e| e.to_string())?
        .starts_with(root)
    {
        return Err("Папка вне хранилища".into());
    }
    let rel = format!("Dock Panel/{}.md", title.trim().trim_end_matches('.'));
    use std::io::Write;
    let mut f = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(&rel))
        .map_err(|e| format!("Не удалось создать заметку (возможно, имя уже занято): {e}"))?;
    f.write_all(body.as_bytes()).map_err(|e| e.to_string())?;
    Ok(rel)
}
#[tauri::command]
pub async fn obsidian_command(op: String, args: Value) -> Result<Value, String> {
    tauri::async_runtime::spawn_blocking(move || execute(op, args))
        .await
        .map_err(|e| e.to_string())?
}
fn execute(op: String, args: Value) -> Result<Value, String> {
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let root = root()?;
    let string = |key: &str| args[key].as_str().unwrap_or("");
    match op.as_str() {
        "notes_state" | "notes_sync" | "notes_search" | "notion_tasks" => {
            let mut files = Vec::new();
            scan(&root, &root, &mut files, 0)?;
            files.sort();
            if op == "notion_tasks" {
                let mut tasks = Vec::new();
                for rel in files {
                    for (n, checked, title) in task_lines(&read(&path(&root, &rel)?)?) {
                        if !checked {
                            tasks.push(task(&root, &rel, n, title));
                        }
                    }
                }
                return Ok(json!({"tasks":tasks,"canComplete":true}));
            }
            let mut notes = files
                .iter()
                .map(|r| note(&root, r))
                .collect::<Result<Vec<_>, _>>()?;
            notes.sort_by(|a, b| b["edited"].as_str().cmp(&a["edited"].as_str()));
            if op == "notes_search" {
                let query = string("query").to_lowercase();
                let mut hits = Vec::new();
                for n in notes {
                    let content = read(&path(&root, &rel(n["id"].as_str().unwrap())?)?)?;
                    if n["title"]
                        .as_str()
                        .unwrap_or("")
                        .to_lowercase()
                        .contains(&query)
                        || content.to_lowercase().contains(&query)
                    {
                        hits.push(json!({"id":n["id"],"title":n["title"],"icon":null,"snippet":content.chars().take(240).collect::<String>()}));
                    }
                    if hits.len() >= args["limit"].as_u64().unwrap_or(6).min(50) as usize {
                        break;
                    }
                }
                return Ok(json!(hits));
            }
            Ok(
                json!({"configured":true,"source":{"id":"obsidian","title":"Obsidian","databaseId":null},"notes":notes,"syncedAt":std::time::SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis() as u64,"syncing":false,"error":null,"pending":0,"tagOptions":[],"canTag":false,"canPin":false}),
            )
        }
        "notes_create" => {
            let rel = create(&root, string("title"), string("body"))?;
            note(&root, &rel)
        }
        "notes_page" => page(&root, &rel(string("id"))?),
        "notes_text" => {
            let p = path(&root, &rel(string("id"))?)?;
            let text = read(&p)?;
            READS
                .get_or_init(Default::default)
                .lock()
                .unwrap()
                .insert(p, digest(&text));
            Ok(json!(text))
        }
        "notes_edit" => {
            let rel = rel(string("id"))?;
            let p = path(&root, &rel)?;
            let before = read(&p)?;
            let mut reads = READS.get_or_init(Default::default).lock().unwrap();
            if reads.get(&p) != Some(&digest(&before)) {
                return Err("Заметка изменилась. Откройте редактор заново.".into());
            }
            write(&p, &before, string("text"))?;
            reads.insert(p, digest(string("text")));
            page(&root, &rel)
        }
        "notes_set_props" => Err("Название Markdown-файла меняйте в Obsidian".into()),
        "notes_toggle" => {
            let rel = rel(string("pageId"))?;
            let p = path(&root, &rel)?;
            let before = read(&p)?;
            let (n, hash) = string("blockId").split_once(':').ok_or("Неверный блок")?;
            let n: usize = n.parse().map_err(|_| "Неверная строка")?;
            let line = before.lines().nth(n).ok_or("Строка переместилась")?;
            if digest(line) != hash {
                return Err("Заметка изменилась. Обновите её.".into());
            }
            let (_, _, title) = checkbox(line).ok_or("Не чекбокс")?;
            mutate_task(
                &root,
                &task_id(&rel, n, title),
                Some(args["checked"].as_bool().unwrap_or(false)),
                None,
                false,
            )?;
            page(&root, &rel)
        }
        "notion_task_schema" => Ok(
            json!({"statuses":[],"priorities":[],"tags":[],"hasStatus":false,"hasDue":false,"hasPriority":false,"hasTag":false}),
        ),
        "notion_complete" | "notion_restore" => {
            mutate_task(
                &root,
                string("pageId"),
                Some(op == "notion_complete"),
                None,
                false,
            )?;
            Ok(Value::Null)
        }
        "notion_delete" => {
            mutate_task(&root, string("pageId"), None, None, true)?;
            Ok(Value::Null)
        }
        "notion_update" => mutate_task(
            &root,
            string("pageId"),
            None,
            args["change"]["title"].as_str(),
            false,
        ),
        "notion_create" => {
            let title = string("title").trim();
            if title.is_empty() || title.contains(['\r', '\n']) {
                return Err("Укажите название задачи в одну строку".into());
            }
            let rel = "Dock Panel/Tasks.md";
            if !root.join(rel).exists() {
                create(&root, "Tasks", "")?;
            }
            let p = path(&root, rel)?;
            let before = read(&p)?;
            let separator = if before.is_empty() || before.ends_with('\n') {
                ""
            } else {
                "\n"
            };
            let n = before.lines().count();
            write(&p, &before, &format!("{before}{separator}- [ ] {title}\n"))?;
            Ok(task(&root, rel, n, title))
        }
        _ => Err("Неподдерживаемое действие Obsidian".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn checkboxes_skip_code_and_metadata() {
        assert_eq!(
            task_lines(
                "---\n- [ ] metadata\n---\n```md\n- [ ] example\n```\n  - [ ] real\n* [X] done\n"
            ),
            vec![(6, false, "real"), (7, true, "done")]
        );
    }
    #[test]
    fn encoded_paths_roundtrip() {
        assert_eq!(rel(&id("Папка/Заметка.md")).unwrap(), "Папка/Заметка.md");
    }
    #[test]
    fn rejects_traversal() {
        assert!(path(Path::new("."), "../secret.md").is_err());
        assert!(path(Path::new("."), "C:/secret.md").is_err());
    }
    struct Vault(PathBuf);
    impl Vault {
        fn new() -> Self {
            let mut bytes = [0u8; 8];
            getrandom::fill(&mut bytes).unwrap();
            let dir = std::env::temp_dir().join(format!(
                "dock-obsidian-test-{}",
                URL_SAFE_NO_PAD.encode(bytes)
            ));
            fs::create_dir(&dir).unwrap();
            Self(fs::canonicalize(dir).unwrap())
        }
    }
    impl Drop for Vault {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
    #[test]
    fn complete_undo_and_rename_preserve_markdown() {
        let vault = Vault::new();
        let p = vault.0.join("Note.md");
        let original =
            "---\r\ntags: [work]\r\n---\r\n# Title\r\n  - [ ] Buy milk\r\nTail **bold**\r\n";
        fs::write(&p, original).unwrap();
        let tid = task_id("Note.md", 4, "Buy milk");
        mutate_task(&vault.0, &tid, Some(true), None, false).unwrap();
        assert_eq!(read(&p).unwrap(), original.replace("[ ]", "[x]"));
        mutate_task(&vault.0, &tid, Some(false), None, false).unwrap();
        assert_eq!(read(&p).unwrap(), original);
        let renamed = mutate_task(&vault.0, &tid, None, Some("Buy bread"), false).unwrap();
        assert_eq!(read(&p).unwrap(), original.replace("Buy milk", "Buy bread"));
        assert!(mutate_task(&vault.0, &tid, Some(true), None, false).is_err());
        mutate_task(&vault.0, renamed["id"].as_str().unwrap(), None, None, true).unwrap();
        assert_eq!(
            read(&p).unwrap(),
            original.replace("  - [ ] Buy milk\r\n", "")
        );
    }
    #[test]
    fn external_changes_are_not_overwritten() {
        let vault = Vault::new();
        let p = vault.0.join("Note.md");
        fs::write(&p, "external edit").unwrap();
        assert!(write(&p, "original", "panel edit").is_err());
        assert_eq!(read(&p).unwrap(), "external edit");
        fs::write(&p, "Intro\n- [ ] moved\n").unwrap();
        assert!(mutate_task(
            &vault.0,
            &task_id("Note.md", 0, "moved"),
            Some(true),
            None,
            false
        )
        .is_err());
        assert_eq!(read(&p).unwrap(), "Intro\n- [ ] moved\n");
    }
    #[test]
    fn create_never_replaces_existing_note() {
        let vault = Vault::new();
        let rel = create(&vault.0, "First", "Original").unwrap();
        assert!(create(&vault.0, "First", "Replacement").is_err());
        assert_eq!(read(&path(&vault.0, &rel).unwrap()).unwrap(), "Original");
    }
}
