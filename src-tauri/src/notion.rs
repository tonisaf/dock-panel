//! Notion tasks: list open tasks from one data source, complete them, add new
//! ones, edit their title, status, due date, priority and tag, delete them. Uses an internal integration token kept in Windows Credential
//! Manager; it never reaches the webview.
//!
//! Any task database works: the title, status (or checkbox), due date,
//! priority and tag properties are detected from the schema.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use reqwest::Method;
use serde::Serialize;
use serde_json::{json, Value};

use crate::{net, secrets};

const API: &str = "https://api.notion.com/v1";
const NOTION_VERSION: &str = "2025-09-03";
const SCHEMA_TTL: Duration = Duration::from_secs(10 * 60);
const PAGE_SIZE: u32 = 50;

// ---- token ----------------------------------------------------------------------

const SECRET: &str = "DockPanel/notion";

pub(crate) fn token() -> Result<String, String> {
    secrets::read(SECRET).ok_or_else(|| "Notion не подключён".to_string())
}

// ---- HTTP -----------------------------------------------------------------------

pub(crate) async fn call(
    token: &str,
    method: Method,
    path: &str,
    body: Option<Value>,
) -> Result<Value, String> {
    let mut req = net::client()
        .request(method, format!("{API}{path}"))
        .bearer_auth(token)
        .header("Notion-Version", NOTION_VERSION);
    if let Some(body) = body {
        req = req.json(&body);
    }
    let res = req
        .send()
        .await
        .map_err(|e| format!("Нет связи с Notion: {e}"))?;
    let status = res.status();
    let v: Value = res.json().await.unwrap_or(Value::Null);
    if status.is_success() {
        return Ok(v);
    }
    Err(match v["code"].as_str() {
        Some("unauthorized") => "Токен Notion недействителен. Проверьте, что скопировали его целиком.".into(),
        Some("object_not_found") | Some("restricted_resource") => {
            "Нет доступа к базе: откройте её в Notion → ··· → Connections и добавьте свою интеграцию.".into()
        }
        Some("rate_limited") => "Notion просит подождать: слишком много запросов.".into(),
        _ => format!("Notion: {}", v["message"].as_str().unwrap_or(status.as_str())),
    })
}

// ---- schema detection -------------------------------------------------------------

#[derive(Clone)]
struct StatusProp {
    name: String,
    options: Vec<Badge>,
    /// Options of the "Complete" group; the first non-archive one marks a task done.
    done: Vec<String>,
    in_progress: Vec<String>,
}

#[derive(Clone)]
struct Schema {
    title: String,
    status: Option<StatusProp>,
    checkbox: Option<String>,
    date: Option<String>,
    priority: Option<String>,
    tag: Option<String>,
    priority_options: Vec<Badge>,
    tag_options: Vec<Badge>,
}

impl Schema {
    fn done_option(&self) -> Option<&str> {
        let s = self.status.as_ref()?;
        s.done
            .iter()
            .find(|n| !matches_any(n, &["archiv", "архив"]))
            .or(s.done.first())
            .map(String::as_str)
    }
}

fn matches_any(name: &str, needles: &[&str]) -> bool {
    let n = name.to_lowercase();
    needles.iter().any(|x| n.contains(x))
}

fn detect(ds: &Value) -> Result<Schema, String> {
    let props = ds["properties"].as_object().ok_or("У базы нет свойств")?;
    let of_type = |t: &'static str| {
        props
            .iter()
            .filter(move |(_, p)| p["type"] == t)
            .map(|(n, p)| (n.clone(), p))
    };
    let prefer = |t: &'static str, hints: &[&str]| {
        of_type(t)
            .find(|(n, _)| matches_any(n, hints))
            .or_else(|| of_type(t).next())
    };

    let title = of_type("title")
        .next()
        .map(|(n, _)| n)
        .ok_or("В базе нет поля названия")?;

    let status = of_type("status").next().map(|(name, p)| {
        let options: HashMap<&str, &str> = p["status"]["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|o| Some((o["id"].as_str()?, o["name"].as_str()?)))
            .collect();
        let group = |group: &str| -> Vec<String> {
            p["status"]["groups"]
                .as_array()
                .into_iter()
                .flatten()
                .filter(|g| {
                    g["name"]
                        .as_str()
                        .is_some_and(|n| n.eq_ignore_ascii_case(group))
                })
                .flat_map(|g| g["option_ids"].as_array().cloned().unwrap_or_default())
                .filter_map(|id| options.get(id.as_str()?).map(|n| n.to_string()))
                .collect()
        };
        let all = p["status"]["options"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(badge)
            .collect();
        StatusProp {
            name,
            options: all,
            done: group("Complete"),
            in_progress: group("In progress"),
        }
    });

    let checkbox = if status.is_none() {
        prefer("checkbox", &["done", "готов", "выполн", "complete"]).map(|(n, _)| n)
    } else {
        None
    };
    let date = prefer("date", &["due", "срок", "дедлайн", "deadline", "дата"]).map(|(n, _)| n);
    let priority = of_type("select")
        .find(|(n, _)| matches_any(n, &["priority", "приоритет"]))
        .map(|(n, _)| n);
    let tag = of_type("select")
        .find(|(n, _)| Some(n) != priority.as_ref())
        .map(|(n, _)| n);
    let options = |name: &Option<String>| -> Vec<Badge> {
        name.as_ref()
            .and_then(|n| props[n]["select"]["options"].as_array())
            .into_iter()
            .flatten()
            .filter_map(badge)
            .collect()
    };
    let (priority_options, tag_options) = (options(&priority), options(&tag));

    Ok(Schema {
        title,
        status,
        checkbox,
        date,
        priority,
        tag,
        priority_options,
        tag_options,
    })
}

static SCHEMAS: Mutex<Option<HashMap<String, (Instant, Schema)>>> = Mutex::new(None);

async fn schema(token: &str, source_id: &str) -> Result<Schema, String> {
    if let Some(s) = SCHEMAS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .get(source_id)
        .filter(|(t, _)| t.elapsed() < SCHEMA_TTL)
        .map(|(_, s)| s.clone())
    {
        return Ok(s);
    }
    let ds = call(
        token,
        Method::GET,
        &format!("/data_sources/{source_id}"),
        None,
    )
    .await?;
    let s = detect(&ds)?;
    SCHEMAS
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .get_or_insert_with(HashMap::new)
        .insert(source_id.to_string(), (Instant::now(), s.clone()));
    Ok(s)
}

// ---- mapping ----------------------------------------------------------------------

#[derive(Serialize, Clone)]
pub struct Badge {
    name: String,
    color: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    id: String,
    url: String,
    title: String,
    status: Option<Badge>,
    in_progress: bool,
    /// ISO date or datetime.
    due: Option<String>,
    priority: Option<Badge>,
    tag: Option<Badge>,
}

pub(crate) fn plain_text(rich: &Value) -> String {
    rich.as_array()
        .into_iter()
        .flatten()
        .filter_map(|t| t["plain_text"].as_str())
        .collect()
}

fn badge(v: &Value) -> Option<Badge> {
    Some(Badge {
        name: v["name"].as_str()?.to_string(),
        color: v["color"].as_str().unwrap_or("default").to_string(),
    })
}

fn to_task(page: &Value, schema: &Schema) -> Task {
    let props = &page["properties"];
    let status = schema
        .status
        .as_ref()
        .and_then(|s| badge(&props[&s.name]["status"]));
    let in_progress = match (&schema.status, &status) {
        (Some(s), Some(b)) => s.in_progress.contains(&b.name),
        _ => false,
    };
    Task {
        id: page["id"].as_str().unwrap_or_default().to_string(),
        url: page["url"].as_str().unwrap_or_default().to_string(),
        title: plain_text(&props[&schema.title]["title"]),
        status,
        in_progress,
        due: schema
            .date
            .as_ref()
            .and_then(|d| props[d]["date"]["start"].as_str().map(str::to_string)),
        priority: schema
            .priority
            .as_ref()
            .and_then(|p| badge(&props[p]["select"])),
        tag: schema.tag.as_ref().and_then(|t| badge(&props[t]["select"])),
    }
}

// ---- commands ---------------------------------------------------------------------

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NotionStatus {
    connected: bool,
    workspace: Option<String>,
    error: Option<String>,
}

async fn whoami(token: &str) -> Result<Option<String>, String> {
    let me = call(token, Method::GET, "/users/me", None).await?;
    Ok(me["bot"]["workspace_name"].as_str().map(str::to_string))
}

#[tauri::command]
pub async fn notion_status() -> NotionStatus {
    let Ok(token) = token() else {
        return NotionStatus {
            connected: false,
            workspace: None,
            error: None,
        };
    };
    match whoami(&token).await {
        Ok(workspace) => NotionStatus {
            connected: true,
            workspace,
            error: None,
        },
        Err(e) => NotionStatus {
            connected: true,
            workspace: None,
            error: Some(e),
        },
    }
}

#[tauri::command]
pub async fn notion_set_token(token: String) -> Result<Option<String>, String> {
    let token = token.trim().to_string();
    if token.is_empty() {
        return Err("Вставьте токен интеграции".into());
    }
    let workspace = whoami(&token).await?;
    secrets::write(SECRET, &token)?;
    Ok(workspace)
}

#[tauri::command]
pub fn notion_disconnect() {
    secrets::delete(SECRET);
    if let Ok(mut cache) = SCHEMAS.lock() {
        *cache = None;
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Source {
    id: String,
    title: String,
    /// Parent database, for "open in Notion" links.
    database_id: Option<String>,
}

/// Data sources the integration has been given access to.
#[tauri::command]
pub async fn notion_list_sources() -> Result<Vec<Source>, String> {
    let token = token()?;
    let body =
        json!({ "filter": { "property": "object", "value": "data_source" }, "page_size": 100 });
    let res = call(&token, Method::POST, "/search", Some(body)).await?;
    Ok(res["results"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|ds| {
            let title = plain_text(&ds["title"]);
            Some(Source {
                id: ds["id"].as_str()?.to_string(),
                title: if title.is_empty() {
                    "Без названия".into()
                } else {
                    title
                },
                database_id: ds["parent"]["database_id"].as_str().map(str::to_string),
            })
        })
        .collect())
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskList {
    tasks: Vec<Task>,
    can_complete: bool,
}

/// Open tasks: everything outside the status "Complete" group (or unchecked).
#[tauri::command]
pub async fn notion_tasks(source_id: String) -> Result<TaskList, String> {
    let token = token()?;
    let schema = schema(&token, &source_id).await?;

    let filter = if let Some(s) = &schema.status {
        let clauses: Vec<Value> = s
            .done
            .iter()
            .map(|n| json!({ "property": s.name, "status": { "does_not_equal": n } }))
            .collect();
        (!clauses.is_empty()).then(|| json!({ "and": clauses }))
    } else {
        schema
            .checkbox
            .as_ref()
            .map(|c| json!({ "property": c, "checkbox": { "equals": false } }))
    };
    let mut sorts = Vec::new();
    if let Some(p) = &schema.priority {
        sorts.push(json!({ "property": p, "direction": "ascending" }));
    }
    if let Some(d) = &schema.date {
        sorts.push(json!({ "property": d, "direction": "ascending" }));
    }
    sorts.push(json!({ "timestamp": "created_time", "direction": "descending" }));

    let mut body = json!({ "sorts": sorts, "page_size": PAGE_SIZE });
    if let Some(f) = filter {
        body["filter"] = f;
    }
    let res = call(
        &token,
        Method::POST,
        &format!("/data_sources/{source_id}/query"),
        Some(body),
    )
    .await?;
    let tasks = res["results"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|p| to_task(p, &schema))
        .collect();
    Ok(TaskList {
        tasks,
        can_complete: schema.done_option().is_some() || schema.checkbox.is_some(),
    })
}

#[tauri::command]
pub async fn notion_complete(source_id: String, page_id: String) -> Result<(), String> {
    let token = token()?;
    let schema = schema(&token, &source_id).await?;
    let props = if let (Some(s), Some(done)) = (&schema.status, schema.done_option()) {
        json!({ s.name.clone(): { "status": { "name": done } } })
    } else if let Some(c) = &schema.checkbox {
        json!({ c.clone(): { "checkbox": true } })
    } else {
        return Err("В базе нет статуса или флажка «выполнено»".into());
    };
    call(
        &token,
        Method::PATCH,
        &format!("/pages/{page_id}"),
        Some(json!({ "properties": props })),
    )
    .await?;
    Ok(())
}

/// Undo for `notion_complete`: puts the previous status back (or unchecks).
#[tauri::command]
pub async fn notion_restore(
    source_id: String,
    page_id: String,
    status: Option<String>,
) -> Result<(), String> {
    let token = token()?;
    let schema = schema(&token, &source_id).await?;
    let props = match (&schema.status, status, &schema.checkbox) {
        (Some(s), Some(name), _) => json!({ s.name.clone(): { "status": { "name": name } } }),
        (Some(s), None, _) => json!({ s.name.clone(): { "status": null } }),
        (None, _, Some(c)) => json!({ c.clone(): { "checkbox": false } }),
        _ => return Ok(()),
    };
    call(
        &token,
        Method::PATCH,
        &format!("/pages/{page_id}"),
        Some(json!({ "properties": props })),
    )
    .await?;
    Ok(())
}

#[tauri::command]
pub async fn notion_create(source_id: String, title: String) -> Result<Task, String> {
    let token = token()?;
    let schema = schema(&token, &source_id).await?;
    let body = json!({
        "parent": { "type": "data_source_id", "data_source_id": source_id },
        "properties": { schema.title.clone(): { "title": [{ "text": { "content": title.trim() } }] } },
    });
    let page = call(&token, Method::POST, "/pages", Some(body)).await?;
    Ok(to_task(&page, &schema))
}

/// What a task's editor can offer: the database's options, and which fields it has.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TaskSchema {
    statuses: Vec<Badge>,
    priorities: Vec<Badge>,
    tags: Vec<Badge>,
    has_status: bool,
    has_due: bool,
    has_priority: bool,
    has_tag: bool,
}

#[tauri::command]
pub async fn notion_task_schema(source_id: String) -> Result<TaskSchema, String> {
    let token = token()?;
    let s = schema(&token, &source_id).await?;
    Ok(TaskSchema {
        statuses: s
            .status
            .as_ref()
            .map(|st| st.options.clone())
            .unwrap_or_default(),
        priorities: s.priority_options.clone(),
        tags: s.tag_options.clone(),
        has_status: s.status.is_some(),
        has_due: s.date.is_some(),
        has_priority: s.priority.is_some(),
        has_tag: s.tag.is_some(),
    })
}

/// A present field, even when it is null (null clears it); an absent one stays `None`.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Value>, D::Error> {
    serde::Deserialize::deserialize(d).map(Some)
}

/// Fields to change; left out ones stay, `null` clears (due, priority, tag).
#[derive(serde::Deserialize)]
pub struct TaskChange {
    #[serde(default)]
    title: Option<String>,
    #[serde(default)]
    status: Option<String>,
    /// ISO date or datetime.
    #[serde(default, deserialize_with = "present")]
    due: Option<Value>,
    #[serde(default, deserialize_with = "present")]
    priority: Option<Value>,
    #[serde(default, deserialize_with = "present")]
    tag: Option<Value>,
}

#[tauri::command]
pub async fn notion_update(
    source_id: String,
    page_id: String,
    change: TaskChange,
) -> Result<Task, String> {
    let token = token()?;
    let schema = schema(&token, &source_id).await?;
    let mut props = serde_json::Map::new();
    if let Some(t) = &change.title {
        props.insert(
            schema.title.clone(),
            json!({ "title": [{ "text": { "content": t.trim() } }] }),
        );
    }
    if let (Some(st), Some(prop)) = (&change.status, &schema.status) {
        props.insert(prop.name.clone(), json!({ "status": { "name": st } }));
    }
    if let (Some(due), Some(prop)) = (&change.due, &schema.date) {
        let value = match due.as_str().filter(|d| !d.is_empty()) {
            Some(d) => json!({ "start": d }),
            None => Value::Null,
        };
        props.insert(prop.clone(), json!({ "date": value }));
    }
    for (value, prop) in [
        (&change.priority, &schema.priority),
        (&change.tag, &schema.tag),
    ] {
        if let (Some(v), Some(prop)) = (value, prop) {
            let select = match v.as_str().filter(|s| !s.is_empty()) {
                Some(name) => json!({ "name": name }),
                None => Value::Null,
            };
            props.insert(prop.clone(), json!({ "select": select }));
        }
    }
    let page = call(
        &token,
        Method::PATCH,
        &format!("/pages/{page_id}"),
        Some(json!({ "properties": props })),
    )
    .await?;
    Ok(to_task(&page, &schema))
}

/// Moves the task to Notion's trash (restorable there for 30 days).
#[tauri::command]
pub async fn notion_delete(page_id: String) -> Result<(), String> {
    let token = token()?;
    call(
        &token,
        Method::PATCH,
        &format!("/pages/{page_id}"),
        Some(json!({ "in_trash": true })),
    )
    .await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{detect, TaskChange};
    use serde_json::json;

    #[test]
    fn a_null_field_clears_and_a_missing_one_stays() {
        let c: TaskChange =
            serde_json::from_value(json!({ "due": null, "priority": "Высокий" })).unwrap();
        assert_eq!(c.due, Some(serde_json::Value::Null));
        assert_eq!(c.priority, Some(json!("Высокий")));
        assert!(c.tag.is_none() && c.title.is_none() && c.status.is_none());
    }

    #[test]
    fn detects_wickflow_like_schema() {
        let ds = json!({ "properties": {
            "Task name": { "type": "title" },
            "Status": { "type": "status", "status": {
                "options": [
                    { "id": "a", "name": "Not started" }, { "id": "b", "name": "In progress" },
                    { "id": "c", "name": "Done" }, { "id": "d", "name": "Archived" }
                ],
                "groups": [
                    { "name": "To-do", "option_ids": ["a"] },
                    { "name": "In progress", "option_ids": ["b"] },
                    { "name": "Complete", "option_ids": ["d", "c"] }
                ]
            }},
            "Due": { "type": "date" },
            "Область": { "type": "select" },
            "Приоритет": { "type": "select" }
        }});
        let s = detect(&ds).unwrap();
        assert_eq!(s.title, "Task name");
        assert_eq!(s.done_option(), Some("Done"));
        let status = s.status.clone().unwrap();
        assert_eq!(status.in_progress, vec!["In progress"]);
        assert_eq!(status.options.len(), 4);
        assert_eq!(s.date.as_deref(), Some("Due"));
        assert_eq!(s.priority.as_deref(), Some("Приоритет"));
        assert_eq!(s.tag.as_deref(), Some("Область"));
    }
}
