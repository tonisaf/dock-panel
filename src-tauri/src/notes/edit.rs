//! Editing a note's content as plain text. The note becomes one line per
//! block ("# ", "- ", "1. ", "[ ] ", "> ", indentation for nesting; images and
//! other blocks the text can't express become locked tokens like
//! "⟦картинка⟧"), and a save is diffed line by line against the lines it
//! started from, so Notion gets only what changed: untouched blocks keep
//! their formatting, comments and ids.

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::blocks::{Block, Span};

/// Two spaces (or a tab) per nesting level.
const INDENT: &str = "  ";
/// A line break inside one block, which the one-line-per-block text can't hold.
const SOFT_BREAK: char = '↵';

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct Line {
    pub depth: usize,
    /// Notion block type, or "locked" for blocks kept as they are.
    pub kind: String,
    pub text: String,
    #[serde(default)]
    pub checked: bool,
    /// The block this line came from; `None` for a new line.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
}

impl Line {
    fn key(&self) -> (usize, &str, &str, bool) {
        (self.depth, &self.kind, &self.text, self.checked)
    }
}

fn plain(spans: &[Span]) -> String {
    spans.iter().map(|s| s.text.as_str()).collect::<String>().replace('\n', &SOFT_BREAK.to_string())
}

/// What a block the text can't express shows as.
fn token(b: &Block) -> String {
    let what = match b.kind.as_str() {
        "image" => "картинка".to_string(),
        "code" => format!("код{}", b.note.as_deref().map(|l| format!(": {l}")).unwrap_or_default()),
        "bookmark" => "ссылка".into(),
        "equation" => "формула".into(),
        "child_page" => format!("страница: {}", plain(&b.text)),
        _ => b.note.clone().unwrap_or_else(|| b.kind.clone()),
    };
    format!("⟦{what}⟧")
}

fn can_nest(kind: &str) -> bool {
    matches!(kind, "paragraph" | "bulleted_list_item" | "numbered_list_item" | "to_do" | "toggle" | "quote" | "callout")
}

const TEXT_KINDS: [&str; 10] = [
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
];

/// The note's blocks as lines, depth first.
pub fn to_lines(blocks: &[Block]) -> Vec<Line> {
    fn walk(blocks: &[Block], depth: usize, out: &mut Vec<Line>) {
        for b in blocks {
            let (kind, text) = if TEXT_KINDS.contains(&b.kind.as_str()) || b.kind == "divider" {
                (b.kind.clone(), plain(&b.text))
            } else {
                ("locked".to_string(), token(b))
            };
            out.push(Line { depth, kind, text, checked: b.checked.unwrap_or(false), id: Some(b.id.clone()) });
            if !b.children.is_empty() && can_nest(&b.kind) {
                walk(&b.children, depth + 1, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(blocks, 0, &mut out);
    out
}

pub fn to_text(lines: &[Line]) -> String {
    lines
        .iter()
        .map(|l| {
            let prefix = match (l.kind.as_str(), l.checked) {
                ("heading_1", _) => "# ",
                ("heading_2", _) => "## ",
                ("heading_3", _) => "### ",
                ("bulleted_list_item", _) => "- ",
                ("numbered_list_item", _) => "1. ",
                ("to_do", false) => "[ ] ",
                ("to_do", true) => "[x] ",
                ("toggle", _) => "+ ",
                ("quote", _) => "> ",
                ("callout", _) => "! ",
                _ => "",
            };
            let text = if l.kind == "divider" { "---" } else { &l.text };
            format!("{}{prefix}{text}", INDENT.repeat(l.depth))
        })
        .collect::<Vec<_>>()
        .join("\n")
}

/// Parses edited text. Nesting only goes under blocks that can hold children,
/// one level deeper than the line above at most.
pub fn from_text(text: &str) -> Vec<Line> {
    let mut out: Vec<Line> = Vec::new();
    for raw in text.lines() {
        if raw.trim().is_empty() {
            continue;
        }
        let expanded = raw.replace('\t', INDENT);
        let spaces = expanded.len() - expanded.trim_start().len();
        let t = expanded.trim();
        let (kind, rest, checked) = if t == "---" {
            ("divider", "", false)
        } else if t.starts_with('⟦') && t.ends_with('⟧') {
            ("locked", t, false)
        } else {
            let rules: [(&str, &str, bool); 13] = [
                ("### ", "heading_3", false),
                ("## ", "heading_2", false),
                ("# ", "heading_1", false),
                ("- [ ] ", "to_do", false),
                ("- [x] ", "to_do", true),
                ("[ ] ", "to_do", false),
                ("[x] ", "to_do", true),
                ("[X] ", "to_do", true),
                ("- ", "bulleted_list_item", false),
                ("* ", "bulleted_list_item", false),
                ("+ ", "toggle", false),
                ("> ", "quote", false),
                ("! ", "callout", false),
            ];
            rules
                .iter()
                .find_map(|(p, k, c)| t.strip_prefix(p).map(|r| (*k, r, *c)))
                .or_else(|| numbered(t).map(|r| ("numbered_list_item", r, false)))
                .unwrap_or(("paragraph", t, false))
        };
        let wanted = spaces / INDENT.len();
        // At most one level under the line above, and only if that one can hold children.
        let depth = match out.last() {
            Some(prev) if can_nest(&prev.kind) => wanted.min(prev.depth + 1),
            Some(prev) => wanted.min(prev.depth),
            None => 0,
        };
        out.push(Line { depth, kind: kind.into(), text: rest.trim_end().into(), checked, id: None });
    }
    out
}

/// "12. text" -> "text".
fn numbered(t: &str) -> Option<&str> {
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    (digits > 0).then(|| t[digits..].strip_prefix(". ")).flatten()
}

/// Gives the edited lines the ids of the lines they keep or change (same
/// depth and kind), so only real differences reach Notion.
pub fn match_ids(before: &[Line], after: &mut [Line]) {
    // Longest common subsequence on the whole line.
    let (n, m) = (before.len(), after.len());
    let mut lcs = vec![vec![0u32; m + 1]; n + 1];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i][j] = if before[i].key() == after[j].key() { lcs[i + 1][j + 1] + 1 } else { lcs[i + 1][j].max(lcs[i][j + 1]) };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut kept: Vec<(usize, usize)> = Vec::new();
    while i < n && j < m {
        if before[i].key() == after[j].key() {
            kept.push((i, j));
            i += 1;
            j += 1;
        } else if lcs[i + 1][j] >= lcs[i][j + 1] {
            i += 1;
        } else {
            j += 1;
        }
    }
    for &(i, j) in &kept {
        after[j].id = before[i].id.clone();
    }
    // Between kept lines, changed lines pair up in order when depth and kind agree.
    let mut bounds = vec![(0usize, 0usize)];
    bounds.extend(kept.iter().map(|&(i, j)| (i + 1, j + 1)));
    let ends: Vec<(usize, usize)> = kept.iter().copied().chain([(n, m)]).collect();
    for (&(bi, bj), &(ei, ej)) in bounds.iter().zip(&ends) {
        let mut olds = (bi..ei).filter(|&i| before[i].kind != "locked" && before[i].kind != "divider").peekable();
        for line in &mut after[bj..ej] {
            let Some(&i) = olds.peek() else { break };
            if before[i].depth == line.depth && before[i].kind == line.kind && line.kind != "locked" {
                line.id = before[i].id.clone();
                olds.next();
            }
        }
    }
}

/// One step of applying an edit to Notion.
#[derive(Debug, PartialEq)]
pub enum Step {
    Update { id: String, body: Value },
    Delete { id: String },
    /// New sibling lines (by index into `after`), under `parent` (None: the page),
    /// placed after `after_line` (an index whose id is known by then) or first.
    Insert { parent: Option<usize>, after_line: Option<usize>, lines: Vec<usize> },
}

fn rich(text: &str) -> Value {
    let text = text.replace(SOFT_BREAK, "\n");
    // Notion caps one text object at 2000 characters.
    let chunks: Vec<Value> = text
        .chars()
        .collect::<Vec<_>>()
        .chunks(2000)
        .map(|c| json!({ "type": "text", "text": { "content": c.iter().collect::<String>() } }))
        .collect();
    json!(chunks)
}

/// A line as a Notion block body (for a new block, or the fields of an update).
pub fn block_body(l: &Line) -> Value {
    match l.kind.as_str() {
        "divider" => json!({ "type": "divider", "divider": {} }),
        "to_do" => json!({ "type": "to_do", "to_do": { "rich_text": rich(&l.text), "checked": l.checked } }),
        // A token typed by hand is just text.
        "locked" => json!({ "type": "paragraph", "paragraph": { "rich_text": rich(&l.text) } }),
        k => json!({ "type": k, k: { "rich_text": rich(&l.text) } }),
    }
}

/// The steps that turn `before` into `after` (whose ids `match_ids` has set).
pub fn plan(before: &[Line], after: &[Line]) -> Vec<Step> {
    let mut steps = Vec::new();
    let kept: std::collections::HashSet<&str> = after.iter().filter_map(|l| l.id.as_deref()).collect();

    for l in after {
        let Some(id) = &l.id else { continue };
        let Some(old) = before.iter().find(|b| b.id.as_deref() == Some(id.as_str())) else { continue };
        if old.key() != l.key() && l.kind != "locked" && l.kind != "divider" {
            let mut body = block_body(l);
            let kind = l.kind.clone();
            steps.push(Step::Update { id: id.clone(), body: json!({ kind.clone(): body[&kind].take() }) });
        }
    }
    for b in before {
        if let Some(id) = &b.id {
            if !kept.contains(id.as_str()) {
                steps.push(Step::Delete { id: id.clone() });
            }
        }
    }

    // New lines, in runs of siblings; a run's parent and predecessor come earlier in
    // the document, so they exist (or were just created) when the run goes out.
    let parent_of = |j: usize| (0..j).rev().find(|&k| after[k].depth < after[j].depth);
    let prev_sibling = |j: usize| {
        (0..j)
            .rev()
            .take_while(|&k| after[k].depth >= after[j].depth)
            .find(|&k| after[k].depth == after[j].depth)
    };
    let mut j = 0;
    while j < after.len() {
        if after[j].id.is_some() {
            j += 1;
            continue;
        }
        let parent = parent_of(j);
        let mut lines = vec![j];
        let mut k = j + 1;
        while k < after.len() && after[k].id.is_none() && after[k].depth == after[j].depth {
            lines.push(k);
            k += 1;
        }
        steps.push(Step::Insert { parent, after_line: prev_sibling(j), lines });
        j = k;
    }
    steps
}

/// The note as it should look after the edit, for the cache: kept blocks stay
/// as they were (formatting, images), changed ones take the new text, new ones
/// get local ids until the sync brings Notion's.
pub fn rebuild(old: &[Block], after: &[Line], local_prefix: &str) -> Vec<Block> {
    let mut by_id = std::collections::HashMap::new();
    fn index(blocks: &[Block], map: &mut std::collections::HashMap<String, Block>) {
        for b in blocks {
            map.insert(b.id.clone(), b.clone());
            index(&b.children, map);
        }
    }
    index(old, &mut by_id);

    let flat: Vec<(usize, Block)> = after
        .iter()
        .enumerate()
        .map(|(n, l)| {
            let original = l.id.as_ref().and_then(|id| by_id.get(id)).cloned();
            let mut b = match original {
                Some(mut b) if l.kind == "locked" || b.kind == "divider" => {
                    b.children.clear();
                    b
                }
                Some(mut b) => {
                    if plain(&b.text) != l.text {
                        b.text = vec![Span { text: l.text.replace(SOFT_BREAK, "\n"), ..Default::default() }];
                    }
                    b.checked = (b.kind == "to_do").then_some(l.checked);
                    b.children.clear();
                    b
                }
                None => {
                    let mut v = block_body(l);
                    v["id"] = json!(format!("{local_prefix}-{n}"));
                    let kind = v["type"].as_str().unwrap_or("paragraph").to_string();
                    let text = l.text.replace(SOFT_BREAK, "\n");
                    v[&kind]["rich_text"] = json!([{ "plain_text": text }]);
                    super::blocks::parse(&v).expect("built from a known shape")
                }
            };
            b.has_children = false;
            (l.depth, b)
        })
        .collect();

    fn nest(flat: &[(usize, Block)], at: &mut usize, depth: usize) -> Vec<Block> {
        let mut out = Vec::new();
        while *at < flat.len() && flat[*at].0 >= depth {
            let mut b = flat[*at].1.clone();
            *at += 1;
            if *at < flat.len() && flat[*at].0 > depth {
                b.children = nest(flat, at, depth + 1);
            }
            out.push(b);
        }
        out
    }
    let mut at = 0;
    nest(&flat, &mut at, 0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(id: &str, kind: &str, text: &str, children: Vec<Block>) -> Block {
        let mut v = json!({ "id": id, "type": kind, kind: { "rich_text": [{ "plain_text": text }] } });
        if kind == "to_do" {
            v[kind]["checked"] = json!(false);
        }
        let mut b = super::super::blocks::parse(&v).unwrap();
        b.children = children;
        b
    }

    fn sample() -> Vec<Block> {
        vec![
            block("h", "heading_2", "План", vec![]),
            block("p", "paragraph", "Купить:", vec![]),
            block("t1", "to_do", "хлеб", vec![block("n", "paragraph", "свежий", vec![])]),
            block("t2", "to_do", "молоко", vec![]),
            block("img", "image", "", vec![]),
        ]
    }

    #[test]
    fn round_trips_through_text() {
        let lines = to_lines(&sample());
        let text = to_text(&lines);
        assert_eq!(text, "## План\nКупить:\n[ ] хлеб\n  свежий\n[ ] молоко\n⟦картинка⟧");
        let mut parsed = from_text(&text);
        match_ids(&lines, &mut parsed);
        assert_eq!(parsed, lines);
        assert!(plan(&lines, &parsed).is_empty());
    }

    #[test]
    fn plans_only_the_difference() {
        let before = to_lines(&sample());
        let mut after = from_text("## План\nКупить:\n[x] хлеб\n  свежий\n  без глютена\n[ ] сыр\n[ ] молоко\n⟦картинка⟧");
        match_ids(&before, &mut after);
        let steps = plan(&before, &after);
        assert_eq!(steps.len(), 3, "{steps:#?}");
        assert!(matches!(&steps[0], Step::Update { id, body } if id == "t1" && body["to_do"]["checked"] == true));
        // "без глютена" goes under "хлеб", after "свежий".
        assert_eq!(steps[1], Step::Insert { parent: Some(2), after_line: Some(3), lines: vec![4] });
        // "сыр" after "хлеб" at the top level.
        assert_eq!(steps[2], Step::Insert { parent: None, after_line: Some(2), lines: vec![5] });
    }

    #[test]
    fn deletes_and_inserts_at_the_start() {
        let before = to_lines(&sample());
        let mut after = from_text("Сначала\n## План\n[ ] молоко\n⟦картинка⟧");
        match_ids(&before, &mut after);
        let steps = plan(&before, &after);
        let deleted: Vec<&str> = steps.iter().filter_map(|s| match s { Step::Delete { id } => Some(id.as_str()), _ => None }).collect();
        assert_eq!(deleted, ["p", "t1", "n"]);
        assert!(steps.contains(&Step::Insert { parent: None, after_line: None, lines: vec![0] }));
    }

    #[test]
    fn changed_text_updates_the_same_block() {
        let before = to_lines(&sample());
        let mut after = from_text("## План на субботу\nКупить:\n[ ] хлеб\n  свежий\n[ ] молоко\n⟦картинка⟧");
        match_ids(&before, &mut after);
        let steps = plan(&before, &after);
        assert_eq!(steps.len(), 1);
        assert!(matches!(&steps[0], Step::Update { id, body } if id == "h" && body["heading_2"]["rich_text"][0]["text"]["content"] == "План на субботу"));
    }

    #[test]
    fn nesting_only_under_blocks_that_hold_children() {
        let lines = from_text("# Заголовок\n    под заголовком\n- пункт\n      глубоко");
        assert_eq!(lines.iter().map(|l| l.depth).collect::<Vec<_>>(), [0, 0, 0, 1]);
    }

    #[test]
    fn rebuilds_the_cached_tree() {
        let old = sample();
        let before = to_lines(&old);
        let mut after = from_text("## План\n[x] хлеб\n  свежий\n  ещё\n⟦картинка⟧");
        match_ids(&before, &mut after);
        let blocks = rebuild(&old, &after, "local-edit");
        assert_eq!(blocks.len(), 3);
        assert_eq!(blocks[1].checked, Some(true));
        assert_eq!(blocks[1].children.len(), 2);
        assert_eq!(blocks[1].children[1].id, "local-edit-3");
        assert_eq!(blocks[2].kind, "image");
    }
}
