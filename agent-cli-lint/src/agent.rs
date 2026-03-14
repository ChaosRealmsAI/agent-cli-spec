use std::fs;
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::output::CliError;

pub fn read_brief(root_dir: &Path) -> Result<String, CliError> {
    let path = root_dir.join("agent/brief.md");
    fs::read_to_string(&path).map_err(|_| {
        CliError::new(
            "NOT_FOUND",
            "agent/brief.md not found",
            Some("Create agent/brief.md at the project root."),
            20,
        )
    })
}

pub fn rules_value(root_dir: &Path) -> Result<Value, CliError> {
    let dir = root_dir.join("agent/rules");
    let mut items = Vec::new();
    for path in sorted_markdown_files(&dir)? {
        let name = path
            .file_stem()
            .and_then(|entry| entry.to_str())
            .unwrap_or_default()
            .to_string();
        let content = fs::read_to_string(&path).map_err(|err| {
            CliError::new(
                "INTERNAL_ERROR",
                format!("Failed to read {}: {err}", path.display()),
                Some("Check file permissions."),
                1,
            )
        })?;
        items.push(json!({ "name": name, "content": content }));
    }
    Ok(Value::Array(items))
}

pub fn skills_entries(root_dir: &Path) -> Result<Value, CliError> {
    let dir = root_dir.join("agent/skills");
    let mut items = Vec::new();
    for path in sorted_markdown_files(&dir)? {
        let name = path
            .file_stem()
            .and_then(|entry| entry.to_str())
            .unwrap_or_default()
            .to_string();
        let description = read_frontmatter_description(&path).unwrap_or_default();
        items.push(json!({
            "name": name,
            "description": description,
            "command": format!("agent-cli-lint skills {name}")
        }));
    }
    Ok(Value::Array(items))
}

pub fn skills_value(root_dir: &Path) -> Result<Value, CliError> {
    skills_entries(root_dir).map(|entries| json!({ "result": entries }))
}

pub fn skill_detail(root_dir: &Path, name: &str) -> Result<Value, CliError> {
    let path = root_dir.join("agent/skills").join(format!("{name}.md"));
    if !path.is_file() {
        let available = sorted_markdown_files(&root_dir.join("agent/skills"))?
            .into_iter()
            .filter_map(|entry| {
                entry
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .map(ToOwned::to_owned)
            })
            .collect::<Vec<_>>()
            .join(", ");
        return Err(CliError::new(
            "NOT_FOUND",
            format!("Skill not found: {name}"),
            Some(format!("Available: {available}")),
            20,
        ));
    }

    let description = read_frontmatter_description(&path).unwrap_or_default();
    let content = fs::read_to_string(&path).map_err(|err| {
        CliError::new(
            "INTERNAL_ERROR",
            format!("Failed to read {}: {err}", path.display()),
            Some("Check file permissions."),
            1,
        )
    })?;
    Ok(json!({
        "name": name,
        "description": description,
        "content": content
    }))
}

fn sorted_markdown_files(dir: &Path) -> Result<Vec<PathBuf>, CliError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
    }

    let mut entries = fs::read_dir(dir)
        .map_err(|err| {
            CliError::new(
                "INTERNAL_ERROR",
                format!("Failed to read {}: {err}", dir.display()),
                Some("Check directory permissions."),
                1,
            )
        })?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
        .collect::<Vec<_>>();
    entries.sort();
    Ok(entries)
}

fn read_frontmatter_description(path: &Path) -> Option<String> {
    let content = fs::read_to_string(path).ok()?;
    let mut lines = content.lines();
    if lines.next()? != "---" {
        return None;
    }

    for line in lines {
        if line == "---" {
            break;
        }
        if let Some(rest) = line.strip_prefix("description:") {
            return Some(rest.trim().trim_matches('"').to_string());
        }
    }
    None
}
