//! Read-only view of the Bionic (LM Studio) projects and chats on this PC.
//!
//! Bionic keeps `~/.lmstudio/apps/bionic/projects/<id>/project.json` and, per
//! project, a SQLite `ng-sessions.sqlite` (WAL mode, open in the app while it
//! runs, so it is only ever opened read-only). `sessions` holds the chats;
//! `chat_entries` is a linked list (`previous_id`) of everything said in them,
//! and the chain back from a session's committed head is the branch on screen.
//! The format is Bionic's own and moves between versions, so every field is
//! read leniently and anything unknown is skipped.

use std::collections::HashSet;
use std::path::PathBuf;

use rusqlite::{Connection, OpenFlags};
use serde::Serialize;
use serde_json::Value;

/// A long tool result or argument is cut to this many characters.
const CLIP: usize = 1500;
/// A chat is followed back at most this far.
const MAX_ENTRIES: usize = 5000;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Session {
    id: String,
    title: String,
    updated_ms: i64,
    unread: bool,
    model: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Project {
    id: String,
    name: String,
    sessions: Vec<Session>,
}

/// One piece of a message: `text`, `reasoning`, `tool` (a call) or `result`.
#[derive(Serialize, PartialEq, Debug)]
pub struct Block {
    kind: &'static str,
    text: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Message {
    id: String,
    /// `user`, `assistant` or `tool`.
    role: String,
    ts: Option<i64>,
    blocks: Vec<Block>,
}

fn projects_dir() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_default();
    home.join(".lmstudio")
        .join("apps")
        .join("bionic")
        .join("projects")
}

/// Ids come from the webview and go into a path: only what a UUID is made of.
fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-')
}

fn open(project_id: &str) -> Result<Connection, String> {
    if !valid_id(project_id) {
        return Err("Неверный проект".into());
    }
    let path = projects_dir()
        .join(project_id)
        .join(".internal")
        .join("ng-sessions.sqlite");
    if !path.exists() {
        return Err("Проект не найден".into());
    }
    let conn = Connection::open_with_flags(
        &path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| e.to_string())?;
    conn.busy_timeout(std::time::Duration::from_secs(2))
        .map_err(|e| e.to_string())?;
    Ok(conn)
}

fn clip(text: &str) -> String {
    match text.char_indices().nth(CLIP) {
        Some((end, _)) => format!("{}…", &text[..end]),
        None => text.to_string(),
    }
}

/// The first line of the first thing the user said, for chats Bionic has not named yet.
fn first_user_line(conn: &Connection, head: &str) -> Option<String> {
    let messages = entries_back(conn, head, 400).ok()?;
    let first = messages.into_iter().rev().find(|m| m.role == "user")?;
    let text = first.blocks.iter().find(|b| b.kind == "text")?;
    let line: String = text.text.lines().next()?.chars().take(60).collect();
    (!line.is_empty()).then_some(line)
}

fn sessions_of(conn: &Connection) -> Result<Vec<Session>, String> {
    // Transient and temporary chats and sub-sessions (shell approval reviews,
    // name suggestions) are Bionic's own; the ones without a head are empty.
    let mut stmt = conn
        .prepare(
            "SELECT session_id, session_name, suggested_session_name, updated_timestamp, has_unread, session_json, \
                    committed_head_entry_id \
             FROM sessions \
             WHERE is_transient = 0 AND is_temporary = 0 AND parent_session_id IS NULL \
               AND committed_head_entry_id IS NOT NULL \
             ORDER BY updated_timestamp DESC",
        )
        .map_err(|e| e.to_string())?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, i64>(3)?,
                r.get::<_, i64>(4)? != 0,
                r.get::<_, String>(5)?,
                r.get::<_, String>(6)?,
            ))
        })
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (id, name, suggested, updated_ms, unread, json, head) in rows.flatten() {
        let model = serde_json::from_str::<Value>(&json)
            .ok()
            .and_then(|v| model_name(&v["modelSpecifier"]));
        let named = name
            .filter(|n| !n.trim().is_empty())
            .or(suggested.filter(|n| !n.trim().is_empty()));
        let title = named
            .or_else(|| first_user_line(conn, &head))
            .unwrap_or_else(|| "Без названия".into());
        out.push(Session {
            id,
            title,
            updated_ms,
            unread,
            model,
        });
    }
    Ok(out)
}

/// A model specifier is a string or an object with a path/identifier.
fn model_name(v: &Value) -> Option<String> {
    let s = v.as_str().map(str::to_string).or_else(|| {
        [
            "indexedModelIdentifier",
            "path",
            "identifier",
            "modelKey",
            "id",
            "name",
        ]
        .iter()
        .find_map(|k| v[*k].as_str().map(str::to_string))
    })?;
    Some(s.rsplit('/').next().unwrap_or(&s).to_string())
}

#[tauri::command]
pub fn bionic_projects() -> Vec<Project> {
    let Ok(dir) = std::fs::read_dir(projects_dir()) else {
        return vec![];
    };
    let mut projects: Vec<Project> = dir
        .flatten()
        .filter_map(|entry| {
            let id = entry.file_name().to_string_lossy().into_owned();
            if !valid_id(&id) {
                return None;
            }
            let meta: Value = serde_json::from_str(
                &std::fs::read_to_string(entry.path().join("project.json")).ok()?,
            )
            .ok()?;
            let name = meta["name"].as_str()?.to_string();
            let sessions = open(&id)
                .ok()
                .and_then(|c| sessions_of(&c).ok())
                .unwrap_or_default();
            (!sessions.is_empty()).then_some(Project { id, name, sessions })
        })
        .collect();
    // The project with the freshest chat first.
    projects.sort_by_key(|p| std::cmp::Reverse(p.sessions.first().map_or(0, |s| s.updated_ms)));
    projects
}

#[tauri::command]
pub fn bionic_session(project_id: String, session_id: String) -> Result<Vec<Message>, String> {
    if !valid_id(&session_id) {
        return Err("Неверный чат".into());
    }
    let conn = open(&project_id)?;
    let head: String = conn
        .query_row(
            "SELECT committed_head_entry_id FROM sessions WHERE session_id = ?1",
            [&session_id],
            |r| r.get(0),
        )
        .map_err(|_| "Чат не найден".to_string())?;
    let mut messages = entries_back(&conn, &head, MAX_ENTRIES)?;
    messages.reverse();
    Ok(messages)
}

/// The visible messages from `head` back to the start of the chat, newest first.
fn entries_back(conn: &Connection, head: &str, limit: usize) -> Result<Vec<Message>, String> {
    let mut stmt = conn
        .prepare("SELECT entry_json, previous_id, redirect_to_id FROM chat_entries WHERE id = ?1")
        .map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    let mut seen = HashSet::new();
    let mut current = Some(head.to_string());
    for _ in 0..limit {
        let Some(id) = current.take() else { break };
        if !seen.insert(id.clone()) {
            break;
        }
        let Ok((json, previous, redirect)) = stmt.query_row([&id], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<String>>(2)?,
            ))
        }) else {
            break;
        };
        if let Some(message) = serde_json::from_str::<Value>(&json)
            .ok()
            .and_then(|v| message_of(&v))
        {
            out.push(message);
        }
        current = previous.or(redirect);
    }
    Ok(out)
}

fn text_block(kind: &'static str, part: &Value) -> Option<Block> {
    let text = part["text"].as_str()?.trim();
    (!text.is_empty()).then(|| Block {
        kind,
        text: text.to_string(),
    })
}

/// The shown form of one entry, or `None` for everything that is not a line of the conversation.
fn message_of(entry: &Value) -> Option<Message> {
    if entry["type"] != "message" || entry["hidden"] == true {
        return None;
    }
    let m = &entry["message"];
    // Bionic's own prompts (name suggestions, context) are marked as not wanting an answer.
    if m["noAssistantResponse"] == true {
        return None;
    }
    let role = m["role"].as_str()?.to_string();
    let blocks: Vec<Block> = m["parts"]
        .as_array()?
        .iter()
        .filter_map(|p| match p["type"].as_str()? {
            "text" => text_block("text", p),
            "reasoning" => text_block("reasoning", p),
            "toolCallRequest" => {
                let name = p["name"].as_str().unwrap_or("tool");
                let args = if p["parameters"].is_null() {
                    String::new()
                } else {
                    p["parameters"].to_string()
                };
                Some(Block {
                    kind: "tool",
                    text: clip(&format!("{name} {args}")),
                })
            }
            "toolCallResult" => {
                let text = p["result"]
                    .as_array()
                    .map(|a| {
                        a.iter()
                            .filter_map(|x| x["text"].as_str())
                            .collect::<Vec<_>>()
                            .join("\n")
                    })
                    .or_else(|| p["result"].as_str().map(str::to_string))
                    .unwrap_or_default();
                Some(Block {
                    kind: "result",
                    text: clip(&text),
                })
            }
            _ => None,
        })
        .collect();
    if blocks.is_empty() {
        return None;
    }
    Some(Message {
        id: entry["id"].as_str()?.to_string(),
        role,
        ts: entry["createdTimestamp"].as_i64(),
        blocks,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn rejects_path_tricks() {
        assert!(valid_id("635e2fda-18a1-5f72-b988-792a1a749cac"));
        assert!(!valid_id("../x") && !valid_id("") && !valid_id("a/b"));
    }

    #[test]
    fn shows_only_conversation_messages() {
        let user = json!({ "id": "a", "type": "message", "createdTimestamp": 5,
            "message": { "role": "user", "parts": [{ "type": "text", "text": " Привет " }] } });
        let m = message_of(&user).unwrap();
        assert_eq!((m.role.as_str(), m.ts), ("user", Some(5)));
        assert_eq!(
            m.blocks,
            [Block {
                kind: "text",
                text: "Привет".into()
            }]
        );

        let hidden = json!({ "id": "b", "type": "message", "hidden": true,
            "message": { "role": "user", "parts": [{ "type": "text", "text": "<environment>" }] } });
        let naming = json!({ "id": "c", "type": "message",
            "message": { "role": "user", "noAssistantResponse": true, "parts": [{ "type": "text", "text": "x" }] } });
        let state = json!({ "id": "d", "type": "stateChange" });
        assert!(
            message_of(&hidden).is_none()
                && message_of(&naming).is_none()
                && message_of(&state).is_none()
        );
    }

    #[test]
    fn keeps_tool_calls_and_results() {
        let call = json!({ "id": "e", "type": "message", "message": { "role": "assistant", "parts": [
            { "type": "reasoning", "text": "думаю" },
            { "type": "toolCallRequest", "name": "list_dir", "parameters": { "path": "C:\\x" } }] } });
        let kinds: Vec<_> = message_of(&call)
            .unwrap()
            .blocks
            .iter()
            .map(|b| b.kind)
            .collect();
        assert_eq!(kinds, ["reasoning", "tool"]);
        let result = json!({ "id": "f", "type": "message", "message": { "role": "tool", "parts": [
            { "type": "toolCallResult", "result": [{ "type": "text", "text": "F a.log" }] }] } });
        assert_eq!(message_of(&result).unwrap().blocks[0].text, "F a.log");
    }

    #[test]
    fn clips_long_text_on_a_char_boundary() {
        let long = "я".repeat(CLIP + 10);
        assert_eq!(clip(&long).chars().count(), CLIP + 1);
        assert_eq!(clip("short"), "short");
    }
}
