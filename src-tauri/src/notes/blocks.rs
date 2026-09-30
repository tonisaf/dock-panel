//! Notion blocks to the panel's own, smaller block tree (what the cache keeps
//! and the UI draws), their plain text for search, and new-note text back to
//! Notion blocks.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

/// A run of text with one set of annotations.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, Default)]
#[serde(rename_all = "camelCase")]
pub struct Span {
    pub text: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub bold: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub italic: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub strike: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub underline: bool,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub code: bool,
    /// Notion colour name when not "default".
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub href: Option<String>,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Block {
    pub id: String,
    /// Notion's block type: paragraph, heading_1, to_do, image, …; anything
    /// the panel doesn't draw is kept as "unsupported" with its type in `note`.
    pub kind: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub text: Vec<Span>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub checked: Option<bool>,
    /// Callout emoji, code language, unsupported block type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
    /// Image source (remote, until the sync caches it), bookmark or file link.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Image kept on disk: served as `noteimg://localhost/<file>`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub children: Vec<Block>,
    /// Notion said it has children (fetched separately, filled into `children`).
    #[serde(skip)]
    pub has_children: bool,
}

pub fn spans(rich: &Value) -> Vec<Span> {
    rich.as_array()
        .into_iter()
        .flatten()
        .map(|t| {
            let a = &t["annotations"];
            let flag = |k: &str| a[k].as_bool().unwrap_or(false);
            Span {
                text: t["plain_text"].as_str().unwrap_or_default().to_string(),
                bold: flag("bold"),
                italic: flag("italic"),
                strike: flag("strikethrough"),
                underline: flag("underline"),
                code: flag("code"),
                color: a["color"]
                    .as_str()
                    .filter(|c| *c != "default")
                    .map(str::to_string),
                href: t["href"].as_str().map(str::to_string),
            }
        })
        .collect()
}

const DRAWN: [&str; 16] = [
    "paragraph",
    "heading_1",
    "heading_2",
    "heading_3",
    "bulleted_list_item",
    "numbered_list_item",
    "to_do",
    "toggle",
    "quote",
    "callout",
    "code",
    "divider",
    "image",
    "bookmark",
    "child_page",
    "equation",
];

pub fn parse(b: &Value) -> Option<Block> {
    let id = b["id"].as_str()?.to_string();
    let kind = b["type"].as_str()?.to_string();
    let body = &b[&kind];
    let mut block = Block {
        id,
        kind: kind.clone(),
        text: spans(&body["rich_text"]),
        checked: None,
        note: None,
        url: None,
        file: None,
        children: Vec::new(),
        has_children: b["has_children"].as_bool().unwrap_or(false),
    };
    match kind.as_str() {
        "to_do" => block.checked = Some(body["checked"].as_bool().unwrap_or(false)),
        "callout" => block.note = body["icon"]["emoji"].as_str().map(str::to_string),
        "code" => block.note = body["language"].as_str().map(str::to_string),
        "image" => {
            block.url = body["file"]["url"]
                .as_str()
                .or(body["external"]["url"].as_str())
                .map(str::to_string);
            block.text = spans(&body["caption"]);
        }
        "bookmark" | "embed" | "link_preview" => {
            block.url = body["url"].as_str().map(str::to_string);
            block.text = spans(&body["caption"]);
            block.kind = "bookmark".into();
        }
        "child_page" => {
            block.text = vec![Span {
                text: body["title"].as_str().unwrap_or_default().to_string(),
                ..Default::default()
            }];
            // A sub-page is its own note-sized thing: shown as a link, not inlined.
            block.has_children = false;
        }
        "equation" => {
            block.text = vec![Span {
                text: body["expression"].as_str().unwrap_or_default().to_string(),
                code: true,
                ..Default::default()
            }]
        }
        k if DRAWN.contains(&k) => {}
        other => {
            block.kind = "unsupported".into();
            block.note = Some(other.to_string());
            block.has_children = false;
        }
    }
    Some(block)
}

/// All text of a tree, one line per block, for search and previews.
pub fn plain_text(blocks: &[Block]) -> String {
    let mut out = String::new();
    fn walk(blocks: &[Block], out: &mut String) {
        for b in blocks {
            let line: String = b.text.iter().map(|s| s.text.as_str()).collect();
            if !line.trim().is_empty() {
                out.push_str(line.trim());
                out.push('\n');
            }
            walk(&b.children, out);
        }
    }
    walk(blocks, &mut out);
    out
}

/// Every block, depth first.
pub fn each_mut(blocks: &mut [Block], f: &mut impl FnMut(&mut Block)) {
    for b in blocks {
        f(b);
        each_mut(&mut b.children, f);
    }
}

fn rich(text: &str) -> Value {
    json!([{ "type": "text", "text": { "content": text } }])
}

/// A new note's text as Notion blocks: "- " bullets, "[ ]" / "[x]" to-dos,
/// "# " headings, the rest paragraphs.
pub fn from_text(body: &str) -> Vec<Value> {
    body.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty())
        .map(|line| {
            let t = line.trim_start();
            let todo = t.strip_prefix("- [ ] ").or(t.strip_prefix("[ ] ")).map(|r| (r, false)).or_else(|| {
                ["- [x] ", "[x] ", "- [X] ", "[X] "].iter().find_map(|p| t.strip_prefix(p)).map(|r| (r, true))
            });
            if let Some((rest, checked)) = todo {
                json!({ "type": "to_do", "to_do": { "rich_text": rich(rest), "checked": checked } })
            } else if let Some(rest) = t.strip_prefix("- ").or(t.strip_prefix("* ")) {
                json!({ "type": "bulleted_list_item", "bulleted_list_item": { "rich_text": rich(rest) } })
            } else if let Some(rest) = t.strip_prefix("# ") {
                json!({ "type": "heading_3", "heading_3": { "rich_text": rich(rest) } })
            } else {
                json!({ "type": "paragraph", "paragraph": { "rich_text": rich(line) } })
            }
        })
        .collect()
}

/// The same text as panel blocks, for the note shown before Notion has it.
pub fn local_blocks(body: &str, id_prefix: &str) -> Vec<Block> {
    from_text(body)
        .iter()
        .enumerate()
        .filter_map(|(i, v)| {
            let mut v = v.clone();
            v["id"] = json!(format!("{id_prefix}-{i}"));
            let kind = v["type"].as_str()?.to_string();
            let text = v[&kind]["rich_text"][0]["text"]["content"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            v[&kind]["rich_text"] = json!([{ "plain_text": text }]);
            parse(&v)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_blocks() {
        let todo = json!({
            "id": "b1", "type": "to_do", "has_children": false,
            "to_do": { "checked": true, "rich_text": [
                { "plain_text": "Купить ", "annotations": { "bold": false, "color": "default" } },
                { "plain_text": "хлеб", "href": "https://x", "annotations": { "bold": true, "color": "red" } }
            ] }
        });
        let b = parse(&todo).unwrap();
        assert_eq!((b.kind.as_str(), b.checked), ("to_do", Some(true)));
        assert!(
            b.text[1].bold && b.text[1].href.is_some() && b.text[1].color.as_deref() == Some("red")
        );
        let table = json!({ "id": "b2", "type": "table", "has_children": true, "table": {} });
        let t = parse(&table).unwrap();
        assert_eq!(
            (t.kind.as_str(), t.note.as_deref(), t.has_children),
            ("unsupported", Some("table"), false)
        );
        assert_eq!(plain_text(&[b]), "Купить хлеб\n");
    }

    #[test]
    fn text_to_blocks() {
        let blocks = from_text("# План\nкупить:\n- [ ] хлеб\n[x] молоко\n- позвонить\n\n");
        let kinds: Vec<&str> = blocks.iter().map(|b| b["type"].as_str().unwrap()).collect();
        assert_eq!(
            kinds,
            [
                "heading_3",
                "paragraph",
                "to_do",
                "to_do",
                "bulleted_list_item"
            ]
        );
        assert_eq!(blocks[3]["to_do"]["checked"], true);
        let local = local_blocks("- [ ] хлеб", "local-1");
        assert_eq!(
            (
                local[0].id.as_str(),
                local[0].checked,
                local[0].text[0].text.as_str()
            ),
            ("local-1-0", Some(false), "хлеб")
        );
    }
}
