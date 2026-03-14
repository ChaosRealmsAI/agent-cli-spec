use std::path::Path;
use std::process::{Command, Stdio};

use crate::output::CliError;

pub fn delegate_to_legacy(root_dir: &Path, args: &[String]) -> Result<i32, CliError> {
    let script = root_dir.join("agent-cli-lint");
    if !script.is_file() {
        return Err(CliError::new(
            "NOT_FOUND",
            format!("Legacy script not found: {}", script.display()),
            Some("Restore the root-level agent-cli-lint script before using compatibility mode."),
            20,
        ));
    }

    let status = Command::new(&script)
        .args(args)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .map_err(|err| {
            CliError::new(
                "INTERNAL_ERROR",
                format!("Failed to execute {}: {err}", script.display()),
                Some("Verify the legacy script is executable."),
                1,
            )
        })?;

    Ok(status.code().unwrap_or(1))
}
