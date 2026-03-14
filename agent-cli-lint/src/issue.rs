use std::fs;
use std::path::Path;

use chrono::Utc;
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::output::CliError;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Issue {
    pub id: String,
    #[serde(rename = "type")]
    pub issue_type: String,
    pub message: String,
    pub version: String,
    pub status: String,
    pub created: String,
    pub created_at: String,
    pub updated_at: String,
}

pub fn create_issue(
    issues_dir: &Path,
    issue_type: &str,
    message: &str,
    version: &str,
) -> Result<Issue, CliError> {
    let now = Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string();
    let id = format!(
        "iss-{}-{}",
        Utc::now().timestamp(),
        rand::thread_rng().gen_range(0..1000)
    );
    let issue = Issue {
        id: id.clone(),
        issue_type: issue_type.to_string(),
        message: message.to_string(),
        version: version.to_string(),
        status: "open".to_string(),
        created: now.clone(),
        created_at: now.clone(),
        updated_at: now,
    };

    let path = issues_dir.join(format!("{id}.json"));
    fs::write(
        &path,
        serde_json::to_string_pretty(&issue).expect("issue JSON should serialize"),
    )
    .map_err(|err| {
        CliError::new(
            "INTERNAL_ERROR",
            format!("Failed to write {}: {err}", path.display()),
            Some("Check directory permissions."),
            1,
        )
    })?;
    Ok(issue)
}

pub fn list_issues(issues_dir: &Path) -> Result<Value, CliError> {
    let mut issues = Vec::new();
    if issues_dir.is_dir() {
        let mut paths = fs::read_dir(issues_dir)
            .map_err(|err| {
                CliError::new(
                    "INTERNAL_ERROR",
                    format!("Failed to read {}: {err}", issues_dir.display()),
                    Some("Check directory permissions."),
                    1,
                )
            })?
            .filter_map(Result::ok)
            .map(|entry| entry.path())
            .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("json"))
            .collect::<Vec<_>>();
        paths.sort();

        for path in paths {
            let value = fs::read_to_string(&path)
                .ok()
                .and_then(|content| serde_json::from_str::<Value>(&content).ok())
                .unwrap_or(Value::Null);
            if !value.is_null() {
                issues.push(value);
            }
        }
    }
    Ok(Value::Array(issues))
}

pub fn show_issue(issues_dir: &Path, id: &str) -> Result<Issue, CliError> {
    let path = issues_dir.join(format!("{id}.json"));
    let content = fs::read_to_string(&path).map_err(|_| {
        CliError::new(
            "NOT_FOUND",
            format!("Issue not found: {id}"),
            Some("Use: agent-cli-lint issue list"),
            20,
        )
    })?;
    serde_json::from_str::<Issue>(&content).map_err(|err| {
        CliError::new(
            "INTERNAL_ERROR",
            format!("Failed to parse {}: {err}", path.display()),
            Some("Repair or remove the invalid issue file."),
            1,
        )
    })
}
