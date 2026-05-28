use std::fs;
use std::path::{Path, PathBuf};

use aimem_core::Drawer;
use serde_json::{Map, Value};

pub(crate) const CODEX_REPO_WING: &str = "codex_repo";
pub(crate) const CODEX_EXPERIENCE_WING: &str = "codex_experience";
pub(crate) const CODEX_INCIDENT_WING: &str = "codex_incident";
pub(crate) const CODEX_DEFAULT_LIMIT: usize = 5;

pub(crate) fn rank_codex_drawers(
    mut drawers: Vec<Drawer>,
    query: &str,
    repo_path: &str,
    language: Option<&str>,
) -> Vec<Drawer> {
    drawers.sort_by(|a, b| {
        let a_score = codex_rank_score(a, query, repo_path, language);
        let b_score = codex_rank_score(b, query, repo_path, language);
        b_score
            .cmp(&a_score)
            .then_with(|| b.filed_at.cmp(&a.filed_at))
            .then_with(|| a.id.cmp(&b.id))
    });
    drawers
}

fn codex_rank_score(drawer: &Drawer, query: &str, repo_path: &str, language: Option<&str>) -> i64 {
    let mut score = 0;
    if !repo_path.is_empty()
        && (drawer.content.contains(repo_path) || drawer.source_file.as_deref() == Some(repo_path))
    {
        score += 1_000;
    }
    if drawer.wing == CODEX_INCIDENT_WING || drawer.room == "incident" {
        score += 250;
    }
    if let Some(language) = language {
        if drawer.content.contains(&format!("language: {language}")) {
            score += 150;
        }
    }
    score += codex_keyword_overlap(query, &drawer.content) as i64 * 20;
    score
}

fn codex_keyword_overlap(query: &str, content: &str) -> usize {
    let content = content.to_ascii_lowercase();
    query
        .split(|ch: char| !ch.is_alphanumeric())
        .filter(|token| token.len() >= 3)
        .map(str::to_ascii_lowercase)
        .filter(|token| content.contains(token))
        .count()
}

pub(crate) fn codex_card_scalar(content: &str, key: &str) -> Option<String> {
    let prefix = format!("{key}: ");
    content
        .lines()
        .find_map(|line| line.strip_prefix(&prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToString::to_string)
}

pub(crate) fn detect_repo_profile(repo_path: &str) -> Map<String, Value> {
    let mut profile = Map::new();
    let Some(root) = detect_repo_root(Path::new(repo_path)) else {
        return profile;
    };

    profile.insert(
        "repo_root".to_string(),
        Value::String(root.display().to_string()),
    );
    if let Some(name) = root.file_name().and_then(|name| name.to_str()) {
        profile.insert("repo_name".to_string(), Value::String(name.to_string()));
    }

    let manifests = detect_manifests(&root);
    if !manifests.is_empty() {
        profile.insert(
            "manifests".to_string(),
            Value::Array(manifests.iter().cloned().map(Value::String).collect()),
        );
    }
    if let Some(language) = detect_language(&manifests) {
        profile.insert("language".to_string(), Value::String(language.to_string()));
    }
    let test_commands = detect_test_commands(&manifests);
    if !test_commands.is_empty() {
        profile.insert(
            "test_commands".to_string(),
            Value::Array(test_commands.into_iter().map(Value::String).collect()),
        );
    }

    if let Some(git_head) = read_git_head(&root) {
        profile.insert("git_head".to_string(), Value::String(git_head));
    }
    if let Some(remote) = read_git_remote(&root) {
        profile.insert("git_remote".to_string(), Value::String(remote));
    }

    profile
}

pub(crate) fn merge_detected_repo_profile(
    arguments: &Map<String, Value>,
    mut detected: Map<String, Value>,
) -> Map<String, Value> {
    for (key, value) in arguments {
        detected.insert(key.clone(), value.clone());
    }
    detected
}

fn detect_repo_root(path: &Path) -> Option<PathBuf> {
    let mut current = if path.is_dir() {
        path.to_path_buf()
    } else {
        path.parent()?.to_path_buf()
    };

    loop {
        if current.join(".git").exists() || has_known_manifest(&current) {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn has_known_manifest(dir: &Path) -> bool {
    [
        "Cargo.toml",
        "package.json",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "settings.gradle",
        "deno.json",
    ]
    .iter()
    .any(|name| dir.join(name).is_file())
}

fn detect_manifests(root: &Path) -> Vec<String> {
    [
        "Cargo.toml",
        "package.json",
        "pnpm-lock.yaml",
        "yarn.lock",
        "package-lock.json",
        "pyproject.toml",
        "go.mod",
        "pom.xml",
        "build.gradle",
        "settings.gradle",
        "deno.json",
    ]
    .iter()
    .filter(|name| root.join(name).is_file())
    .map(|name| name.to_string())
    .collect()
}

fn detect_language(manifests: &[String]) -> Option<&'static str> {
    if manifests.iter().any(|item| item == "Cargo.toml") {
        Some("rust")
    } else if manifests.iter().any(|item| item == "package.json") {
        Some("javascript/typescript")
    } else if manifests.iter().any(|item| item == "pyproject.toml") {
        Some("python")
    } else if manifests.iter().any(|item| item == "go.mod") {
        Some("go")
    } else if manifests.iter().any(|item| {
        matches!(
            item.as_str(),
            "pom.xml" | "build.gradle" | "settings.gradle"
        )
    }) {
        Some("jvm")
    } else if manifests.iter().any(|item| item == "deno.json") {
        Some("deno")
    } else {
        None
    }
}

fn detect_test_commands(manifests: &[String]) -> Vec<String> {
    let mut commands = Vec::new();
    if manifests.iter().any(|item| item == "Cargo.toml") {
        commands.push("cargo test".to_string());
    }
    if manifests.iter().any(|item| item == "package.json") {
        commands.push("npm test".to_string());
    }
    if manifests.iter().any(|item| item == "pnpm-lock.yaml") {
        commands.push("pnpm test".to_string());
    }
    if manifests.iter().any(|item| item == "pyproject.toml") {
        commands.push("pytest".to_string());
    }
    if manifests.iter().any(|item| item == "go.mod") {
        commands.push("go test ./...".to_string());
    }
    if manifests.iter().any(|item| item == "pom.xml") {
        commands.push("mvn test".to_string());
    }
    if manifests
        .iter()
        .any(|item| matches!(item.as_str(), "build.gradle" | "settings.gradle"))
    {
        commands.push("gradle test".to_string());
    }
    if manifests.iter().any(|item| item == "deno.json") {
        commands.push("deno test".to_string());
    }
    commands
}

fn read_git_head(root: &Path) -> Option<String> {
    let git = root.join(".git");
    let head_path = if git.is_dir() {
        git.join("HEAD")
    } else {
        return None;
    };
    let head = fs::read_to_string(head_path).ok()?;
    let head = head.trim();
    if let Some(reference) = head.strip_prefix("ref: ") {
        let hash = fs::read_to_string(git.join(reference)).ok();
        if let Some(hash) = hash {
            return Some(format!("{} {}", reference, hash.trim()));
        }
    }
    Some(head.to_string())
}

fn read_git_remote(root: &Path) -> Option<String> {
    let config = fs::read_to_string(root.join(".git").join("config")).ok()?;
    let mut in_origin = false;
    for line in config.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') {
            in_origin = trimmed == r#"[remote "origin"]"#;
            continue;
        }
        if in_origin {
            if let Some(url) = trimmed.strip_prefix("url =") {
                return Some(url.trim().to_string());
            }
        }
    }
    None
}

pub(crate) fn codex_room_for_kind(kind: &str) -> &'static str {
    match kind {
        "incident" => "incident",
        "decision" => "architecture",
        "command" => "workflow",
        "round_summary" => "workflow",
        "release" => "release",
        "dependency" => "dependency",
        "test" | "testing" => "testing",
        "style" => "style",
        _ => "workflow",
    }
}

pub(crate) fn codex_stable_id(kind: &str, repo_path: &str) -> String {
    let digest = md5::compute(format!("codex\u{1f}{kind}\u{1f}{repo_path}").as_bytes());
    format!("codex_{kind}_{digest:x}")
}

pub(crate) fn codex_experience_id(
    kind: &str,
    repo_path: &str,
    arguments: &Map<String, Value>,
) -> String {
    let fingerprint = codex_experience_fingerprint(kind, repo_path, arguments);
    let digest = md5::compute(fingerprint.as_bytes());
    format!(
        "codex_{}_{}",
        codex_slugish(kind),
        hex_prefix(&format!("{digest:x}"), 24)
    )
}

pub(crate) fn codex_experience_fingerprint(
    kind: &str,
    repo_path: &str,
    arguments: &Map<String, Value>,
) -> String {
    let problem = arguments
        .get("problem")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let solution = arguments
        .get("solution")
        .and_then(Value::as_str)
        .unwrap_or_default();
    [kind, repo_path, problem, solution]
        .into_iter()
        .map(canonical_fingerprint_part)
        .collect::<Vec<_>>()
        .join("\u{1f}")
}

fn canonical_fingerprint_part(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_ascii_lowercase()
}

fn hex_prefix(value: &str, len: usize) -> &str {
    let end = value.len().min(len);
    &value[..end]
}

pub(crate) fn format_codex_card(
    kind: &str,
    repo_path: &str,
    arguments: &Map<String, Value>,
    scalar_keys: &[&str],
    filed_at: &str,
) -> String {
    let mut lines = Vec::new();
    lines.push(format!("repo_path: {}", sanitize_codex_value(repo_path)));
    lines.push(format!("kind: {}", sanitize_codex_value(kind)));
    lines.push(format!("created_at: {filed_at}"));

    for key in scalar_keys {
        if let Some(value) = arguments.get(*key).and_then(Value::as_str) {
            if !value.is_empty() {
                lines.push(format!("{key}: {}", sanitize_codex_value(value)));
            }
        }
    }

    for key in [
        "tags",
        "source_files",
        "commands",
        "manifests",
        "test_commands",
        "changed_files",
        "next_steps",
    ] {
        if let Some(items) = string_array(arguments.get(key)) {
            if !items.is_empty() {
                lines.push(format!("{key}:"));
                for item in items {
                    lines.push(format!("  - {}", sanitize_codex_value(&item)));
                }
            }
        }
    }

    lines.push("---".to_string());
    if let Some(details) = arguments.get("details").and_then(Value::as_str) {
        if !details.is_empty() {
            lines.push("Details:".to_string());
            lines.push(sanitize_codex_value(details));
        }
    }
    if let Some(rule) = arguments.get("future_rule").and_then(Value::as_str) {
        if !rule.is_empty() {
            lines.push("Future Codex rule:".to_string());
            lines.push(format!("- {}", sanitize_codex_value(rule)));
        }
    }

    lines.join("\n")
}

pub(crate) fn string_array(value: Option<&Value>) -> Option<Vec<String>> {
    let array = value?.as_array()?;
    Some(
        array
            .iter()
            .filter_map(Value::as_str)
            .filter(|item| !item.is_empty())
            .map(ToString::to_string)
            .collect(),
    )
}

fn sanitize_codex_value(value: &str) -> String {
    value
        .lines()
        .map(redact_secretish_line)
        .collect::<Vec<_>>()
        .join("\n")
}

fn redact_secretish_line(line: &str) -> String {
    let upper = line.to_ascii_uppercase();
    let secretish = ["TOKEN", "SECRET", "PASSWORD", "API_KEY", "PRIVATE KEY"];
    if secretish.iter().any(|needle| upper.contains(needle)) {
        if let Some((key, _)) = line.split_once('=') {
            return format!("{}=<redacted>", key.trim());
        }
        if let Some((key, _)) = line.split_once(':') {
            return format!("{}: <redacted>", key.trim());
        }
        return "<redacted secret-like line>".to_string();
    }
    line.to_string()
}

fn codex_slugish(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for ch in value.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch.to_ascii_lowercase());
        } else {
            out.push('_');
        }
    }
    out.trim_matches('_').to_string()
}
