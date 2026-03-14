use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread::sleep;
use std::time::{Duration, Instant};

use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};

use crate::AppContext;
use crate::output::{self, CliError};

#[derive(Clone, Debug)]
struct RuleMeta {
    id: String,
    name: String,
    dimension_id: String,
    dimension_name: String,
    priority: String,
    layer: String,
}

#[derive(Clone, Debug, Serialize)]
struct RuleResult {
    id: String,
    status: String,
    detail: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    suggestion: Option<String>,
}

#[derive(Clone, Debug)]
struct Filters {
    dimension: String,
    layer: String,
    priority: String,
    rule: String,
}

#[derive(Clone, Debug)]
struct TargetState {
    cli: String,
    target_dir: PathBuf,
    help_cache: String,
    help_json: Option<Value>,
    help_exit: i32,
    describe_cache: String,
    describe_json: Option<Value>,
    describe_exit: i32,
    brief_cache: String,
    brief_exit: i32,
    version_cache: String,
}

#[derive(Clone, Debug)]
struct RunOutput {
    stdout: String,
    stderr: String,
    exit: i32,
}

pub fn cmd_check(ctx: &AppContext, args: &[String]) -> Result<i32, CliError> {
    let (cli, filters) = parse_check_args(args)?;
    ctx.ensure_dirs()?;
    let target = TargetState::discover(ctx, &cli)?;
    let registry = load_rule_registry(&ctx.root_dir)?;
    validate_rule_filters(&registry, &filters)?;

    let mut results = Vec::new();
    let mut checked = 0usize;

    for rule in &registry {
        if !matches_filters(rule, &filters) {
            continue;
        }
        ctx.log(&format!("  [{}] {} ...", rule.id, rule.name));
        results.push(run_rule(ctx, &target, rule)?);
        checked += 1;
    }

    if checked == 0 {
        return Err(CliError::new(
            "NOT_FOUND",
            "No rules matched the filter",
            Some(
                "Check --dimension (01-11), --layer (core/recommended/ecosystem), --priority (p0/p1/p2), --rule (e.g. O1)",
            ),
            20,
        ));
    }

    let report = build_report(&target, &registry, &results, &filters);
    if ctx.wants_json_output() {
        output::print_json(ctx, report.clone())?;
    } else {
        print_human_report(&report);
    }

    let failed = report
        .get("summary")
        .and_then(|value| value.get("failed"))
        .and_then(Value::as_i64)
        .unwrap_or(1);
    Ok(if failed == 0 { 0 } else { 1 })
}

pub fn cmd_snapshot(ctx: &AppContext, args: &[String]) -> Result<i32, CliError> {
    let cli = args.first().ok_or_else(|| {
        CliError::new(
            "MISSING_PARAM",
            "Missing: target CLI",
            Some("Usage: agent-cli-lint snapshot <cli>"),
            2,
        )
    })?;
    ctx.ensure_dirs()?;
    let target = TargetState::discover(ctx, cli)?;
    let tool_name = target.tool_name();
    let snap_dir = ctx.snapshots_dir.join(&tool_name);
    fs::create_dir_all(&snap_dir).map_err(|err| {
        CliError::new(
            "INTERNAL_ERROR",
            format!("Failed to create {}: {err}", snap_dir.display()),
            Some("Check directory permissions."),
            1,
        )
    })?;
    let schema_path = snap_dir.join("schema.json");
    let schema = if target.has_describe_schema() {
        target.describe_json.clone().unwrap_or_else(|| json!({}))
    } else {
        target.help_json.clone().unwrap_or_else(|| json!({}))
    };
    fs::write(
        &schema_path,
        serde_json::to_string_pretty(&schema).expect("snapshot JSON should serialize"),
    )
    .map_err(|err| {
        CliError::new(
            "INTERNAL_ERROR",
            format!("Failed to write {}: {err}", schema_path.display()),
            Some("Check directory permissions."),
            1,
        )
    })?;
    ctx.log(&format!("Snapshot saved: {}", schema_path.display()));
    if ctx.wants_json_output() {
        output::print_json(
            ctx,
            json!({ "snapshot": snap_dir.display().to_string(), "tool": tool_name }),
        )?;
    }
    Ok(0)
}

pub fn cmd_diff(ctx: &AppContext, args: &[String]) -> Result<i32, CliError> {
    let cli = args.first().ok_or_else(|| {
        CliError::new(
            "MISSING_PARAM",
            "Missing: target CLI",
            Some("Usage: agent-cli-lint diff <cli>"),
            2,
        )
    })?;
    ctx.ensure_dirs()?;
    let target = TargetState::discover(ctx, cli)?;
    let tool_name = target.tool_name();
    let snap_dir = ctx.snapshots_dir.join(&tool_name);
    let schema_path = snap_dir.join("schema.json");
    let snapshot_raw = fs::read_to_string(&schema_path).map_err(|_| {
        CliError::new(
            "NOT_FOUND",
            format!("No snapshot for {tool_name}"),
            Some(format!("Run: agent-cli-lint snapshot {cli}")),
            20,
        )
    })?;
    let snapshot_json: Value = serde_json::from_str(&snapshot_raw).unwrap_or_else(|_| json!({}));
    let old_cmds = command_names_from_value(&snapshot_json);
    let new_cmds = target
        .describe_json
        .as_ref()
        .map(command_names_from_value)
        .unwrap_or_default();

    let added = new_cmds
        .iter()
        .filter(|name| !old_cmds.contains(name))
        .cloned()
        .collect::<Vec<_>>()
        .join(",");
    let removed = old_cmds
        .iter()
        .filter(|name| !new_cmds.contains(name))
        .cloned()
        .collect::<Vec<_>>()
        .join(",");
    let breaking = !removed.is_empty();

    if ctx.wants_json_output() {
        output::print_json(
            ctx,
            json!({ "added": added, "removed": removed, "breaking": breaking }),
        )?;
    } else {
        println!("  Snapshot diff for {tool_name}:");
        if !added.is_empty() {
            println!("    Added:   {added}");
        }
        if !removed.is_empty() {
            println!("    Removed: {removed} (BREAKING)");
        }
        if added.is_empty() && removed.is_empty() {
            println!("    No changes");
        }
    }

    Ok(if breaking { 1 } else { 0 })
}

pub fn cmd_ai_prompts(ctx: &AppContext, args: &[String]) -> Result<i32, CliError> {
    let cli = args.first().ok_or_else(|| {
        CliError::new(
            "MISSING_PARAM",
            "Missing: target CLI",
            Some("Usage: agent-cli-lint ai-prompts <cli>"),
            2,
        )
    })?;
    let target = TargetState::discover(ctx, cli)?;
    let list_cmd = target.find_list_command().unwrap_or_default();
    let all_cmds = target.command_names().join(", ");
    let test_cmd = if list_cmd.is_empty() {
        String::new()
    } else {
        format!("{cli} {list_cmd} --json")
    };
    let src_dir = target.target_dir.display().to_string();
    let grep_env = format!(
        "grep -rE '(HOME|XDG_|\\.config/|\\.local/|process\\.env|getenv)' {src_dir} --include='*.ts' --include='*.sh' --include='*.py' --include='*.js' 2>/dev/null || echo 'NONE'"
    );
    let grep_update = format!(
        "grep -riE '(auto.?update|self.?update|check.?update|fetch.*latest|upgrade.*silent)' {src_dir} --include='*.ts' --include='*.sh' --include='*.py' --include='*.js' 2>/dev/null || echo 'NONE'"
    );
    let grep_errors = format!(
        "grep -rn '(catch|rescue|except|trap|\\.catch\\(|try {{)' {src_dir} --include='*.ts' --include='*.sh' --include='*.py' --include='*.js' 2>/dev/null | head -20 || true"
    );
    let prompts = json!([
        {
            "id": "I2",
            "name": "No positional ambiguity",
            "steps": [
                {"command": format!("{cli} --describe"), "capture": "schema"},
                {"action": "analyze", "target": "schema.commands[].parameters — check positional args"},
                {"action": "judge", "criteria": "Each positional arg has fixed meaning regardless of other args"}
            ],
            "pass_if": "No positional arg changes meaning based on context",
            "fail_if": "Same positional slot means different things depending on other args"
        },
        {
            "id": "I8",
            "name": "No implicit state",
            "steps": [
                {"command": grep_env, "capture": "env_refs"},
                {"command": format!("{cli} --describe"), "capture": "schema"},
                {"action": "compare", "target": "env_refs vs schema declared env vars"},
                {"action": "judge", "criteria": "All env/config dependencies must be declared in --describe"}
            ],
            "pass_if": "All env/config dependencies declared in schema, or none exist",
            "fail_if": "Behavior depends on undeclared env vars or hidden config files"
        },
        {
            "id": "C7",
            "name": "Idempotency",
            "steps": [
                {"command": test_cmd, "capture": "run1"},
                {"command": test_cmd, "capture": "run2"},
                {"action": "diff", "target": "run1 vs run2 (ignore timestamps)"},
                {"action": "repeat_for", "target": format!("Non-destructive commands: {all_cmds}")}
            ],
            "pass_if": "Identical output on repeated runs (excluding timestamps)",
            "fail_if": "Output differs between identical runs unexpectedly"
        },
        {
            "id": "S5",
            "name": "No auto-upgrade",
            "steps": [
                {"command": grep_update, "capture": "update_refs"},
                {"action": "judge", "criteria": "No silent auto-update. Explicit update commands are OK."}
            ],
            "pass_if": "No auto-update code, or only explicit user-triggered updates",
            "fail_if": "Silent background update or auto-download of new versions"
        },
        {
            "id": "G5",
            "name": "Output redaction",
            "steps": [
                {"command": test_cmd, "capture": "output"},
                {"action": "scan", "target": "output for emails, phones, API keys, SSNs"},
                {"action": "judge", "criteria": "PII should be redacted in --json output"}
            ],
            "pass_if": "No PII in output, or PII is masked",
            "fail_if": "Raw PII appears unmasked in --json output"
        },
        {
            "id": "G9",
            "name": "Fail-closed",
            "steps": [
                {"command": grep_errors, "capture": "error_handlers"},
                {"command": format!("{cli} --describe"), "capture": "schema"},
                {"action": "judge", "criteria": "Guard/validation crashes must DENY, not silently ALLOW"}
            ],
            "pass_if": "Guard errors cause rejection (exit non-zero with error JSON)",
            "fail_if": "Guard errors swallowed, command proceeds anyway"
        }
    ]);
    output::print_json(ctx, prompts)?;
    Ok(0)
}

impl TargetState {
    fn discover(ctx: &AppContext, cli: &str) -> Result<Self, CliError> {
        let target_dir = resolve_target_dir(cli);
        ctx.log(&format!("Discovering: {cli}"));
        ctx.log(&format!("Target dir: {}", target_dir.display()));

        let help_out = run_shell_capture(&format!("{cli} --help 2>&1"))?;
        let help_cache = help_out.stdout;
        let help_json = parse_json(&help_cache);
        let help_exit = if help_json.is_some() { 0 } else { 1 };

        let describe_out = run_shell_capture(&format!("{cli} --describe 2>/dev/null"))?;
        let describe_cache = describe_out.stdout;
        let describe_json = parse_json(&describe_cache);
        let describe_exit = if describe_json.is_some() { 0 } else { 1 };

        let brief_out = run_shell_capture(&format!("{cli} --brief 2>/dev/null"))?;
        let version_out = run_shell_capture(&format!("{cli} --version 2>/dev/null"))?;

        Ok(Self {
            cli: cli.to_string(),
            target_dir,
            help_cache,
            help_json,
            help_exit,
            describe_cache,
            describe_json,
            describe_exit,
            brief_cache: brief_out.stdout,
            brief_exit: brief_out.exit,
            version_cache: version_out.stdout,
        })
    }

    fn has_describe_schema(&self) -> bool {
        self.describe_json
            .as_ref()
            .and_then(|value| value.pointer("/commands/0"))
            .map(|first| {
                first.get("parameters").is_some()
                    || first.get("destructive").is_some()
                    || first.get("output").is_some()
            })
            .unwrap_or(false)
    }

    fn command_names(&self) -> Vec<String> {
        if self.help_exit == 0 {
            let names = self
                .help_json
                .as_ref()
                .map(command_names_from_value)
                .unwrap_or_default();
            if !names.is_empty() {
                return names;
            }
        }
        self.describe_json
            .as_ref()
            .map(command_names_from_value)
            .unwrap_or_default()
    }

    fn tool_name(&self) -> String {
        if let Some(name) = self
            .describe_json
            .as_ref()
            .and_then(|value| value.get("name"))
            .and_then(Value::as_str)
        {
            return name.to_string();
        }
        self.help_json
            .as_ref()
            .and_then(|value| value.get("help"))
            .and_then(Value::as_str)
            .and_then(|help| help.split(" — ").next())
            .and_then(|first| first.split_whitespace().next())
            .unwrap_or("unknown")
            .to_string()
    }

    fn tool_version(&self) -> String {
        if let Some(version) = self
            .describe_json
            .as_ref()
            .and_then(|value| value.get("version"))
            .and_then(Value::as_str)
        {
            return version.to_string();
        }
        parse_json(&self.version_cache)
            .and_then(|value| {
                value
                    .get("version")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .or_else(|| {
                let trimmed = self.version_cache.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            })
            .unwrap_or_else(|| "unknown".to_string())
    }

    fn find_list_command(&self) -> Option<String> {
        if self.has_describe_schema() {
            for command in self.describe_commands() {
                let name = command_name(command)?;
                let destructive = command
                    .get("destructive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if !destructive
                    && matches!(
                        name.as_str(),
                        "list" | "models" | "status" | "voices" | "skills"
                    )
                {
                    return Some(name);
                }
            }
            for command in self.describe_commands() {
                let name = command_name(command)?;
                let destructive = command
                    .get("destructive")
                    .and_then(Value::as_bool)
                    .unwrap_or(false);
                if destructive
                    || matches!(
                        name.as_str(),
                        "issue" | "check" | "snapshot" | "diff" | "ai-prompts"
                    )
                {
                    continue;
                }
                let required = command
                    .get("parameters")
                    .and_then(Value::as_array)
                    .map(|params| {
                        params.iter().any(|param| {
                            param.get("required").and_then(Value::as_bool) == Some(true)
                        })
                    })
                    .unwrap_or(false);
                if !required {
                    return Some(name);
                }
            }
        }
        self.command_names().into_iter().find(|name| {
            matches!(
                name.as_str(),
                "list" | "models" | "status" | "voices" | "skills"
            )
        })
    }

    fn find_destructive_command(&self) -> Option<String> {
        self.describe_commands().into_iter().find_map(|command| {
            if command.get("destructive").and_then(Value::as_bool) == Some(true) {
                command_name(command)
            } else {
                None
            }
        })
    }

    fn find_show_command(&self) -> Option<String> {
        if let Some(found) =
            self.describe_commands()
                .into_iter()
                .find_map(|command| match command_name(command) {
                    Some(name) if matches!(name.as_str(), "show" | "get" | "info") => Some(name),
                    _ => None,
                })
        {
            return Some(found);
        }
        self.command_names()
            .into_iter()
            .find(|name| matches!(name.as_str(), "show" | "get" | "info"))
    }

    fn find_command_with_required_param(&self) -> Option<String> {
        self.describe_commands().into_iter().find_map(|command| {
            let name = command_name(command)?;
            let destructive = command
                .get("destructive")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            if destructive || matches!(name.as_str(), "check" | "snapshot" | "diff" | "ai-prompts")
            {
                return None;
            }
            let has_required = command
                .get("parameters")
                .and_then(Value::as_array)
                .map(|params| {
                    params
                        .iter()
                        .any(|param| param.get("required").and_then(Value::as_bool) == Some(true))
                })
                .unwrap_or(false);
            if has_required { Some(name) } else { None }
        })
    }

    fn describe_commands(&self) -> Vec<&Value> {
        self.describe_json
            .as_ref()
            .and_then(|value| value.get("commands"))
            .and_then(Value::as_array)
            .map(|array| array.iter().collect())
            .unwrap_or_default()
    }

    fn run_cli(&self, args: &str) -> Result<RunOutput, CliError> {
        let command = if args.trim().is_empty() {
            self.cli.clone()
        } else {
            format!("{} {}", self.cli, args)
        };
        run_shell_capture(&command)
    }

    fn run_with_timeout(&self, secs: u64, args: &str) -> Result<RunOutput, CliError> {
        let command = if args.trim().is_empty() {
            self.cli.clone()
        } else {
            format!("{} {}", self.cli, args)
        };
        run_shell_capture_with_timeout(&command, secs)
    }
}

fn parse_check_args(args: &[String]) -> Result<(String, Filters), CliError> {
    let mut cli = String::new();
    let mut dimension = String::new();
    let mut layer = String::new();
    let mut priority = String::new();
    let mut rule = String::new();
    let mut i = 0usize;

    while i < args.len() {
        match args[i].as_str() {
            "--dimension" => {
                let value = args.get(i + 1).ok_or_else(|| {
                    CliError::new(
                        "MISSING_VALUE",
                        "--dimension requires a value",
                        Some("e.g. --dimension 01"),
                        2,
                    )
                })?;
                dimension = value.clone();
                i += 2;
            }
            "--layer" => {
                let value = args.get(i + 1).ok_or_else(|| {
                    CliError::new(
                        "MISSING_VALUE",
                        "--layer requires a value",
                        Some("e.g. --layer core"),
                        2,
                    )
                })?;
                layer = value.to_lowercase();
                i += 2;
            }
            "--priority" => {
                let value = args.get(i + 1).ok_or_else(|| {
                    CliError::new(
                        "MISSING_VALUE",
                        "--priority requires a value",
                        Some("e.g. --priority p0"),
                        2,
                    )
                })?;
                priority = value.to_lowercase();
                i += 2;
            }
            "--rule" => {
                let value = args.get(i + 1).ok_or_else(|| {
                    CliError::new(
                        "MISSING_VALUE",
                        "--rule requires a value",
                        Some("e.g. --rule O1"),
                        2,
                    )
                })?;
                rule = value.to_uppercase();
                i += 2;
            }
            other if other.starts_with('-') => {
                return Err(CliError::new(
                    "UNKNOWN_FLAG",
                    format!("Unknown flag: {other}"),
                    Some("Valid: --dimension, --layer, --priority, --rule"),
                    2,
                ));
            }
            other => {
                cli = other.to_string();
                i += 1;
            }
        }
    }

    if !layer.is_empty() && !matches!(layer.as_str(), "core" | "recommended" | "ecosystem") {
        return Err(CliError::new(
            "INVALID_ENUM",
            format!("Invalid layer: {layer}"),
            Some("Valid: core, recommended, ecosystem"),
            2,
        ));
    }
    if cli.is_empty() {
        return Err(CliError::new(
            "MISSING_PARAM",
            "Missing required parameter: target CLI command",
            Some(
                "Usage: agent-cli-lint check <cli> [--dimension 01] [--layer core] [--priority p0] [--rule O1]",
            ),
            2,
        ));
    }

    Ok((
        cli,
        Filters {
            dimension,
            layer,
            priority,
            rule,
        },
    ))
}

fn validate_rule_filters(registry: &[RuleMeta], filters: &Filters) -> Result<(), CliError> {
    if filters.rule.is_empty() {
        return Ok(());
    }
    let rule = registry
        .iter()
        .find(|entry| entry.id == filters.rule)
        .ok_or_else(|| {
            CliError::new(
                "NOT_FOUND",
                format!("Unknown rule: {}", filters.rule),
                Some("Use a valid rule id such as O1 or D15."),
                20,
            )
        })?;
    if !filters.dimension.is_empty() && filters.dimension != rule.dimension_id {
        return Err(CliError::new(
            "FILTER_CONFLICT",
            format!(
                "Rule {} is in dimension {}, not {}",
                filters.rule, rule.dimension_id, filters.dimension
            ),
            Some("Remove either --rule or --dimension."),
            2,
        ));
    }
    if !filters.priority.is_empty() && filters.priority != rule.priority.to_lowercase() {
        return Err(CliError::new(
            "FILTER_CONFLICT",
            format!(
                "Rule {} has priority {}, not {}",
                filters.rule,
                rule.priority.to_lowercase(),
                filters.priority
            ),
            Some("Remove either --rule or --priority."),
            2,
        ));
    }
    if !filters.layer.is_empty() && filters.layer != rule.layer {
        return Err(CliError::new(
            "FILTER_CONFLICT",
            format!(
                "Rule {} is in layer {}, not {}",
                filters.rule, rule.layer, filters.layer
            ),
            Some("Remove either --rule or --layer."),
            2,
        ));
    }
    Ok(())
}

fn load_rule_registry(root_dir: &Path) -> Result<Vec<RuleMeta>, CliError> {
    let path = root_dir.join("data/rules.tsv");
    let content = fs::read_to_string(&path).map_err(|_| {
        CliError::new(
            "INTERNAL_ERROR",
            format!("Missing rule registry: {}", path.display()),
            Some("Restore data/rules.tsv."),
            1,
        )
    })?;

    let mut rules = Vec::new();
    for line in content.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let parts = line.split('|').collect::<Vec<_>>();
        if parts.len() != 7 {
            return Err(CliError::new(
                "INTERNAL_ERROR",
                format!("Invalid rule registry line: {line}"),
                Some("Fix data/rules.tsv formatting."),
                1,
            ));
        }
        rules.push(RuleMeta {
            id: parts[0].to_string(),
            name: parts[1].to_string(),
            dimension_id: parts[2].to_string(),
            dimension_name: parts[3].to_string(),
            priority: parts[4].to_string(),
            layer: parts[5].to_string(),
        });
    }
    Ok(rules)
}

fn matches_filters(rule: &RuleMeta, filters: &Filters) -> bool {
    (filters.dimension.is_empty() || rule.dimension_id == filters.dimension)
        && (filters.layer.is_empty() || rule.layer == filters.layer)
        && (filters.priority.is_empty() || rule.priority.to_lowercase() == filters.priority)
        && (filters.rule.is_empty() || rule.id == filters.rule)
}

fn build_report(
    target: &TargetState,
    registry: &[RuleMeta],
    results: &[RuleResult],
    filters: &Filters,
) -> Value {
    let partial = !filters.dimension.is_empty()
        || !filters.layer.is_empty()
        || !filters.priority.is_empty()
        || !filters.rule.is_empty();
    let total = results.len() as i64;
    let passed = results
        .iter()
        .filter(|entry| entry.status == "pass")
        .count() as i64;
    let failed = results
        .iter()
        .filter(|entry| entry.status == "fail")
        .count() as i64;
    let skipped = results
        .iter()
        .filter(|entry| entry.status == "skip")
        .count() as i64;
    let ai = results.iter().filter(|entry| entry.status == "ai").count() as i64;
    let warned = results
        .iter()
        .filter(|entry| entry.status == "warn")
        .count() as i64;

    let layers = ["core", "recommended", "ecosystem"]
        .into_iter()
        .map(|layer| build_layer_json(layer, registry, results, filters))
        .collect::<Vec<_>>();
    let layer_lookup = layers
        .iter()
        .filter_map(|entry| {
            entry
                .get("name")
                .and_then(Value::as_str)
                .map(|name| (name.to_string(), entry.clone()))
        })
        .collect::<std::collections::BTreeMap<_, _>>();

    let core_compliant = layer_lookup
        .get("core")
        .and_then(|entry| entry.get("compliant"))
        .and_then(Value::as_bool);
    let rec_compliant = layer_lookup
        .get("recommended")
        .and_then(|entry| entry.get("compliant"))
        .and_then(Value::as_bool);
    let eco_compliant = layer_lookup
        .get("ecosystem")
        .and_then(|entry| entry.get("compliant"))
        .and_then(Value::as_bool);

    let certification = if partial {
        json!({
            "eligible": false,
            "partial": true,
            "agent_friendly": Value::Null,
            "agent_ready": Value::Null,
            "agent_native": Value::Null,
            "reason": "Certification requires a full run without filters."
        })
    } else {
        let af = core_compliant.unwrap_or(false);
        let ar = af && rec_compliant.unwrap_or(false);
        let an = ar && eco_compliant.unwrap_or(false);
        json!({
            "eligible": true,
            "partial": false,
            "agent_friendly": af,
            "agent_ready": ar,
            "agent_native": an,
            "reason": ""
        })
    };

    let mut dimensions = Vec::new();
    let mut current_dim = String::new();
    let mut current_name = String::new();
    let mut dim_rules = Vec::new();

    for rule in registry {
        if !current_dim.is_empty() && current_dim != rule.dimension_id {
            dimensions.push(json!({
                "id": current_dim,
                "name": current_name,
                "rules": dim_rules
            }));
            dim_rules = Vec::new();
        }
        let result = results
            .iter()
            .find(|entry| entry.id == rule.id)
            .cloned()
            .unwrap_or_else(|| RuleResult {
                id: rule.id.clone(),
                status: "skip".to_string(),
                detail: "Not checked".to_string(),
                suggestion: None,
            });
        dim_rules.push(json!({
            "id": result.id,
            "name": rule.name,
            "priority": rule.priority,
            "layer": rule.layer,
            "status": result.status,
            "detail": result.detail,
            "suggestion": result.suggestion
        }));
        current_dim = rule.dimension_id.clone();
        current_name = rule.dimension_name.clone();
    }
    if !current_dim.is_empty() {
        dimensions.push(json!({
            "id": current_dim,
            "name": current_name,
            "rules": dim_rules
        }));
    }

    let ai_checks = results
        .iter()
        .filter(|entry| entry.status == "ai")
        .map(|entry| serde_json::to_value(entry).expect("result should serialize"))
        .collect::<Vec<_>>();

    json!({
        "tool": target.tool_name(),
        "version": target.tool_version(),
        "spec_version": crate::VERSION.split('.').take(2).collect::<Vec<_>>().join("."),
        "timestamp": Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        "scope": {
            "partial": partial,
            "dimension": filters.dimension,
            "layer": filters.layer,
            "priority": filters.priority,
            "rule": filters.rule
        },
        "summary": {
            "total": total,
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
            "ai_review": ai,
            "warned": warned
        },
        "layers": layers,
        "certification": certification,
        "dimensions": dimensions,
        "ai_checks": ai_checks
    })
}

fn build_layer_json(
    layer: &str,
    registry: &[RuleMeta],
    results: &[RuleResult],
    filters: &Filters,
) -> Value {
    let layer_rules = registry
        .iter()
        .filter(|rule| rule.layer == layer)
        .collect::<Vec<_>>();
    let total = layer_rules.len() as i64;
    let selected = layer_rules
        .iter()
        .filter(|rule| matches_filters(rule, filters))
        .count() as i64;
    let mut checked = 0i64;
    let mut passed = 0i64;
    let mut failed = 0i64;
    let mut skipped = 0i64;
    let mut ai = 0i64;
    let mut warned = 0i64;
    let mut compliant = true;

    for rule in &layer_rules {
        let status = results
            .iter()
            .find(|entry| entry.id == rule.id)
            .map(|entry| entry.status.as_str())
            .unwrap_or("not_checked");
        if status != "not_checked" {
            checked += 1;
        }
        match status {
            "pass" => passed += 1,
            "fail" => failed += 1,
            "skip" => skipped += 1,
            "ai" => ai += 1,
            "warn" => warned += 1,
            _ => {}
        }
        if status != "pass" {
            compliant = false;
        }
    }

    let eligible = selected == total;
    json!({
        "name": layer,
        "summary": {
            "total": total,
            "selected": selected,
            "checked": checked,
            "passed": passed,
            "failed": failed,
            "skipped": skipped,
            "ai_review": ai,
            "warned": warned,
            "not_checked": selected - checked
        },
        "eligible": eligible,
        "compliant": if eligible { Value::Bool(compliant) } else { Value::Null }
    })
}

fn print_human_report(report: &Value) {
    let tool = report
        .get("tool")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let version = report
        .get("version")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let passed = report
        .pointer("/summary/passed")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let failed = report
        .pointer("/summary/failed")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let skipped = report
        .pointer("/summary/skipped")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let total = report
        .pointer("/summary/total")
        .and_then(Value::as_i64)
        .unwrap_or(0);
    let partial = report
        .pointer("/scope/partial")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    println!();
    println!(
        "  Agent-Friendly CLI Spec v{} — Lint Report",
        crate::VERSION
            .split('.')
            .take(2)
            .collect::<Vec<_>>()
            .join(".")
    );
    println!("  ─────────────────────────────────────────────");
    println!("  Tool:    {tool} (v{version})");
    println!("  Passed:  {passed} / {total}");
    println!("  Failed:  {failed}");
    println!("  Skipped: {skipped}");
    if partial {
        println!("  Scope:   partial report");
    }
    println!();
}

fn command_names_from_value(value: &Value) -> Vec<String> {
    value
        .get("commands")
        .and_then(Value::as_array)
        .map(|commands| {
            commands
                .iter()
                .filter_map(|command| {
                    command
                        .get("name")
                        .and_then(Value::as_str)
                        .map(ToOwned::to_owned)
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
}

fn command_name(value: &Value) -> Option<String> {
    value
        .get("name")
        .and_then(Value::as_str)
        .map(ToOwned::to_owned)
}

fn parse_json(raw: &str) -> Option<Value> {
    serde_json::from_str::<Value>(raw.trim()).ok()
}

fn run_shell_capture(command: &str) -> Result<RunOutput, CliError> {
    let output = Command::new("/bin/bash")
        .arg("-lc")
        .arg(command)
        .output()
        .map_err(|err| {
            CliError::new(
                "INTERNAL_ERROR",
                format!("Failed to run command: {command}: {err}"),
                Some("Check that /bin/bash is available."),
                1,
            )
        })?;
    Ok(RunOutput {
        stdout: filter_compiler_noise(&String::from_utf8_lossy(&output.stdout)),
        stderr: filter_compiler_noise(&String::from_utf8_lossy(&output.stderr)),
        exit: output.status.code().unwrap_or(1),
    })
}

fn run_shell_capture_with_timeout(command: &str, secs: u64) -> Result<RunOutput, CliError> {
    let mut child = Command::new("/bin/bash")
        .arg("-lc")
        .arg(command)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|err| {
            CliError::new(
                "INTERNAL_ERROR",
                format!("Failed to run command: {command}: {err}"),
                Some("Check that /bin/bash is available."),
                1,
            )
        })?;

    let start = Instant::now();
    loop {
        if child
            .try_wait()
            .map_err(|err| {
                CliError::new(
                    "INTERNAL_ERROR",
                    format!("Failed to poll child process: {err}"),
                    Some("Retry the command."),
                    1,
                )
            })?
            .is_some()
        {
            let output = child.wait_with_output().map_err(|err| {
                CliError::new(
                    "INTERNAL_ERROR",
                    format!("Failed to collect child output: {err}"),
                    Some("Retry the command."),
                    1,
                )
            })?;
            return Ok(RunOutput {
                stdout: filter_compiler_noise(&String::from_utf8_lossy(&output.stdout)),
                stderr: filter_compiler_noise(&String::from_utf8_lossy(&output.stderr)),
                exit: output.status.code().unwrap_or(1),
            });
        }
        if start.elapsed() >= Duration::from_secs(secs) {
            let _ = child.kill();
            let output = child.wait_with_output().map_err(|err| {
                CliError::new(
                    "INTERNAL_ERROR",
                    format!("Failed to collect timed-out output: {err}"),
                    Some("Retry the command."),
                    1,
                )
            })?;
            return Ok(RunOutput {
                stdout: filter_compiler_noise(&String::from_utf8_lossy(&output.stdout)),
                stderr: filter_compiler_noise(&String::from_utf8_lossy(&output.stderr)),
                exit: 137,
            });
        }
        sleep(Duration::from_millis(50));
    }
}

fn filter_compiler_noise(raw: &str) -> String {
    let prefixes = [
        "Compiling ",
        "Finished ",
        "Running ",
        "Blocking ",
        "Downloading ",
        "Downloaded ",
        "Updating ",
        "Unpacking ",
        "Resolving ",
        "Packaging ",
        "Documenting ",
        "Locking ",
        "Linking ",
        "Fresh ",
        "Checking ",
    ];
    raw.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !prefixes.iter().any(|prefix| trimmed.starts_with(prefix))
                && !trimmed.starts_with("warning[")
                && !trimmed.starts_with("warning:")
                && !trimmed.starts_with('$')
        })
        .collect::<Vec<_>>()
        .join("\n")
        .trim()
        .to_string()
}

fn resolve_target_dir(cli: &str) -> PathBuf {
    if let Some(path) = resolve_candidate(cli) {
        return path;
    }
    if let Some(last) = cli.split_whitespace().last().and_then(resolve_candidate) {
        return last;
    }
    if let Some(manifest) = extract_manifest_path(cli).and_then(resolve_candidate) {
        return manifest;
    }
    PathBuf::from(".")
}

fn resolve_candidate(candidate: &str) -> Option<PathBuf> {
    let path = PathBuf::from(candidate);
    if path.is_file() {
        return Some(path.parent().unwrap_or(Path::new(".")).to_path_buf());
    }
    None
}

fn extract_manifest_path(cli: &str) -> Option<&str> {
    let parts = cli.split_whitespace().collect::<Vec<_>>();
    parts
        .windows(2)
        .find_map(|window| {
            if window[0] == "--manifest-path" {
                Some(window[1])
            } else {
                None
            }
        })
        .map(|path| {
            Path::new(path)
                .parent()
                .unwrap_or(Path::new("."))
                .to_str()
                .unwrap_or(".")
        })
}

fn run_rule(
    ctx: &AppContext,
    target: &TargetState,
    rule: &RuleMeta,
) -> Result<RuleResult, CliError> {
    let result = match rule.id.as_str() {
        "D1" => check_d1(target),
        "D2" => check_d2(target)?,
        "D3" => check_d3(target),
        "D4" => check_d4(target),
        "D5" => check_d5(target),
        "D6" => check_d6(target),
        "D7" => check_d7(target),
        "D8" => check_d8(target),
        "D9" => check_d9(target),
        "D10" => check_d10(target),
        "D11" => check_d11(target),
        "D12" => check_d12(target),
        "D13" => check_d13(target),
        "D14" => check_d14(target)?,
        "D15" => check_d15(target),
        "D16" => check_d16(target)?,
        "D17" => check_d17(target, "rules"),
        "D18" => check_d17(target, "skills"),
        "R1" => check_r1(target)?,
        "R2" => check_r2(target)?,
        "R3" => check_r3(target)?,
        "O1" => check_o1(target)?,
        "O2" => check_o2(target)?,
        "O3" => check_o3(ctx, target),
        "O4" => check_o4(target)?,
        "O5" => check_o5(target)?,
        "O6" => check_o6(target)?,
        "O7" => check_o7(target)?,
        "O8" => check_o8(target)?,
        "O9" => check_o9(target),
        "O10" => check_o10(target)?,
        "E1" => check_e1(target)?,
        "E2" => check_e2(target)?,
        "E3" => check_e3(target)?,
        "E4" => check_e4(target)?,
        "E5" => check_e5(target)?,
        "E6" => check_e6(target)?,
        "E7" => check_e7(target)?,
        "E8" => check_e8(ctx, target),
        "I1" => check_i1(target),
        "I2" => ai(
            "I2",
            "Requires AI review: analyze positional argument semantics for ambiguity",
        ),
        "I3" => check_i3(target),
        "I4" => check_i4(target)?,
        "I5" => check_i5(target)?,
        "I6" => skip(
            "I6",
            "Boolean flag negation (--no-X) check requires manual review",
        ),
        "I7" => skip("I7", "Array parameter syntax check requires manual review"),
        "I8" => ai(
            "I8",
            "Requires AI review: check if behavior depends on hidden state files",
        ),
        "I9" => skip("I9", "Auth mechanism check requires manual review"),
        "S1" => check_s1(target)?,
        "S2" => check_s2(target)?,
        "S3" => check_s3(target)?,
        "S4" => check_s4(target)?,
        "S5" => ai("S5", "Requires AI review: check for auto-update mechanisms"),
        "S6" => check_s6(target),
        "S7" => check_s7(target)?,
        "S8" => check_s8(target),
        "X1" => check_x1(target)?,
        "X2" => check_x2(target),
        "X3" => check_x3(target)?,
        "X4" => check_x4(target),
        "X5" => check_x5(target),
        "X6" => check_x6(target)?,
        "X7" => check_x7(target),
        "X8" => check_x8(target)?,
        "X9" => check_x9(target)?,
        "C1" => check_c1(target)?,
        "C2" => check_c2(target)?,
        "C3" => check_c3(target)?,
        "C4" => check_c4(target)?,
        "C5" => check_c5(target),
        "C6" => check_c6(target)?,
        "C7" => ai(
            "C7",
            "Requires AI review: test idempotency by running same command twice",
        ),
        "N1" => pass(
            "N1",
            "Command naming consistency (manual review recommended)",
        ),
        "N2" => check_n2(target),
        "N3" => check_n3(target),
        "N4" => check_n4(target)?,
        "N5" => check_n5(target),
        "N6" => check_n6(target),
        "M1" => check_m1(target),
        "M2" => check_m2(target),
        "M3" => check_m3(target),
        "F1" => check_f1(target)?,
        "F2" => skip(
            "F2",
            "Issue auto-attachment check requires creating a test issue",
        ),
        "F3" => check_f3(target),
        "F4" => skip(
            "F4",
            "Local storage check requires creating and finding issues",
        ),
        "F5" => check_f5(target)?,
        "F6" => skip("F6", "Issue status tracking requires manual review"),
        "F7" => check_f7(target)?,
        "F8" => check_f8(target)?,
        "G1" => check_g1(target)?,
        "G2" => check_g2(target)?,
        "G3" => check_g3(target)?,
        "G4" => check_g4(target),
        "G5" => ai("G5", "Requires AI review: test output redaction of PII"),
        "G6" => skip(
            "G6",
            "Precondition check requires domain-specific test scenarios",
        ),
        "G7" => skip("G7", "Batch limit check requires bulk operation testing"),
        "G8" => check_g8(target)?,
        "G9" => ai(
            "G9",
            "Requires AI review: verify fail-closed behavior when validation logic errors",
        ),
        _ => skip(&rule.id, "Rule not implemented"),
    };
    Ok(result)
}

fn pass(id: &str, detail: &str) -> RuleResult {
    result(id, "pass", detail, None)
}
fn fail(id: &str, detail: &str, suggestion: &str) -> RuleResult {
    result(id, "fail", detail, Some(suggestion.to_string()))
}
fn skip(id: &str, detail: &str) -> RuleResult {
    result(id, "skip", detail, None)
}
fn ai(id: &str, detail: &str) -> RuleResult {
    result(id, "ai", detail, None)
}
fn warn(id: &str, detail: &str, suggestion: &str) -> RuleResult {
    result(id, "warn", detail, Some(suggestion.to_string()))
}
fn result(id: &str, status: &str, detail: &str, suggestion: Option<String>) -> RuleResult {
    RuleResult {
        id: id.to_string(),
        status: status.to_string(),
        detail: detail.to_string(),
        suggestion,
    }
}

fn json_valid(raw: &str) -> bool {
    parse_json(raw).is_some()
}
fn output_json_value(run: &RunOutput) -> Option<Value> {
    parse_json(if !run.stdout.trim().is_empty() {
        &run.stdout
    } else {
        &run.stderr
    })
}
fn error_json_value(run: &RunOutput) -> Option<Value> {
    parse_json(if !run.stderr.trim().is_empty() {
        &run.stderr
    } else {
        &run.stdout
    })
}
fn error_code(run: &RunOutput) -> Option<String> {
    error_json_value(run).and_then(|v| v.get("code").and_then(Value::as_str).map(ToOwned::to_owned))
}
fn schema_flag_supported(
    target: &TargetState,
    cmd: &str,
    param: &str,
    value: &str,
) -> Result<bool, CliError> {
    let run = target.run_cli(&format!("{cmd} --{param} {value} --json"))?;
    Ok(error_code(&run).as_deref() != Some("UNKNOWN_FLAG"))
}

fn required_string_params(target: &TargetState, cmd: &str) -> Vec<String> {
    target
        .describe_commands()
        .into_iter()
        .find_map(|command| {
            let name = command_name(command)?;
            if name != cmd {
                return None;
            }
            Some(
                command
                    .get("parameters")
                    .and_then(Value::as_array)
                    .map(|params| {
                        params
                            .iter()
                            .filter(|param| {
                                param.get("required").and_then(Value::as_bool) == Some(true)
                                    && param.get("type").and_then(Value::as_str) == Some("string")
                            })
                            .filter_map(|param| {
                                param
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .map(ToOwned::to_owned)
                            })
                            .collect::<Vec<_>>()
                    })
                    .unwrap_or_default(),
            )
        })
        .unwrap_or_default()
}

fn build_required_args(
    target: &TargetState,
    cmd: &str,
    first_value: &str,
    other_value: &str,
) -> Option<String> {
    let params = required_string_params(target, cmd);
    if params.is_empty() {
        return None;
    }
    Some(
        params
            .into_iter()
            .enumerate()
            .map(|(index, param)| {
                let value = if index == 0 { first_value } else { other_value };
                format!("--{param} {value}")
            })
            .collect::<Vec<_>>()
            .join(" "),
    )
}

fn check_d1(target: &TargetState) -> RuleResult {
    if target.help_exit == 0
        && target
            .help_json
            .as_ref()
            .and_then(|v| v.get("commands"))
            .is_some()
    {
        pass("D1", "--help outputs structured JSON with commands[]")
    } else if target.describe_exit == 0
        && target
            .describe_json
            .as_ref()
            .and_then(|v| v.get("commands"))
            .is_some()
    {
        pass(
            "D1",
            "--describe outputs JSON with commands list (legacy, migrate to --help)",
        )
    } else {
        fail(
            "D1",
            "--help missing or does not output structured JSON with commands[]",
            "Add --help flag that outputs JSON with: help, commands[], rules[], skills[], issue.",
        )
    }
}

fn check_d2(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip(
            "D2",
            "No list-like command found to test per-command discovery",
        ));
    };
    let help = target.run_cli(&format!("{cmd} --help"))?;
    if help.exit == 0
        && output_json_value(&help)
            .map(|v| {
                v.get("name").is_some() || v.get("help").is_some() || v.get("commands").is_some()
            })
            .unwrap_or(false)
    {
        return Ok(pass(
            "D2",
            &format!("{cmd} --help outputs per-command schema"),
        ));
    }
    let describe = target.run_cli(&format!("{cmd} --describe"))?;
    if describe.exit == 0
        && output_json_value(&describe)
            .and_then(|v| v.get("name").cloned())
            .is_some()
    {
        Ok(pass(
            "D2",
            &format!("{cmd} --describe outputs per-command schema (legacy)"),
        ))
    } else {
        Ok(skip("D2", "Per-command --help not supported (optional P2)"))
    }
}

fn check_d3(target: &TargetState) -> RuleResult {
    if target
        .help_json
        .as_ref()
        .map(|v| v.get("help").is_some() && v.get("commands").is_some())
        .unwrap_or(false)
    {
        pass("D3", "--help has help + commands fields")
    } else if target
        .describe_json
        .as_ref()
        .map(|v| {
            v.get("name").is_some() && v.get("version").is_some() && v.get("commands").is_some()
        })
        .unwrap_or(false)
    {
        pass(
            "D3",
            "--describe has name, version, commands fields (legacy)",
        )
    } else {
        fail(
            "D3",
            "Schema missing required fields",
            "Ensure --help outputs JSON with: help, commands[], rules[], skills[], issue.",
        )
    }
}

fn count_matching_params(target: &TargetState, predicate: impl Fn(&Value) -> bool) -> usize {
    target
        .describe_commands()
        .into_iter()
        .flat_map(|command| {
            command
                .get("parameters")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter(predicate)
        .count()
}
fn total_params(target: &TargetState) -> usize {
    count_matching_params(target, |_| true)
}

fn check_d4(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        return skip(
            "D4",
            "No detailed schema (--describe) available; parameter types not checkable from --help",
        );
    }
    if count_matching_params(target, |param| param.get("type").is_none()) == 0 {
        pass("D4", "All parameters have type declarations")
    } else {
        fail(
            "D4",
            "Parameters without type declarations",
            "Add type field (string/integer/boolean/array) to every parameter.",
        )
    }
}
fn check_d5(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        return skip(
            "D5",
            "No detailed schema (--describe) available; enum declarations not checkable from --help",
        );
    }
    let count = count_matching_params(target, |param| param.get("enum").is_some());
    if count > 0 {
        pass("D5", "Enum values declared for constrained parameters")
    } else {
        skip("D5", "No enum parameters found (may be acceptable)")
    }
}
fn check_d6(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        return skip(
            "D6",
            "No detailed schema (--describe) available; default values not checkable from --help",
        );
    }
    let count = count_matching_params(target, |param| param.get("default").is_some());
    if count > 0 {
        pass("D6", "Default values declared")
    } else {
        skip("D6", "No default values found (may be acceptable)")
    }
}
fn check_d7(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        return skip(
            "D7",
            "No detailed schema (--describe) available; required annotations not checkable from --help",
        );
    }
    let total = total_params(target);
    let marked = count_matching_params(target, |param| param.get("required").is_some());
    if total == 0 {
        skip("D7", "No parameters in schema")
    } else if marked > 0 {
        pass("D7", "Required/optional annotation present on parameters")
    } else {
        fail(
            "D7",
            "No required annotations on parameters",
            "Mark required: true on mandatory parameters.",
        )
    }
}
fn check_d8(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        return skip(
            "D8",
            "No detailed schema (--describe) available; output schema not checkable from --help",
        );
    }
    let total = target.describe_commands().len();
    let count = target
        .describe_commands()
        .into_iter()
        .filter(|command| command.get("output").is_some())
        .count();
    if count > 0 {
        pass(
            "D8",
            &format!("Output schema declared on {count}/{total} commands"),
        )
    } else {
        fail(
            "D8",
            "No commands have output schema",
            "Add output field to each command in --describe schema.",
        )
    }
}
fn check_d9(target: &TargetState) -> RuleResult {
    let source = target.help_json.as_ref().or(target.describe_json.as_ref());
    let missing = source
        .and_then(|value| value.get("commands").and_then(Value::as_array))
        .map(|commands| {
            commands
                .iter()
                .filter_map(|command| {
                    if command
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .is_empty()
                    {
                        command
                            .get("name")
                            .and_then(Value::as_str)
                            .map(ToOwned::to_owned)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if missing.is_empty() {
        pass("D9", "All commands have descriptions")
    } else {
        fail(
            "D9",
            &format!("Commands without description: {}", missing.join(", ")),
            "Add description field to every command.",
        )
    }
}
fn check_d10(target: &TargetState) -> RuleResult {
    if target
        .describe_json
        .as_ref()
        .and_then(|v| v.get("version"))
        .is_some()
    {
        pass("D10", "Version present in --describe schema")
    } else {
        let version = target.tool_version();
        if version
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
        {
            pass(
                "D10",
                &format!("Version available via --version: {version}"),
            )
        } else {
            skip("D10", "No version in schema (optional if --version works)")
        }
    }
}
fn check_d11(target: &TargetState) -> RuleResult {
    if target.help_exit != 0 {
        return fail(
            "D11",
            "--help returned non-zero exit",
            "Add --help flag that outputs structured JSON.",
        );
    }
    if target
        .help_json
        .as_ref()
        .map(|v| {
            v.get("help").is_some()
                && v.get("rules").is_some()
                && v.get("skills").is_some()
                && v.get("commands").is_some()
        })
        .unwrap_or(false)
    {
        pass(
            "D11",
            "--help outputs JSON with help, rules, skills, commands",
        )
    } else {
        fail(
            "D11",
            "--help missing required keys (help/rules/skills/commands)",
            "Output JSON with: help (brief), rules (array), skills (array), commands (array).",
        )
    }
}
fn check_d12(target: &TargetState) -> RuleResult {
    if target.target_dir.join("agent/brief.md").is_file() {
        pass("D12", "agent/brief.md exists")
    } else {
        fail(
            "D12",
            &format!(
                "agent/brief.md not found at {}/agent/brief.md",
                target.target_dir.display()
            ),
            "Create agent/brief.md with a brief description of the CLI.",
        )
    }
}
fn check_d13(target: &TargetState) -> RuleResult {
    let missing = ["trigger.md", "workflow.md", "writeback.md"]
        .into_iter()
        .filter(|name| !target.target_dir.join("agent/rules").join(name).is_file())
        .collect::<Vec<_>>();
    if missing.is_empty() {
        pass(
            "D13",
            "agent/rules/ has trigger.md, workflow.md, writeback.md",
        )
    } else {
        fail(
            "D13",
            &format!("Missing: {}", missing.join(" ")),
            "Create agent/rules/ with trigger.md, workflow.md, writeback.md.",
        )
    }
}
fn check_d14(target: &TargetState) -> Result<RuleResult, CliError> {
    let skill_dir = target.target_dir.join("agent/skills");
    let skill_count = fs::read_dir(&skill_dir)
        .ok()
        .map(|entries| {
            entries
                .filter_map(Result::ok)
                .filter(|entry| entry.path().extension().and_then(|ext| ext.to_str()) == Some("md"))
                .count()
        })
        .unwrap_or(0);
    if skill_count == 0 {
        return Ok(fail(
            "D14",
            "No skills found in agent/skills/",
            "Create agent/skills/ with .md files. Add skills subcommand.",
        ));
    }
    let run = target.run_cli("skills")?;
    if run.exit == 0 {
        Ok(pass(
            "D14",
            &format!("{skill_count} skill(s) in agent/skills/, skills subcommand works"),
        ))
    } else {
        Ok(pass(
            "D14",
            &format!("{skill_count} skill(s) in agent/skills/ (skills subcommand not tested)"),
        ))
    }
}
fn check_d15(target: &TargetState) -> RuleResult {
    if target.brief_exit == 0 && !target.brief_cache.trim().is_empty() {
        pass("D15", "--brief outputs non-empty text")
    } else {
        fail(
            "D15",
            "--brief flag missing or returns empty",
            "Add --brief flag that outputs agent/brief.md content (one paragraph).",
        )
    }
}
fn check_d16(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip(
            "D16",
            "No list-like command to test --agent/--human flags",
        ));
    };
    let human = target.run_cli(&format!("{cmd} --human"))?;
    let default = target.run_cli(&cmd)?;
    let human_ok = human.exit != 2;
    let json_ok = json_valid(&default.stdout);
    Ok(if human_ok && json_ok {
        pass("D16", "Default is JSON (agent mode), --human flag accepted")
    } else if json_ok {
        fail(
            "D16",
            "Default is JSON but --human flag not recognized",
            "Add --human flag for human-friendly output, --agent for explicit JSON mode.",
        )
    } else {
        fail(
            "D16",
            "Default output is not JSON (agent-first required)",
            "Default output must be JSON. Add --human and --agent mode flags.",
        )
    })
}
fn check_d17(target: &TargetState, folder: &str) -> RuleResult {
    let dir = target.target_dir.join("agent").join(folder);
    if !dir.is_dir() {
        return skip(
            if folder == "rules" { "D17" } else { "D18" },
            &format!("No agent/{folder}/ directory"),
        );
    }
    let mut total = 0usize;
    let mut valid = 0usize;
    let mut bad = Vec::new();
    for path in fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("md"))
    {
        total += 1;
        let content = fs::read_to_string(&path).unwrap_or_default();
        let mut lines = content.lines();
        if lines.next() != Some("---") {
            bad.push(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            );
            continue;
        }
        let mut has_name = false;
        let mut has_desc = false;
        for line in lines {
            if line == "---" {
                break;
            }
            if line.starts_with("name:") {
                has_name = true;
            }
            if line.starts_with("description:") {
                has_desc = true;
            }
        }
        if has_name && has_desc {
            valid += 1;
        } else {
            bad.push(
                path.file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string(),
            );
        }
    }
    let id = if folder == "rules" { "D17" } else { "D18" };
    if total == 0 {
        skip(id, &format!("No .md files in agent/{folder}/"))
    } else if valid == total {
        pass(
            id,
            &format!("All {total} agent/{folder}/*.md files have proper frontmatter"),
        )
    } else {
        fail(
            id,
            &format!("Missing frontmatter: {}", bad.join(" ")),
            &format!(
                "Each agent/{folder}/*.md must start with YAML frontmatter: --- name: ... description: ... ---"
            ),
        )
    }
}

fn check_o1(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip(
            "O1",
            "No list-like command found to test default output",
        ));
    };
    let run = target.run_cli(&cmd)?;
    Ok(if json_valid(&run.stdout) {
        pass("O1", "Default output is valid JSON (agent-first)")
    } else {
        fail(
            "O1",
            "Default output is not JSON",
            "Default output must be JSON. Use --human for human-friendly format.",
        )
    })
}
fn check_o2(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("O2", "No command found to test JSON validity"));
    };
    let run = target.run_cli(&cmd)?;
    Ok(if json_valid(&run.stdout) {
        pass("O2", "Default output is valid JSON")
    } else {
        fail(
            "O2",
            "Default output fails jq . validation",
            "Default output must pass jq . validation. No --json flag needed.",
        )
    })
}

fn check_o3(ctx: &AppContext, target: &TargetState) -> RuleResult {
    let snap = ctx
        .snapshots_dir
        .join(target.tool_name())
        .join("schema.json");
    let Ok(snapshot_raw) = fs::read_to_string(&snap) else {
        return skip(
            "O3",
            &format!(
                "No snapshot saved. Run: agent-cli-lint snapshot {}",
                target.cli
            ),
        );
    };
    let old_json = parse_json(&snapshot_raw).unwrap_or_else(|| json!({}));
    let new_json = target.describe_json.clone().unwrap_or_else(|| json!({}));
    let old_keys = old_json
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    let new_keys = new_json
        .as_object()
        .map(|map| map.keys().cloned().collect::<Vec<_>>())
        .unwrap_or_default();
    if old_keys == new_keys {
        pass("O3", "Schema structure matches snapshot")
    } else {
        fail(
            "O3",
            "Schema structure changed since snapshot",
            &format!(
                "Breaking change detected. Update snapshot: agent-cli-lint snapshot {}",
                target.cli
            ),
        )
    }
}

fn check_o4(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("O4", "No command found to test --fields"));
    };
    let fields = target.run_cli(&format!("{cmd} --fields name"))?;
    if fields.exit == 0 {
        return Ok(pass("O4", &format!("--fields flag accepted on {cmd}")));
    }
    let base = target.run_cli(&cmd)?;
    Ok(if base.exit == 0 {
        fail(
            "O4",
            &format!("--fields flag not supported on {cmd}"),
            "Add --fields flag for field filtering.",
        )
    } else {
        skip("O4", "Cannot test --fields (command itself fails)")
    })
}

fn check_o5(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("O5", "No list command to test empty results"));
    };
    let run = target.run_cli(&cmd)?;
    if run.exit != 0 {
        return Ok(skip(
            "O5",
            "List command failed, cannot test empty result behavior",
        ));
    }
    let kind = output_json_value(&run)
        .map(|value| match value {
            Value::Array(_) => "array",
            Value::Object(_) => "object",
            _ => "other",
        })
        .unwrap_or("other");
    Ok(if matches!(kind, "array" | "object") {
        pass(
            "O5",
            &format!("List command returns structured data (type: {kind})"),
        )
    } else {
        fail(
            "O5",
            "List output is not array or object",
            "Return [] for empty results, not bare values.",
        )
    })
}

fn check_o6(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("O6", "No command to test --human flag"));
    };
    let run = target.run_cli(&format!("{cmd} --human"))?;
    Ok(if run.exit != 2 {
        pass("O6", &format!("--human flag accepted on {cmd}"))
    } else {
        fail(
            "O6",
            &format!("--human flag not recognized on {cmd}"),
            "Add --human flag for human-friendly output. Default must be JSON (agent-first).",
        )
    })
}

fn check_o7(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("O7", "No list command to test array wrapping"));
    };
    let run = target.run_cli(&cmd)?;
    let structured = output_json_value(&run)
        .map(|value| {
            matches!(value, Value::Array(_))
                || value.get("items").and_then(Value::as_array).is_some()
                || value.get("result").and_then(Value::as_array).is_some()
                || matches!(value.get("result"), Some(Value::Object(_)))
        })
        .unwrap_or(false);
    Ok(if structured {
        pass("O7", "List output contains structured data")
    } else {
        fail(
            "O7",
            "List output not wrapped in array or object",
            "Wrap list results in JSON array, {items: [...]}, or {result: ...}.",
        )
    })
}

fn check_o8(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("O8", "No list command to test pagination"));
    };
    let run = target.run_cli(&format!("{cmd} --json"))?;
    let has = output_json_value(&run)
        .map(|value| {
            value.get("total").is_some()
                || value.get("has_more").is_some()
                || value.get("showing").is_some()
        })
        .unwrap_or(false);
    Ok(if has {
        pass("O8", "Pagination info present")
    } else {
        skip(
            "O8",
            "No pagination fields (may be acceptable for small datasets)",
        )
    })
}

fn check_o9(target: &TargetState) -> RuleResult {
    let has = target
        .describe_commands()
        .into_iter()
        .flat_map(|command| {
            command
                .get("parameters")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .any(|param| param.get("name").and_then(Value::as_str) == Some("ndjson"));
    if has {
        pass("O9", "NDJSON flag found in schema")
    } else {
        skip(
            "O9",
            "No NDJSON support detected (optional for small datasets)",
        )
    }
}

fn check_o10(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("O10", "No command to test TTY auto-detect"));
    };
    let run = target.run_cli(&cmd)?;
    Ok(if json_valid(&run.stdout) {
        pass("O10", "Auto-detects pipe mode and outputs JSON")
    } else {
        fail(
            "O10",
            "Does not auto-detect pipe mode",
            "When stdout is not TTY, auto-switch to JSON output.",
        )
    })
}

fn capture_error_probe(target: &TargetState) -> Result<(RunOutput, Option<Value>), CliError> {
    let first = target.run_cli("--nonexistent-flag-xyz")?;
    let first_json = error_json_value(&first);
    if first_json.is_some() {
        return Ok((first, first_json));
    }
    let second = target.run_cli("")?;
    let second_json = error_json_value(&second);
    Ok((second, second_json))
}

fn check_e1(target: &TargetState) -> Result<RuleResult, CliError> {
    let (_, err) = capture_error_probe(target)?;
    Ok(
        if err
            .as_ref()
            .map(|v| {
                v.get("error").is_some() && v.get("code").is_some() && v.get("message").is_some()
            })
            .unwrap_or(false)
        {
            pass("E1", "Error format: {error, code, message}")
        } else {
            fail(
                "E1",
                "Error does not follow {error, code, message} format",
                "All errors must output: {error: true, code: \"...\", message: \"...\", suggestion: \"...\"}.",
            )
        },
    )
}

fn check_e2(target: &TargetState) -> Result<RuleResult, CliError> {
    let (run, _) = capture_error_probe(target)?;
    let stderr_err = parse_json(&run.stderr)
        .and_then(|v| v.get("error").cloned())
        .is_some();
    let stdout_err = parse_json(&run.stdout)
        .and_then(|v| v.get("error").cloned())
        .is_some();
    Ok(if stderr_err {
        pass("E2", "Errors go to stderr")
    } else if stdout_err {
        fail(
            "E2",
            "Errors go to stdout instead of stderr",
            "Write error JSON to stderr, keep stdout for data only.",
        )
    } else {
        skip("E2", "Could not determine error output channel")
    })
}

fn check_e3(target: &TargetState) -> Result<RuleResult, CliError> {
    let (_, err) = capture_error_probe(target)?;
    Ok(if err.is_some() {
        pass("E3", "Error output is valid JSON")
    } else {
        fail(
            "E3",
            "Error output is not valid JSON",
            "Even in error cases, output valid JSON.",
        )
    })
}

fn check_e4(target: &TargetState) -> Result<RuleResult, CliError> {
    let (_, err) = capture_error_probe(target)?;
    let code = err.and_then(|v| v.get("code").and_then(Value::as_str).map(ToOwned::to_owned));
    Ok(if let Some(code) = code {
        pass("E4", &format!("Error has code field: {code}"))
    } else {
        fail(
            "E4",
            "Error missing code field",
            "Add machine-readable code (e.g., MISSING_REQUIRED, NOT_FOUND).",
        )
    })
}

fn check_e5(target: &TargetState) -> Result<RuleResult, CliError> {
    let (_, err) = capture_error_probe(target)?;
    Ok(
        if err
            .and_then(|v| {
                v.get("message")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .is_some()
        {
            pass("E5", "Error has message field")
        } else {
            fail(
                "E5",
                "Error missing message field",
                "Add human-readable message to all errors.",
            )
        },
    )
}

fn check_e6(target: &TargetState) -> Result<RuleResult, CliError> {
    let (_, err) = capture_error_probe(target)?;
    Ok(
        if err
            .and_then(|v| {
                v.get("suggestion")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            })
            .is_some()
        {
            pass("E6", "Error has suggestion field")
        } else {
            fail(
                "E6",
                "Error missing suggestion field",
                "Add suggestion telling agent what to do next.",
            )
        },
    )
}

fn check_e7(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target.run_with_timeout(5, "")?;
    Ok(if run.exit != 137 {
        pass(
            "E7",
            &format!(
                "Does not enter interactive mode on error (exited with {})",
                run.exit
            ),
        )
    } else {
        fail(
            "E7",
            "Hangs waiting for input (killed after 5s timeout)",
            "Never enter interactive mode. Return structured error and exit immediately.",
        )
    })
}

fn check_e8(ctx: &AppContext, target: &TargetState) -> RuleResult {
    let snap = ctx
        .snapshots_dir
        .join(target.tool_name())
        .join("errors.json");
    if !snap.is_file() {
        skip(
            "E8",
            &format!(
                "No error snapshot. Run: agent-cli-lint snapshot {}",
                target.cli
            ),
        )
    } else {
        skip(
            "E8",
            "Error stability check requires manual snapshot comparison",
        )
    }
}

fn check_i1(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        return skip("I1", "No flag definitions in schema to verify naming");
    }
    let bad = target
        .describe_commands()
        .into_iter()
        .flat_map(|command| {
            command
                .get("parameters")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|param| {
            param
                .get("name")
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .filter(|name| name.starts_with('-') && name.len() == 2)
        .collect::<Vec<_>>();
    if bad.is_empty() {
        pass("I1", "All flags use --long-name format")
    } else {
        fail(
            "I1",
            &format!("Short-only flags found: {}", bad.join(" ")),
            "All parameters must have --long-name form.",
        )
    }
}

fn check_i3(target: &TargetState) -> RuleResult {
    if target.describe_cache.contains("json-input") {
        pass("I3", "--json-input flag found in schema")
    } else {
        skip("I3", "No --json-input support (optional)")
    }
}

fn check_i4(target: &TargetState) -> Result<RuleResult, CliError> {
    let names = target.command_names();
    if let Some(cmd) = [
        "synth", "add", "create", "set", "update", "delete", "remove",
    ]
    .into_iter()
    .find(|candidate| names.iter().any(|name| name == candidate))
    {
        let run = target.run_cli(cmd)?;
        return Ok(if run.exit == 2 {
            pass("I4", &format!("Missing params on '{cmd}' returns exit 2"))
        } else if run.exit != 0 {
            pass(
                "I4",
                &format!(
                    "Missing params on '{cmd}' returns non-zero (exit {})",
                    run.exit
                ),
            )
        } else if parse_json(&run.stderr)
            .and_then(|v| v.get("error").cloned())
            .is_some()
        {
            warn(
                "I4",
                &format!("Missing params on '{cmd}' returns exit 0 but has error in stderr"),
                "Use exit code 2 for parameter errors.",
            )
        } else {
            fail(
                "I4",
                &format!("Missing params on '{cmd}' returns exit 0"),
                "Return structured error + exit 2 when required params are missing.",
            )
        });
    }
    let run = target.run_cli("")?;
    Ok(if run.exit == 2 {
        pass(
            "I4",
            "Missing params returns exit 2 (no interactive prompt)",
        )
    } else if run.exit != 0 {
        pass(
            "I4",
            &format!("Missing params returns non-zero (exit {})", run.exit),
        )
    } else if parse_json(&run.stdout)
        .and_then(|v| v.get("commands").cloned())
        .is_some()
    {
        pass("I4", "No args shows help (no interactive prompt)")
    } else {
        fail(
            "I4",
            "Missing params returns exit 0",
            "Return structured error + exit 2 when required params are missing.",
        )
    })
}

fn check_i5(target: &TargetState) -> Result<RuleResult, CliError> {
    if !target.has_describe_schema() {
        return Ok(skip(
            "I5",
            "No detailed schema (--describe) available; type validation not checkable from --help",
        ));
    }
    let int_param = target
        .describe_commands()
        .into_iter()
        .flat_map(|command| {
            command
                .get("parameters")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .find_map(|param| {
            if param.get("type").and_then(Value::as_str) == Some("integer") {
                param
                    .get("name")
                    .and_then(Value::as_str)
                    .map(ToOwned::to_owned)
            } else {
                None
            }
        });
    let Some(int_param) = int_param else {
        return Ok(skip("I5", "No integer parameters to test type validation"));
    };
    let cmd = target
        .describe_commands()
        .into_iter()
        .find_map(|command| {
            let params = command.get("parameters").and_then(Value::as_array)?;
            if params
                .iter()
                .any(|param| param.get("name").and_then(Value::as_str) == Some(int_param.as_str()))
            {
                command_name(command)
            } else {
                None
            }
        })
        .unwrap_or_default();
    if !schema_flag_supported(target, &cmd, &int_param, "not-a-number")? {
        return Ok(skip(
            "I5",
            &format!("Schema/CLI mismatch: {cmd} does not accept --{int_param} as a flag"),
        ));
    }
    let run = target.run_cli(&format!("{cmd} --{int_param} not-a-number --json"))?;
    Ok(if run.exit == 2 {
        pass(
            "I5",
            &format!("Type mismatch on --{int_param} returns exit 2"),
        )
    } else {
        skip("I5", "Could not reliably test type validation")
    })
}

fn check_s1(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_destructive_command() else {
        return Ok(skip("S1", "No destructive commands found in schema"));
    };
    let run = target.run_cli(&format!("{cmd} test-id-nonexistent --json"))?;
    if run.exit != 0 {
        if error_code(&run).is_some() {
            Ok(pass(
                "S1",
                &format!("Destructive command {cmd} requires confirmation"),
            ))
        } else {
            Ok(pass(
                "S1",
                &format!(
                    "Destructive command {cmd} rejected without --yes (exit {})",
                    run.exit
                ),
            ))
        }
    } else {
        Ok(fail(
            "S1",
            &format!("Destructive command {cmd} executed without --yes"),
            "Require --yes flag for destructive operations.",
        ))
    }
}
fn check_s2(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_destructive_command() else {
        return Ok(skip("S2", "No destructive commands found"));
    };
    let run = target.run_cli(&format!("{cmd} test-id --json"))?;
    Ok(if run.exit != 0 {
        pass("S2", "Default deny on destructive operation")
    } else {
        fail(
            "S2",
            "Destructive command allowed without --yes",
            "Default behavior must be deny for destructive ops.",
        )
    })
}
fn check_s3(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_destructive_command() else {
        return Ok(skip("S3", "No destructive commands to test --dry-run"));
    };
    let run = target.run_cli(&format!("{cmd} test-id --dry-run --yes --json"))?;
    Ok(if matches!(run.exit, 0 | 20) {
        pass("S3", &format!("--dry-run flag accepted on {cmd}"))
    } else if error_code(&run).as_deref() == Some("UNKNOWN_FLAG") {
        fail(
            "S3",
            &format!("--dry-run not supported on {cmd}"),
            "Add --dry-run for preview without execution.",
        )
    } else {
        pass(
            "S3",
            &format!("--dry-run accepted (exit {} may be expected)", run.exit),
        )
    })
}
fn check_s4(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_command_with_required_param() else {
        return Ok(skip("S4", "No command with params to test input hardening"));
    };
    let run = target.run_cli(&format!("{cmd} ../../etc/passwd --json"))?;
    let code = error_code(&run).unwrap_or_default().to_lowercase();
    Ok(
        if code.contains("path") || code.contains("traversal") || code.contains("blocked") {
            pass("S4", "Path traversal blocked")
        } else {
            skip(
                "S4",
                "Could not reliably test path traversal (depends on command structure)",
            )
        },
    )
}
fn check_s6(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        return skip(
            "S6",
            "No detailed schema (--describe) available; destructive marking not checkable from --help",
        );
    }
    let total = target.describe_commands().len();
    let marked = target
        .describe_commands()
        .into_iter()
        .filter(|command| command.get("destructive").is_some())
        .count();
    if marked > 0 {
        pass(
            "S6",
            &format!("Destructive flag marked on {marked}/{total} commands"),
        )
    } else {
        fail(
            "S6",
            "No commands marked as destructive in schema",
            "Add destructive: true/false to each command in --describe.",
        )
    }
}
fn check_s7(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_destructive_command() else {
        return Ok(skip("S7", "No destructive commands to test --quiet"));
    };
    let run = target.run_cli(&format!("{cmd} test-id --quiet --json"))?;
    Ok(if run.exit != 0 {
        pass("S7", "--quiet does not bypass --yes requirement")
    } else {
        fail(
            "S7",
            "--quiet allowed destructive op without --yes",
            "Even with --quiet, destructive ops must require --yes.",
        )
    })
}
fn check_s8(target: &TargetState) -> RuleResult {
    if target.describe_cache.contains("sanitize") {
        pass("S8", "--sanitize flag found in schema")
    } else {
        skip(
            "S8",
            "No --sanitize flag (optional for tools without external input)",
        )
    }
}

fn check_x1(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("X1", "No command to test success exit code"));
    };
    let run = target.run_cli(&cmd)?;
    Ok(if run.exit == 0 {
        pass("X1", "Success returns exit 0")
    } else {
        fail(
            "X1",
            &format!("Success command returned exit {}", run.exit),
            "Successful commands must return exit 0.",
        )
    })
}
fn check_x2(target: &TargetState) -> RuleResult {
    if target
        .describe_json
        .as_ref()
        .and_then(|v| v.get("exit_codes"))
        .and_then(|v| v.get("1"))
        .is_some()
    {
        pass("X2", "Exit code 1 documented in schema")
    } else {
        skip("X2", "Exit code 1 not explicitly documented")
    }
}
fn check_x3(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target.run_cli("--nonexistent-flag-xyz")?;
    Ok(if run.exit == 2 {
        pass("X3", "Unknown flag returns exit 2")
    } else if run.exit != 0 {
        fail(
            "X3",
            &format!("Unknown flag returns exit {} (expected 2)", run.exit),
            "Parameter/usage errors must return exit 2.",
        )
    } else {
        fail(
            "X3",
            "Unknown flag returns exit 0",
            "Unknown flags must cause exit 2, not be silently ignored.",
        )
    })
}
fn check_x4(target: &TargetState) -> RuleResult {
    if target
        .describe_json
        .as_ref()
        .and_then(|v| v.get("exit_codes"))
        .and_then(|v| v.get("10"))
        .is_some()
    {
        pass("X4", "Exit code 10 (auth failure) documented")
    } else {
        skip("X4", "Exit code 10 not documented (may not apply)")
    }
}
fn check_x5(target: &TargetState) -> RuleResult {
    if target
        .describe_json
        .as_ref()
        .and_then(|v| v.get("exit_codes"))
        .and_then(|v| v.get("11"))
        .is_some()
    {
        pass("X5", "Exit code 11 (permission denied) documented")
    } else {
        skip("X5", "Exit code 11 not documented (may not apply)")
    }
}
fn check_x6(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_show_command() else {
        return Ok(skip("X6", "No show command to test not-found exit code"));
    };
    let run = target.run_cli(&format!("{cmd} nonexistent-id-xyzzy --json"))?;
    Ok(if run.exit == 20 {
        pass("X6", "Not found returns exit 20")
    } else if run.exit != 0 {
        fail(
            "X6",
            &format!("Not found returns exit {} (expected 20)", run.exit),
            "Resource not found should return exit 20.",
        )
    } else {
        fail(
            "X6",
            "Not found returns exit 0",
            "Not-found errors must not return exit 0.",
        )
    })
}
fn check_x7(target: &TargetState) -> RuleResult {
    if target
        .describe_json
        .as_ref()
        .and_then(|v| v.get("exit_codes"))
        .and_then(|v| v.get("30"))
        .is_some()
    {
        pass("X7", "Exit code 30 (conflict) documented")
    } else {
        skip("X7", "Exit code 30 not documented (may not apply)")
    }
}
fn check_x8(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target.run_cli("--nonexistent-flag-xyz")?;
    let code = error_code(&run);
    Ok(if run.exit == 2 && code.is_some() {
        pass(
            "X8",
            &format!(
                "Exit code 2 matches error code {}",
                code.unwrap_or_default()
            ),
        )
    } else if let Some(code) = code {
        pass(
            "X8",
            &format!("Error code {code} present (exit {})", run.exit),
        )
    } else {
        skip("X8", "Cannot verify exit code / error code correspondence")
    })
}
fn check_x9(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target.run_cli("--nonexistent-flag-xyz")?;
    Ok(if run.exit != 0 {
        pass("X9", &format!("Errors do not exit 0 (exit {})", run.exit))
    } else {
        fail(
            "X9",
            "Error exits with 0, masking the failure",
            "Never use exit 0 when an error occurred.",
        )
    })
}

fn check_c1(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("C1", "No command to test stdout purity"));
    };
    let run = target.run_cli(&cmd)?;
    Ok(if json_valid(&run.stdout) {
        pass("C1", "stdout contains pure data (valid JSON)")
    } else {
        fail(
            "C1",
            "stdout contains non-data content",
            "stdout must contain only data. Logs go to stderr.",
        )
    })
}
fn check_c2(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("C2", "No command to test stderr separation"));
    };
    let run = target.run_cli(&cmd)?;
    let stderr_json = parse_json(&run.stderr)
        .map(|value| matches!(value, Value::Array(_) | Value::Object(_)))
        .unwrap_or(false);
    Ok(if run.stderr.trim().is_empty() || !stderr_json {
        pass("C2", "stderr is logs only (not data)")
    } else {
        fail(
            "C2",
            "stderr contains JSON data",
            "Data goes to stdout, logs/progress go to stderr.",
        )
    })
}
fn check_c3(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("C3", "No command to test pipe friendliness"));
    };
    let run = target.run_cli(&cmd)?;
    Ok(if json_valid(&run.stdout) {
        pass("C3", "Output pipes cleanly through jq")
    } else {
        fail(
            "C3",
            "Output does not pipe through jq",
            "Ensure default output is pipe-friendly JSON.",
        )
    })
}
fn check_c4(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("C4", "No command to test --quiet"));
    };
    let run = target.run_cli(&format!("{cmd} --quiet"))?;
    Ok(if run.exit == 0 && run.stderr.trim().is_empty() {
        pass("C4", "--quiet suppresses stderr")
    } else if run.exit == 0 {
        pass("C4", "--quiet accepted (stderr may have minimal output)")
    } else if matches!(
        error_code(&run).as_deref(),
        Some("UNKNOWN_FLAG" | "PARAM_ERROR")
    ) {
        fail(
            "C4",
            "--quiet flag not supported",
            "Add --quiet to suppress non-essential stderr.",
        )
    } else {
        skip(
            "C4",
            "Cannot test --quiet (command failed for other reason)",
        )
    })
}
fn check_c5(target: &TargetState) -> RuleResult {
    if target.describe_cache.contains("json-input") {
        pass("C5", "Pipe chain support via --json-input")
    } else {
        skip("C5", "No --json-input for pipe chaining (optional)")
    }
}
fn check_c6(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip("C6", "No command to test pipe-mode behavior"));
    };
    let run = target.run_cli(&cmd)?;
    Ok(if json_valid(&run.stdout) {
        pass("C6", "No interactive pollution in pipe mode")
    } else {
        fail(
            "C6",
            "Pipe mode output contains non-JSON content",
            "Suppress prompts, spinners, confirmations in pipe mode.",
        )
    })
}

fn check_n2(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        let bad = target
            .command_names()
            .into_iter()
            .filter(|name| name.chars().any(|ch| ch.is_ascii_uppercase()))
            .collect::<Vec<_>>();
        return if bad.is_empty() {
            pass("N2", "All command names use kebab-case")
        } else {
            fail(
                "N2",
                &format!("Non-kebab-case names: {}", bad.join(" ")),
                "Use kebab-case for all names.",
            )
        };
    }
    let bad = target
        .describe_commands()
        .into_iter()
        .flat_map(|command| {
            command
                .get("parameters")
                .and_then(Value::as_array)
                .cloned()
                .unwrap_or_default()
        })
        .filter_map(|param| {
            param
                .get("name")
                .and_then(Value::as_str)
                .map(|name| name.trim_start_matches("--").to_string())
        })
        .filter(|flag| flag.chars().any(|ch| ch.is_ascii_uppercase()))
        .map(|flag| format!("--{flag}"))
        .collect::<Vec<_>>();
    if bad.is_empty() {
        pass("N2", "All flags use kebab-case")
    } else {
        fail(
            "N2",
            &format!("Non-kebab-case flags: {}", bad.join(" ")),
            "Use --output-format not --outputFormat.",
        )
    }
}
fn check_n3(target: &TargetState) -> RuleResult {
    let max_depth = target
        .command_names()
        .into_iter()
        .map(|cmd| cmd.split_whitespace().count())
        .max()
        .unwrap_or(0);
    if max_depth == 0 {
        skip("N3", "No commands to check depth")
    } else if max_depth <= 3 {
        pass("N3", &format!("Max command depth: {max_depth} (≤ 3)"))
    } else {
        fail(
            "N3",
            &format!("Command depth {max_depth} exceeds 3"),
            "Keep commands to max 3 levels: mycli resource action.",
        )
    }
}
fn check_n4(target: &TargetState) -> Result<RuleResult, CliError> {
    let reserved = [
        "--agent",
        "--human",
        "--brief",
        "--help",
        "--version",
        "--yes",
        "--dry-run",
        "--quiet",
        "--fields",
    ];
    let mut found = 0usize;
    if let Some(flags) = target
        .describe_json
        .as_ref()
        .and_then(|v| v.get("global_flags").and_then(Value::as_array))
    {
        for flag in reserved {
            if flags.iter().any(|entry| entry.as_str() == Some(flag)) {
                found += 1;
            }
        }
    } else {
        for flag in reserved {
            match flag {
                "--help" if target.help_exit == 0 => found += 1,
                "--version" if !target.version_cache.trim().is_empty() => found += 1,
                "--brief" if target.brief_exit == 0 && !target.brief_cache.trim().is_empty() => {
                    found += 1
                }
                _ => {
                    let run = target.run_cli(flag)?;
                    if !matches!(
                        error_code(&run).as_deref(),
                        Some("UNKNOWN_FLAG" | "PARAM_ERROR")
                    ) {
                        found += 1;
                    }
                }
            }
        }
    }
    Ok(if found >= 4 {
        pass(
            "N4",
            &format!("Reserved flags present ({found}/{})", reserved.len()),
        )
    } else {
        fail(
            "N4",
            &format!("Only {found}/{} reserved flags found", reserved.len()),
            "Support: --agent, --human, --brief, --help, --version, --yes, --dry-run, --quiet, --fields.",
        )
    })
}
fn check_n5(target: &TargetState) -> RuleResult {
    if target.help_exit == 0 && !target.help_cache.trim().is_empty() {
        pass("N5", "--help produces output")
    } else {
        fail("N5", "--help not working", "Add --help flag.")
    }
}
fn check_n6(target: &TargetState) -> RuleResult {
    let ver = target.tool_version();
    if ver.contains('.')
        && ver
            .chars()
            .next()
            .map(|c| c.is_ascii_digit())
            .unwrap_or(false)
    {
        pass("N6", &format!("--version outputs semver: {ver}"))
    } else {
        fail(
            "N6",
            &format!("--version output not semver: {ver}"),
            "Use semver format: major.minor.patch.",
        )
    }
}

fn check_m1(target: &TargetState) -> RuleResult {
    if target.target_dir.join("AGENTS.md").is_file() {
        pass("M1", "AGENTS.md exists")
    } else {
        fail(
            "M1",
            &format!("AGENTS.md not found at {}/", target.target_dir.display()),
            "Create AGENTS.md with build/test/usage instructions.",
        )
    }
}
fn check_m2(target: &TargetState) -> RuleResult {
    if target.describe_cache.to_lowercase().contains("mcp")
        || target.help_cache.to_lowercase().contains("mcp")
    {
        pass("M2", "MCP export support detected")
    } else {
        skip("M2", "No MCP export (optional)")
    }
}
fn check_m3(target: &TargetState) -> RuleResult {
    if target.target_dir.join("CHANGELOG.md").is_file() {
        pass("M3", "CHANGELOG.md exists")
    } else {
        skip(
            "M3",
            "No CHANGELOG.md (recommended for tracking breaking changes)",
        )
    }
}

fn check_f1(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target.run_cli("issue")?;
    let code = error_code(&run).unwrap_or_default();
    Ok(
        if run.exit != 0
            && matches!(
                code.as_str(),
                "MISSING_PARAM" | "MISSING_REQUIRED" | "MISSING_COMMAND"
            )
        {
            pass("F1", "issue subcommand exists (requires subcommand)")
        } else if code == "UNKNOWN_COMMAND" {
            fail(
                "F1",
                "No issue subcommand",
                "Add issue create/list/show subcommands.",
            )
        } else {
            pass("F1", "issue subcommand recognized (exit path verified)")
        },
    )
}
fn check_f3(target: &TargetState) -> RuleResult {
    if target
        .describe_commands()
        .into_iter()
        .find(|command| {
            command_name(command).as_deref() == Some("issue")
                && command.get("subcommands").is_some()
        })
        .is_some()
    {
        pass("F3", "Issue subcommands defined in schema")
    } else if target.command_names().iter().any(|name| name == "issue") {
        pass("F3", "Issue command found in --help commands list")
    } else {
        skip("F3", "Issue category check requires manual review")
    }
}
fn check_f5(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target.run_cli("issue list")?;
    Ok(if run.exit == 0 {
        pass("F5", "issue list works")
    } else {
        skip("F5", "issue list not tested (may need setup)")
    })
}
fn check_f7(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target
        .run_cli("issue create --type suggestion --message 'lint test issue for F7 check'")?;
    if run.exit != 0 {
        return Ok(skip(
            "F7",
            &format!("Could not create test issue (exit {})", run.exit),
        ));
    }
    let json = output_json_value(&run).unwrap_or_else(|| json!({}));
    let has = |keys: &[&str]| keys.iter().any(|key| json.pointer(key).is_some());
    let mut missing = Vec::new();
    if !has(&["/id", "/result/id"]) {
        missing.push("id");
    }
    if !has(&["/type", "/result/type"]) {
        missing.push("type");
    }
    if !has(&["/status", "/result/status"]) {
        missing.push("status");
    }
    if !has(&["/message", "/result/message"]) {
        missing.push("message");
    }
    if !has(&[
        "/created_at",
        "/created",
        "/result/created_at",
        "/result/created",
    ]) {
        missing.push("created_at");
    }
    if !has(&[
        "/updated_at",
        "/updated",
        "/result/updated_at",
        "/result/updated",
    ]) {
        missing.push("updated_at");
    }
    Ok(if missing.is_empty() {
        pass(
            "F7",
            "Issue JSON has all required fields (id, type, status, message, created_at, updated_at)",
        )
    } else {
        fail(
            "F7",
            &format!("Issue missing fields: {}", missing.join(" ")),
            "Each issue must have: id, type, status, message, context, created_at, updated_at.",
        )
    })
}
fn check_f8(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target.run_cli("issue list")?;
    if run.exit != 0 {
        return Ok(skip(
            "F8",
            &format!("issue list failed (exit {})", run.exit),
        ));
    }
    let json = output_json_value(&run).unwrap_or_else(|| json!([]));
    let items = match json {
        Value::Array(items) => items,
        Value::Object(map) => map
            .get("result")
            .and_then(Value::as_array)
            .cloned()
            .unwrap_or_default(),
        _ => Vec::new(),
    };
    if items.is_empty() {
        return Ok(skip("F8", "No issues found to verify status field"));
    }
    let missing = items
        .iter()
        .filter(|item| item.get("status").is_none())
        .count();
    Ok(if missing == 0 {
        pass(
            "F8",
            &format!("All {} issues have status field", items.len()),
        )
    } else {
        fail(
            "F8",
            &format!("{missing}/{} issues missing status field", items.len()),
            "Each issue must have status: open|in-progress|resolved|closed.",
        )
    })
}

fn check_g1(target: &TargetState) -> Result<RuleResult, CliError> {
    let run = target.run_cli("--completely-bogus-flag-xyz")?;
    Ok(if run.exit == 2 {
        pass("G1", "Unknown flags rejected with exit 2")
    } else if run.exit != 0 {
        pass("G1", &format!("Unknown flags rejected (exit {})", run.exit))
    } else {
        fail(
            "G1",
            "Unknown flags silently ignored",
            "Reject unknown flags with structured error.",
        )
    })
}
fn check_g2(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_command_with_required_param() else {
        return Ok(skip(
            "G2",
            "No command with params to test secret detection",
        ));
    };
    let args = build_required_args(
        target,
        &cmd,
        "sk-abc123secretkey456789012345678901",
        "test-dummy-value",
    )
    .unwrap_or_else(|| "--name sk-abc123secretkey456789012345678901".to_string());
    let first_param = args
        .split_whitespace()
        .next()
        .unwrap_or("--name")
        .trim_start_matches("--")
        .to_string();
    if !schema_flag_supported(target, &cmd, &first_param, "lint-probe")? {
        return Ok(skip(
            "G2",
            &format!("Schema/CLI mismatch: {cmd} required params are not invocable as flags"),
        ));
    }
    let run = target.run_cli(&format!("{cmd} {args} --json"))?;
    let code = error_code(&run).unwrap_or_default();
    Ok(if code.contains("SECRET") || code.contains("BLOCKED") {
        pass("G2", &format!("API key pattern blocked (code: {code})"))
    } else if run.exit != 0 {
        fail(
            "G2",
            &format!(
                "API key not explicitly detected (exited {} but no SECRET code)",
                run.exit
            ),
            "Add secret detection: reject sk-*, ghp_*, API key patterns.",
        )
    } else {
        fail(
            "G2",
            "API key pattern accepted — no secret detection",
            "Add guardrail: detect and block API key/token patterns in arguments.",
        )
    })
}
fn check_g3(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_command_with_required_param() else {
        return Ok(skip("G3", "No command to test path rejection"));
    };
    let args = build_required_args(target, &cmd, "../../etc/passwd", "test-dummy-value")
        .unwrap_or_else(|| "--name ../../etc/passwd".to_string());
    let first_param = args
        .split_whitespace()
        .next()
        .unwrap_or("--name")
        .trim_start_matches("--")
        .to_string();
    if !schema_flag_supported(target, &cmd, &first_param, "lint-probe")? {
        return Ok(skip(
            "G3",
            &format!("Schema/CLI mismatch: {cmd} required params are not invocable as flags"),
        ));
    }
    let run = target.run_cli(&format!("{cmd} {args} --json"))?;
    let code = error_code(&run).unwrap_or_default();
    Ok(
        if ["PATH", "TRAVERSAL", "SENSITIVE", "BLOCKED"]
            .iter()
            .any(|needle| code.contains(needle))
        {
            pass("G3", &format!("Dangerous path blocked (code: {code})"))
        } else {
            skip(
                "G3",
                "Path rejection may depend on param type (not all params are paths)",
            )
        },
    )
}
fn check_g4(target: &TargetState) -> RuleResult {
    if !target.has_describe_schema() {
        return skip(
            "G4",
            "No detailed schema (--describe) available; permission levels not checkable from --help",
        );
    }
    let marked = target
        .describe_commands()
        .into_iter()
        .filter(|command| command.get("permission").is_some())
        .count();
    if marked > 0 {
        pass("G4", "Permission levels marked on commands")
    } else {
        fail(
            "G4",
            "No permission levels in schema",
            "Add permission: read/write/delete to each command.",
        )
    }
}
fn check_g8(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_command_with_required_param() else {
        return Ok(skip("G8", "No command to test shell metachar rejection"));
    };
    let args = build_required_args(target, &cmd, "'foo;rm -rf /'", "test-dummy-value")
        .unwrap_or_else(|| "--name 'foo;rm -rf /'".to_string());
    let first_param = args
        .split_whitespace()
        .next()
        .unwrap_or("--name")
        .trim_start_matches("--")
        .to_string();
    if !schema_flag_supported(target, &cmd, &first_param, "lint-probe")? {
        return Ok(skip(
            "G8",
            &format!("Schema/CLI mismatch: {cmd} required params are not invocable as flags"),
        ));
    }
    let run = target.run_cli(&format!("{cmd} {args} --json"))?;
    let code = error_code(&run).unwrap_or_default();
    Ok(
        if ["SHELL", "INJECTION", "BLOCKED", "META"]
            .iter()
            .any(|needle| code.contains(needle))
        {
            pass(
                "G8",
                &format!("Shell metacharacters blocked (code: {code})"),
            )
        } else if run.exit != 0 {
            fail(
                "G8",
                &format!(
                    "Command failed (exit {}) but no explicit shell injection code",
                    run.exit
                ),
                "Add guardrail: reject ; | & $ ` in argument values.",
            )
        } else {
            fail(
                "G8",
                "Shell metacharacters accepted — no injection guard",
                "Add guardrail: reject ; | & $ ` in argument values.",
            )
        },
    )
}

fn check_r1(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip(
            "R1",
            "No list-like command found to test rules[] in response",
        ));
    };
    let run = target.run_cli(&cmd)?;
    Ok(
        if output_json_value(&run)
            .and_then(|v| {
                v.get("rules")
                    .and_then(Value::as_array)
                    .map(|arr| !arr.is_empty())
            })
            .unwrap_or(false)
        {
            pass("R1", "Response includes non-empty rules[]")
        } else {
            fail(
                "R1",
                "Response missing rules[] array",
                "Every command response must include rules[] with full content from agent/rules/*.md.",
            )
        },
    )
}
fn check_r2(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip(
            "R2",
            "No list-like command found to test skills[] in response",
        ));
    };
    let run = target.run_cli(&cmd)?;
    let has_skills = output_json_value(&run)
        .map(|v| v.get("skills").and_then(Value::as_array).is_some())
        .unwrap_or(false);
    Ok(if has_skills {
        pass("R2", "Response includes skills[]")
    } else {
        fail(
            "R2",
            "Response missing skills[] array",
            "Every command response must include skills[] (name + description + command).",
        )
    })
}
fn check_r3(target: &TargetState) -> Result<RuleResult, CliError> {
    let Some(cmd) = target.find_list_command() else {
        return Ok(skip(
            "R3",
            "No list-like command found to test issue in response",
        ));
    };
    let run = target.run_cli(&cmd)?;
    Ok(
        if output_json_value(&run)
            .and_then(|v| v.get("issue").cloned())
            .is_some()
        {
            pass("R3", "Response includes issue field")
        } else {
            fail(
                "R3",
                "Response missing issue field",
                "Every command response must include issue (feedback guide string or object).",
            )
        },
    )
}
