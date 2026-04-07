//! normalize.rs — convert chat export formats to plain transcript.

use serde_json::Value;
use std::path::Path;

/// Normalize file content to plain transcript string.
pub fn normalize_content(content: &str, path: &Path) -> String {
    let content = content.trim();
    if content.is_empty() {
        return content.to_string();
    }

    // Already has `>` markers — pass through
    let quote_count = content
        .lines()
        .filter(|l| l.trim_start().starts_with('>'))
        .count();
    if quote_count >= 3 {
        return content.to_string();
    }

    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    if ext == "json"
        || ext == "jsonl"
        || content.trim_start().starts_with('{')
        || content.trim_start().starts_with('[')
    {
        if let Some(normalized) = try_normalize_json(content) {
            return normalized;
        }
    }

    content.to_string()
}

fn try_normalize_json(content: &str) -> Option<String> {
    if let Some(s) = try_claude_code_jsonl(content) {
        return Some(s);
    }
    let data: Value = serde_json::from_str(content).ok()?;
    try_claude_ai(&data)
        .or_else(|| try_chatgpt(&data))
        .or_else(|| try_slack(&data))
}

fn try_claude_code_jsonl(content: &str) -> Option<String> {
    let mut messages = Vec::new();
    let mut any_ok = false;
    for line in content.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let Ok(val): Result<Value, _> = serde_json::from_str(line) else {
            continue;
        };
        any_ok = true;
        let role = val["role"].as_str().unwrap_or("");
        let text = extract_text(&val["content"]);
        if !text.is_empty() {
            if role == "user" {
                messages.push(format!("> {}", text.replace('\n', "\n> ")));
            } else {
                messages.push(text);
            }
        }
    }
    if any_ok && !messages.is_empty() {
        Some(messages.join("\n\n"))
    } else {
        None
    }
}

fn try_claude_ai(data: &Value) -> Option<String> {
    let arr = data
        .get("conversation")
        .or(data.get("messages"))?
        .as_array()?;
    if arr.is_empty() {
        return None;
    }
    let mut messages = Vec::new();
    for msg in arr {
        let sender = msg["sender"]
            .as_str()
            .unwrap_or(msg["role"].as_str().unwrap_or("unknown"));
        let text = extract_text(&msg["text"]);
        let text = if text.is_empty() {
            extract_text(&msg["content"])
        } else {
            text
        };
        if text.is_empty() {
            continue;
        }
        if sender == "human" || sender == "user" {
            messages.push(format!("> {}", text.replace('\n', "\n> ")));
        } else {
            messages.push(text);
        }
    }
    if messages.is_empty() {
        None
    } else {
        Some(messages.join("\n\n"))
    }
}

fn try_chatgpt(data: &Value) -> Option<String> {
    let conversations = if data.is_array() {
        data.as_array()?.clone()
    } else if data.get("mapping").is_some() {
        vec![data.clone()]
    } else {
        return None;
    };

    let mut all_messages: Vec<String> = Vec::new();
    for conv in &conversations {
        let mapping = conv.get("mapping")?.as_object()?;
        let mut nodes: Vec<(&str, &Value)> = mapping.iter().map(|(k, v)| (k.as_str(), v)).collect();
        nodes.sort_by(|a, b| {
            let ta = a.1["message"]["create_time"].as_f64().unwrap_or(0.0);
            let tb = b.1["message"]["create_time"].as_f64().unwrap_or(0.0);
            ta.partial_cmp(&tb).unwrap_or(std::cmp::Ordering::Equal)
        });
        for (_, node) in nodes {
            let role = node["message"]["author"]["role"].as_str().unwrap_or("");
            let text = extract_text(&node["message"]["content"]);
            if text.is_empty() || role == "system" {
                continue;
            }
            if role == "user" {
                all_messages.push(format!("> {}", text.replace('\n', "\n> ")));
            } else {
                all_messages.push(text);
            }
        }
    }
    if all_messages.is_empty() {
        None
    } else {
        Some(all_messages.join("\n\n"))
    }
}

fn try_slack(data: &Value) -> Option<String> {
    let arr = data.as_array()?;
    if !arr.iter().any(|m| m["type"].as_str() == Some("message")) {
        return None;
    }
    let messages: Vec<String> = arr
        .iter()
        .filter(|m| m["type"].as_str() == Some("message"))
        .filter_map(|m| {
            let text = m["text"].as_str()?;
            let user = m["user"].as_str().unwrap_or("unknown");
            Some(format!("[{user}] {text}"))
        })
        .collect();
    if messages.is_empty() {
        None
    } else {
        Some(messages.join("\n"))
    }
}

fn extract_text(val: &Value) -> String {
    match val {
        Value::String(s) => s.trim().to_string(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|p| {
                if p["type"].as_str() == Some("text") {
                    p["text"].as_str().map(|s| s.to_string())
                } else {
                    p.as_str().map(|s| s.to_string())
                }
            })
            .collect::<Vec<_>>()
            .join("\n"),
        _ => String::new(),
    }
}
