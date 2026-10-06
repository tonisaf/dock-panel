use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::{
    path::PathBuf,
    sync::OnceLock,
    time::{SystemTime, UNIX_EPOCH},
};
use tauri::{AppHandle, Manager};

static PATH: OnceLock<PathBuf> = OnceLock::new();
pub(crate) fn storage_path() -> Result<&'static PathBuf, String> {
    PATH.get().ok_or_else(|| "Хранилище ещё не готово".into())
}
pub fn init(app: &AppHandle) {
    if let Ok(dir) = app.path().app_data_dir() {
        let _ = std::fs::create_dir_all(&dir);
        let _ = PATH.set(dir.join("local-notes.sqlite"));
    }
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Item {
    pub id: String,
    pub text: String,
    pub checked: bool,
}
#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Note {
    pub id: String,
    pub title: String,
    pub body: String,
    pub kind: String,
    pub items: Vec<Item>,
    #[serde(default)]
    pub images: Vec<crate::local_note_images::Image>,
    pub color: String,
    pub pinned: bool,
    pub trashed: bool,
    pub created: u64,
    pub edited: u64,
    pub revision: u64,
}
fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}
fn setup(db: &Connection) -> Result<(), String> {
    db.busy_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| e.to_string())?;
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL; CREATE TABLE IF NOT EXISTS notes (id TEXT PRIMARY KEY, document TEXT NOT NULL, revision INTEGER NOT NULL);").map_err(|e|e.to_string())
}
fn all(db: &Connection) -> Result<Vec<Note>, String> {
    let mut statement = db
        .prepare("SELECT document FROM notes")
        .map_err(|e| e.to_string())?;
    let rows = statement
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| e.to_string())?;
    let mut notes = Vec::new();
    for row in rows {
        notes.push(
            serde_json::from_str::<Note>(&row.map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?,
        );
    }
    notes.sort_by(|a, b| b.pinned.cmp(&a.pinned).then(b.edited.cmp(&a.edited)));
    Ok(notes)
}
fn get(db: &Connection, id: &str) -> Result<Note, String> {
    let value: String = db
        .query_row("SELECT document FROM notes WHERE id=?1", [id], |r| r.get(0))
        .map_err(|e| e.to_string())?;
    serde_json::from_str(&value).map_err(|e| e.to_string())
}
fn validate(note: &Note) -> Result<(), String> {
    if !["text", "list"].contains(&note.kind.as_str())
        || !["default", "yellow", "green", "blue", "pink", "purple"].contains(&note.color.as_str())
        || note.title.len() > 1000
        || note.body.len() > 1_000_000
        || note.items.len() > 1000
    {
        return Err("Неверный формат или слишком большая заметка".into());
    }
    if note.images.len() > 20
        || note
            .images
            .iter()
            .any(|i| !crate::local_note_images::safe_id(&i.id) || i.name.len() > 800)
    {
        return Err("Не больше 20 изображений на заметку".into());
    }
    let mut ids = std::collections::HashSet::new();
    if note
        .items
        .iter()
        .any(|i| i.text.len() > 10_000 || i.id.is_empty() || i.id.len() > 128 || !ids.insert(&i.id))
    {
        return Err("Неверные пункты списка".into());
    }
    Ok(())
}
fn create(db: &Connection, title: String, body: String, kind: String) -> Result<Note, String> {
    let mut bytes = [0u8; 16];
    getrandom::fill(&mut bytes).map_err(|e| e.to_string())?;
    let id = format!(
        "local-note:{}",
        bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
    );
    let n = Note {
        id,
        title,
        body,
        kind,
        items: vec![],
        images: vec![],
        color: "default".into(),
        pinned: false,
        trashed: false,
        created: now(),
        edited: now(),
        revision: 1,
    };
    validate(&n)?;
    db.execute(
        "INSERT INTO notes VALUES (?1,?2,?3)",
        params![
            n.id,
            serde_json::to_string(&n).map_err(|e| e.to_string())?,
            n.revision
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(n)
}
fn save(db: &Connection, mut n: Note) -> Result<Note, String> {
    validate(&n)?;
    let old = get(db, &n.id)?;
    n.created = old.created;
    n.edited = now();
    let expected = n.revision;
    n.revision = n
        .revision
        .checked_add(1)
        .filter(|v| *v <= i64::MAX as u64)
        .ok_or("Неверная версия заметки")?;
    let affected = db
        .execute(
            "UPDATE notes SET document=?1,revision=?2 WHERE id=?3 AND revision=?4",
            params![
                serde_json::to_string(&n).map_err(|e| e.to_string())?,
                n.revision,
                n.id,
                expected
            ],
        )
        .map_err(|e| e.to_string())?;
    if affected != 1 {
        return Err(
            "Заметка изменена в другом окне. Черновик сохранён; откройте заметку заново.".into(),
        );
    }
    Ok(n)
}
fn text(n: &Note) -> String {
    if n.kind == "list" {
        n.items
            .iter()
            .map(|i| format!("- [{}] {}", if i.checked { "x" } else { " " }, i.text))
            .collect::<Vec<_>>()
            .join("\n")
    } else {
        n.body.clone()
    }
}
fn external(n: &Note) -> Value {
    json!({"id":n.id,"title":n.title,"url":"","icon":null,"tags":[],"pinned":n.pinned,"edited":crate::notes::iso(n.edited),"created":crate::notes::iso(n.created),"preview":text(n).chars().take(240).collect::<String>(),"local":false})
}
fn execute(db: &Connection, op: &str, args: Value) -> Result<Value, String> {
    match op {
        "list" => Ok(json!(all(db)?)),
        "get" => Ok(json!(get(db, args["id"].as_str().unwrap_or(""))?)),
        "create" | "notes_create" => {
            let n = create(
                db,
                args["title"].as_str().unwrap_or("").into(),
                args["body"].as_str().unwrap_or("").into(),
                args["kind"].as_str().unwrap_or("text").into(),
            )?;
            Ok(if op == "notes_create" {
                external(&n)
            } else {
                json!(n)
            })
        }
        "save" => Ok(json!(save(
            db,
            serde_json::from_value(args["note"].clone()).map_err(|e| e.to_string())?
        )?)),
        "notes_state" | "notes_sync" => Ok(
            json!({"configured":true,"source":{"id":"local","title":"Dock Panel","databaseId":null},"notes":all(db)?.iter().filter(|n|!n.trashed).map(external).collect::<Vec<_>>(),"syncedAt":now(),"syncing":false,"error":null,"pending":0,"tagOptions":[],"canTag":false,"canPin":true}),
        ),
        "notes_search" => {
            let q = args["query"].as_str().unwrap_or("").to_lowercase();
            Ok(json!(all(db)?.iter().filter(|n|!n.trashed && format!("{}\n{}",n.title,text(n)).to_lowercase().contains(&q)).take(args["limit"].as_u64().unwrap_or(6).min(50) as usize).map(|n|json!({"id":n.id,"title":n.title,"icon":null,"snippet":text(n).chars().take(240).collect::<String>()})).collect::<Vec<_>>()))
        }
        "notes_page" => {
            let n = get(db, args["id"].as_str().unwrap_or(""))?;
            Ok(json!(crate::notes::blocks::local_blocks(&text(&n), &n.id)))
        }
        "notes_text" => Ok(json!(text(&get(db, args["id"].as_str().unwrap_or(""))?))),
        _ => Err("Неизвестное действие заметок".into()),
    }
}
#[tauri::command]
pub async fn local_notes_command(app: AppHandle, op: String, args: Value) -> Result<Value, String> {
    let changed = ["create", "save", "notes_create"].contains(&op.as_str());
    let result = tauri::async_runtime::spawn_blocking(move || {
        let db = Connection::open(PATH.get().ok_or("Хранилище ещё не готово")?)
            .map_err(|e| e.to_string())?;
        setup(&db)?;
        execute(&db, &op, args)
    })
    .await
    .map_err(|e| e.to_string())??;
    use tauri::Emitter;
    if changed {
        let _ = app.emit("local-notes:changed", ());
        let _ = app.emit("notes:changed", ());
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn survives_reopening_database() {
        let mut bytes = [0u8; 8];
        getrandom::fill(&mut bytes).unwrap();
        let dir = std::env::temp_dir().join(format!("dock-local-notes-test-{:?}", bytes));
        std::fs::create_dir(&dir).unwrap();
        let path = dir.join("notes.sqlite");
        let id = {
            let db = Connection::open(&path).unwrap();
            setup(&db).unwrap();
            let mut n = create(&db, "План".into(), "Позвонить".into(), "text".into()).unwrap();
            n.color = "blue".into();
            n.pinned = true;
            save(&db, n).unwrap().id
        };
        {
            let db = Connection::open(&path).unwrap();
            setup(&db).unwrap();
            let n = get(&db, &id).unwrap();
            assert_eq!(
                (
                    n.title.as_str(),
                    n.body.as_str(),
                    n.color.as_str(),
                    n.pinned
                ),
                ("План", "Позвонить", "blue", true)
            );
        }
        std::fs::remove_dir_all(dir).unwrap();
    }
    #[test]
    fn persistence_trash_restore_and_conflict() {
        let db = Connection::open_in_memory().unwrap();
        setup(&db).unwrap();
        let first = create(&db, "Title".into(), "Body".into(), "text".into()).unwrap();
        let mut n = first.clone();
        n.trashed = true;
        let n = save(&db, n).unwrap();
        assert!(execute(&db, "notes_state", json!({})).unwrap()["notes"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(save(&db, first).is_err());
        let mut n = n;
        n.trashed = false;
        n.kind = "list".into();
        n.items = vec![Item {
            id: "a".into(),
            text: "Milk".into(),
            checked: true,
        }];
        let n = save(&db, n).unwrap();
        assert_eq!(get(&db, &n.id).unwrap().items[0].text, "Milk");
        assert_eq!(
            execute(&db, "notes_search", json!({"query":"milk"}))
                .unwrap()
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
