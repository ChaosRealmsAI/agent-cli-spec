mod agent;
mod issue;
mod lint;
mod output;

use std::env;
use std::fs;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use chrono::Utc;
use output::CliError;
use serde_json::{Value, json};

const VERSION: &str = "0.1.0";
#[derive(Clone, Debug, Default)]
struct GlobalOptions {
    output_json: bool,
    force_human: bool,
    fields: Option<Vec<String>>,
    quiet: bool,
}

#[derive(Clone, Debug)]
enum CommandSpec {
    Help,
    Brief,
    Describe,
    Version,
    Skills { name: Option<String> },
    Issue(IssueCommand),
    Check(Vec<String>),
    Snapshot(Vec<String>),
    Diff(Vec<String>),
    AiPrompts(Vec<String>),
}

#[derive(Clone, Debug)]
enum IssueCommand {
    Create { issue_type: String, message: String },
    List,
    Show { id: String },
}

#[derive(Clone, Debug)]
struct AppContext {
    root_dir: PathBuf,
    lint_dir: PathBuf,
    issues_dir: PathBuf,
    snapshots_dir: PathBuf,
    global: GlobalOptions,
}

impl AppContext {
    fn new(global: GlobalOptions) -> Result<Self, CliError> {
        let root_dir = repo_root()?;
        let home = env::var("HOME").map_err(|_| {
            CliError::new(
                "INTERNAL_ERROR",
                "HOME is not set",
                Some("Set HOME before running agent-cli-lint."),
                1,
            )
        })?;
        let lint_dir = env::var("AGENT_CLI_LINT_DIR")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from(home).join(".agent-cli-lint"));
        let issues_dir = lint_dir.join("issues");
        let snapshots_dir = lint_dir.join("snapshots");

        Ok(Self {
            root_dir,
            lint_dir,
            issues_dir,
            snapshots_dir,
            global,
        })
    }

    fn wants_json_output(&self) -> bool {
        if self.global.output_json {
            return true;
        }
        if self.global.force_human {
            return false;
        }
        !std::io::stdout().is_terminal()
    }

    fn ensure_dirs(&self) -> Result<(), CliError> {
        fs::create_dir_all(&self.issues_dir)
            .and_then(|_| fs::create_dir_all(&self.snapshots_dir))
            .map_err(|err| {
                CliError::new(
                    "INTERNAL_ERROR",
                    format!("Failed to create {}: {err}", self.lint_dir.display()),
                    Some("Check directory permissions."),
                    1,
                )
            })
    }

    fn log(&self, message: &str) {
        if !self.global.quiet {
            eprintln!("{message}");
        }
    }

    fn issue_guide(&self) -> String {
        "Any problem, bad output, or confusion — run: agent-cli-lint issue create --type <bug|requirement|suggestion|bad-output> --message '...'".to_string()
    }
}

fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code as u8),
        Err(error) => {
            output::print_error(&error);
            ExitCode::from(error.exit_code as u8)
        }
    }
}

fn run() -> Result<i32, CliError> {
    let raw_args: Vec<String> = env::args().skip(1).collect();
    let (global, command) = parse_args(&raw_args)?;
    let ctx = AppContext::new(global)?;

    match command {
        CommandSpec::Help => {
            let value = build_help(&ctx)?;
            output::print_json(&ctx, value)?;
            Ok(0)
        }
        CommandSpec::Brief => {
            println!("{}", agent::read_brief(&ctx.root_dir)?);
            Ok(0)
        }
        CommandSpec::Describe => {
            output::print_json(&ctx, build_describe())?;
            Ok(0)
        }
        CommandSpec::Version => {
            println!("{VERSION}");
            Ok(0)
        }
        CommandSpec::Skills { name } => {
            let value = if let Some(name) = name {
                agent::skill_detail(&ctx.root_dir, &name)?
            } else {
                agent::skills_value(&ctx.root_dir)?
            };
            output::print_json(&ctx, value)?;
            Ok(0)
        }
        CommandSpec::Issue(issue_cmd) => handle_issue(&ctx, issue_cmd),
        CommandSpec::Check(args) => lint::cmd_check(&ctx, &args),
        CommandSpec::Snapshot(args) => lint::cmd_snapshot(&ctx, &args),
        CommandSpec::Diff(args) => lint::cmd_diff(&ctx, &args),
        CommandSpec::AiPrompts(args) => lint::cmd_ai_prompts(&ctx, &args),
    }
}

fn parse_args(args: &[String]) -> Result<(GlobalOptions, CommandSpec), CliError> {
    let mut global = GlobalOptions::default();
    let mut rest = Vec::new();
    let mut i = 0;

    while i < args.len() {
        match args[i].as_str() {
            "--json" | "--agent" => {
                global.output_json = true;
                global.force_human = false;
                i += 1;
            }
            "--human" => {
                global.force_human = true;
                global.output_json = false;
                i += 1;
            }
            "--fields" => {
                let value = args.get(i + 1).ok_or_else(|| {
                    CliError::new(
                        "MISSING_VALUE",
                        "--fields requires a value",
                        None::<String>,
                        2,
                    )
                })?;
                global.fields = Some(
                    value
                        .split(',')
                        .filter(|entry| !entry.is_empty())
                        .map(ToOwned::to_owned)
                        .collect(),
                );
                i += 2;
            }
            "--quiet" => {
                global.quiet = true;
                i += 1;
            }
            "--brief" => return Ok((global, CommandSpec::Brief)),
            "--help" | "-h" => return Ok((global, CommandSpec::Help)),
            "--describe" => return Ok((global, CommandSpec::Describe)),
            "--version" => return Ok((global, CommandSpec::Version)),
            other => {
                rest.push(other.to_string());
                i += 1;
            }
        }
    }

    let command = rest.first().ok_or_else(|| {
        CliError::new(
            "MISSING_COMMAND",
            "No command specified",
            Some("Commands: check, snapshot, diff, ai-prompts, issue, skills"),
            2,
        )
    })?;

    match command.as_str() {
        "skills" => Ok((
            global,
            CommandSpec::Skills {
                name: rest.get(1).cloned(),
            },
        )),
        "issue" => Ok((global, CommandSpec::Issue(parse_issue_args(&rest[1..])?))),
        "check" => Ok((global, CommandSpec::Check(rest[1..].to_vec()))),
        "snapshot" => Ok((global, CommandSpec::Snapshot(rest[1..].to_vec()))),
        "diff" => Ok((global, CommandSpec::Diff(rest[1..].to_vec()))),
        "ai-prompts" => Ok((global, CommandSpec::AiPrompts(rest[1..].to_vec()))),
        _ => Err(CliError::new(
            "UNKNOWN_COMMAND",
            format!("Unknown command: {command}"),
            Some("Commands: check, snapshot, diff, ai-prompts, issue, skills"),
            2,
        )),
    }
}

fn parse_issue_args(args: &[String]) -> Result<IssueCommand, CliError> {
    let subcommand = args.first().ok_or_else(|| {
        CliError::new(
            "MISSING_PARAM",
            "Missing subcommand for issue",
            Some("Usage: agent-cli-lint issue <create|list|show>"),
            2,
        )
    })?;

    match subcommand.as_str() {
        "list" => Ok(IssueCommand::List),
        "show" => {
            let id = args.get(1).ok_or_else(|| {
                CliError::new(
                    "MISSING_PARAM",
                    "Missing: issue ID",
                    Some("Usage: agent-cli-lint issue show <id>"),
                    2,
                )
            })?;
            Ok(IssueCommand::Show { id: id.clone() })
        }
        "create" => {
            let mut issue_type = None;
            let mut message = None;
            let mut i = 1;

            while i < args.len() {
                match args[i].as_str() {
                    "--type" => {
                        let value = args.get(i + 1).ok_or_else(|| {
                            CliError::new(
                                "MISSING_VALUE",
                                "Flag --type requires a value",
                                None::<String>,
                                2,
                            )
                        })?;
                        issue_type = Some(value.clone());
                        i += 2;
                    }
                    "--message" => {
                        let value = args.get(i + 1).ok_or_else(|| {
                            CliError::new(
                                "MISSING_VALUE",
                                "Flag --message requires a value",
                                None::<String>,
                                2,
                            )
                        })?;
                        message = Some(value.clone());
                        i += 2;
                    }
                    other => {
                        return Err(CliError::new(
                            "UNKNOWN_FLAG",
                            format!("Unknown flag: {other}"),
                            Some("Valid flags: --type, --message"),
                            2,
                        ));
                    }
                }
            }

            let issue_type = issue_type.ok_or_else(|| {
                CliError::new(
                    "MISSING_PARAM",
                    "Missing: --type",
                    Some("Options: bug, requirement, suggestion, bad-output"),
                    2,
                )
            })?;
            let message = message.ok_or_else(|| {
                CliError::new(
                    "MISSING_PARAM",
                    "Missing: --message",
                    Some("Describe the issue."),
                    2,
                )
            })?;
            validate_issue_input(&issue_type, &message)?;
            Ok(IssueCommand::Create {
                issue_type,
                message,
            })
        }
        other => Err(CliError::new(
            "UNKNOWN_COMMAND",
            format!("Unknown issue subcommand: {other}"),
            Some("Available: create, list, show"),
            2,
        )),
    }
}

fn validate_issue_input(issue_type: &str, message: &str) -> Result<(), CliError> {
    match issue_type {
        "bug" | "requirement" | "suggestion" | "bad-output" => {}
        _ => {
            return Err(CliError::new(
                "INVALID_ENUM",
                format!("Invalid issue type: {issue_type}"),
                Some("Valid: bug, requirement, suggestion, bad-output"),
                2,
            ));
        }
    }

    if message.contains(';')
        || message.contains('|')
        || message.contains('&')
        || message.contains("`")
        || message.contains("$(")
    {
        return Err(CliError::new(
            "SHELL_INJECTION",
            "Parameter --message contains shell metacharacters",
            Some("Remove ; | & $ ` characters from the value."),
            2,
        ));
    }

    if message
        .chars()
        .any(|ch| ch.is_control() && !ch.is_whitespace())
    {
        return Err(CliError::new(
            "CONTROL_CHAR",
            "Parameter --message contains control characters",
            Some("Remove non-printable characters from the value."),
            2,
        ));
    }

    Ok(())
}

fn handle_issue(ctx: &AppContext, issue_cmd: IssueCommand) -> Result<i32, CliError> {
    match issue_cmd {
        IssueCommand::Create {
            issue_type,
            message,
        } => {
            ctx.ensure_dirs()?;
            let issue = issue::create_issue(&ctx.issues_dir, &issue_type, &message, VERSION)?;
            ctx.log(&format!("Issue created: {}", issue.id));
            if ctx.wants_json_output() {
                output::print_json(
                    ctx,
                    serde_json::to_value(issue).expect("issue should serialize"),
                )?;
            }
            Ok(0)
        }
        IssueCommand::List => {
            ctx.ensure_dirs()?;
            let issues = issue::list_issues(&ctx.issues_dir)?;
            output::print_list(ctx, issues)?;
            Ok(0)
        }
        IssueCommand::Show { id } => {
            ctx.ensure_dirs()?;
            let issue = issue::show_issue(&ctx.issues_dir, &id)?;
            output::print_json(
                ctx,
                serde_json::to_value(issue).expect("issue should serialize"),
            )?;
            Ok(0)
        }
    }
}

fn build_help(ctx: &AppContext) -> Result<Value, CliError> {
    Ok(json!({
        "help": agent::read_brief(&ctx.root_dir)?,
        "version": VERSION,
        "commands": [
            {"name": "check", "description": "Check CLI compliance against spec rules"},
            {"name": "snapshot", "description": "Save schema and error code snapshot"},
            {"name": "diff", "description": "Compare current vs saved snapshot"},
            {"name": "ai-prompts", "description": "Generate AI check prompts for non-automatable rules"},
            {"name": "issue", "description": "Feedback system (create/list/show)"},
            {"name": "skills", "description": "View available skills"}
        ],
        "layers": [
            {"name": "core", "description": "Minimum execution contract: JSON, errors, exit codes, stdout/stderr separation, safety guardrails"},
            {"name": "recommended", "description": "Machine-friendly ergonomics: self-description, explicit flags, pipe-safe UX, richer safety semantics"},
            {"name": "ecosystem", "description": "Agent-native ecosystem contract: agent metadata, skills, feedback workflow, inline context"}
        ],
        "rules": agent::rules_value(&ctx.root_dir)?,
        "skills": agent::skills_entries(&ctx.root_dir)?,
        "issue": ctx.issue_guide()
    }))
}

fn build_describe() -> Value {
    json!({
        "name": "agent-cli-lint",
        "version": VERSION,
        "description": "Agent-Friendly CLI Spec v0.1 compliance checker",
        "commands": [
            {
                "name": "check",
                "description": "Check CLI compliance against 98 rules",
                "destructive": false,
                "permission": "read",
                "parameters": [
                    {"name": "cli", "type": "string", "required": true, "description": "Target CLI command to check"},
                    {"name": "dimension", "type": "string", "required": false, "description": "Filter by dimension (01-11)"},
                    {"name": "layer", "type": "string", "required": false, "enum": ["core", "recommended", "ecosystem"], "description": "Filter by execution layer"},
                    {"name": "priority", "type": "string", "required": false, "enum": ["p0", "p1", "p2"], "description": "Filter by priority"},
                    {"name": "rule", "type": "string", "required": false, "description": "Check single rule by ID (e.g. O1)"}
                ],
                "output": {"type": "object", "properties": {"tool": "string", "scope": "object", "summary": "object", "layers": "array", "certification": "object", "dimensions": "array"}}
            },
            {
                "name": "snapshot",
                "description": "Save schema and error code snapshot",
                "destructive": false,
                "permission": "write",
                "parameters": [
                    {"name": "cli", "type": "string", "required": true, "description": "Target CLI command"}
                ]
            },
            {
                "name": "diff",
                "description": "Compare current vs saved snapshot",
                "destructive": false,
                "permission": "read",
                "parameters": [
                    {"name": "cli", "type": "string", "required": true, "description": "Target CLI command"}
                ]
            },
            {
                "name": "ai-prompts",
                "description": "Generate AI check prompts for non-automatable rules",
                "destructive": false,
                "permission": "read",
                "parameters": [
                    {"name": "cli", "type": "string", "required": true, "description": "Target CLI command"}
                ]
            },
            {
                "name": "issue",
                "description": "Feedback system",
                "subcommands": ["create", "list", "show"],
                "destructive": false
            },
            {
                "name": "skills",
                "description": "View available skills",
                "destructive": false,
                "permission": "read",
                "parameters": [
                    {"name": "name", "type": "string", "required": false, "description": "Skill name"}
                ]
            }
        ],
        "global_flags": ["--agent", "--human", "--brief", "--help", "--version", "--fields", "--quiet"],
        "exit_codes": {
            "0": "Success (all checked rules pass)",
            "1": "Lint failures found or general error",
            "2": "Usage/parameter error",
            "20": "Resource not found"
        }
    })
}

fn repo_root() -> Result<PathBuf, CliError> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .and_then(Path::parent)
        .map(Path::to_path_buf)
        .ok_or_else(|| {
            CliError::new(
                "INTERNAL_ERROR",
                "Failed to resolve repository root",
                Some("Run from the checked-out repository."),
                1,
            )
        })
}

#[allow(dead_code)]
fn _timestamp() -> String {
    Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_help_flag() {
        let args = vec!["--help".to_string()];
        let (_, command) = parse_args(&args).expect("help should parse");
        assert!(matches!(command, CommandSpec::Help));
    }

    #[test]
    fn parses_issue_create_flags() {
        let args = vec![
            "issue".to_string(),
            "create".to_string(),
            "--type".to_string(),
            "bug".to_string(),
            "--message".to_string(),
            "broken output".to_string(),
        ];
        let (_, command) = parse_args(&args).expect("issue create should parse");
        match command {
            CommandSpec::Issue(IssueCommand::Create {
                issue_type,
                message,
            }) => {
                assert_eq!(issue_type, "bug");
                assert_eq!(message, "broken output");
            }
            _ => panic!("expected issue create command"),
        }
    }

    #[test]
    fn rejects_shell_metacharacters_in_issue_message() {
        let error =
            validate_issue_input("bug", "oops; rm -rf").expect_err("should reject shell meta");
        assert_eq!(error.code, "SHELL_INJECTION");
    }
}
