use std::collections::BTreeMap;

use serde_json::{Map, Value, json};

use crate::{AppContext, agent};

#[derive(Clone, Debug)]
pub struct CliError {
    pub code: String,
    pub message: String,
    pub suggestion: Option<String>,
    pub exit_code: i32,
}

impl CliError {
    pub fn new<S, M, H>(code: S, message: M, suggestion: Option<H>, exit_code: i32) -> Self
    where
        S: Into<String>,
        M: Into<String>,
        H: Into<String>,
    {
        Self {
            code: code.into(),
            message: message.into(),
            suggestion: suggestion.map(Into::into),
            exit_code,
        }
    }
}

pub fn print_error(error: &CliError) {
    let mut value = Map::new();
    value.insert("error".into(), Value::Bool(true));
    value.insert("code".into(), Value::String(error.code.clone()));
    value.insert("message".into(), Value::String(error.message.clone()));
    if let Some(suggestion) = &error.suggestion {
        value.insert("suggestion".into(), Value::String(suggestion.clone()));
    }
    eprintln!(
        "{}",
        serde_json::to_string(&Value::Object(value)).expect("error JSON should serialize")
    );
}

pub fn print_json(ctx: &AppContext, value: Value) -> Result<(), CliError> {
    let attached = attach_agent_context(ctx, value)?;
    let filtered = filter_fields(ctx, attached);
    println!(
        "{}",
        serde_json::to_string_pretty(&filtered).expect("JSON output should serialize")
    );
    Ok(())
}

pub fn print_list(ctx: &AppContext, value: Value) -> Result<(), CliError> {
    if ctx.wants_json_output() {
        return print_json(ctx, value);
    }

    let entries = match &value {
        Value::Array(entries) => entries.clone(),
        Value::Object(map) => map
            .get("result")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };

    for entry in &entries {
        let id = entry.get("id").and_then(Value::as_str).unwrap_or("-");
        let status = entry.get("status").and_then(Value::as_str).unwrap_or("-");
        let detail = entry
            .get("detail")
            .or_else(|| entry.get("message"))
            .and_then(Value::as_str)
            .unwrap_or("-");
        println!("  {id}  {status}  {detail}");
    }

    ctx.log(&format!("  ({} items)", entries.len()));
    Ok(())
}

fn attach_agent_context(ctx: &AppContext, value: Value) -> Result<Value, CliError> {
    let rules = agent::rules_value(&ctx.root_dir)?;
    let skills = agent::skills_entries(&ctx.root_dir)?;
    let issue = Value::String(ctx.issue_guide());

    Ok(match value {
        Value::Array(entries) => json!({
            "result": entries,
            "rules": rules,
            "skills": skills,
            "issue": issue
        }),
        Value::Object(mut map) => {
            map.insert("rules".into(), rules);
            map.insert("skills".into(), skills);
            map.insert("issue".into(), issue);
            Value::Object(map)
        }
        other => json!({
            "result": other,
            "rules": rules,
            "skills": skills,
            "issue": issue
        }),
    })
}

fn filter_fields(ctx: &AppContext, value: Value) -> Value {
    let Some(fields) = &ctx.global.fields else {
        return value;
    };

    let Value::Object(map) = value else {
        return value;
    };

    let mut filtered = BTreeMap::new();
    for field in fields {
        if let Some(entry) = map.get(field) {
            filtered.insert(field.clone(), entry.clone());
        }
    }
    Value::Object(Map::from_iter(filtered))
}
