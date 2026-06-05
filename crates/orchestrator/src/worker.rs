// crates/orchestrator/src/worker.rs
//
// Tier 4: The Worker
// Executes SURGICAL edits only — no full file rewrites allowed.
// On failure (exit code > 0): auto-pushes error to R&D queue for self-healing.

use anyhow::Result;
use std::path::{Path, PathBuf};
use tokio::process::Command;
use tracing::{info, warn};

use crate::manager::WorkerInstruction;
use crate::config::OrchestratorConfig;
use crate::gateway::GatewayClient;

#[derive(Debug)]
pub struct ExecutionResult {
    pub success: bool,
    pub title: String,
    pub output: String,
    pub rnd_queued: bool,
}

pub struct Worker {
    workspace: PathBuf,
    gateway: GatewayClient,
}

impl Worker {
    pub fn new(config: &OrchestratorConfig, gateway: GatewayClient) -> Self {
        Worker {
            workspace: PathBuf::from(&config.workspace_path),
            gateway,
        }
    }

    pub async fn execute(&self, instruction: &WorkerInstruction) -> ExecutionResult {
        match instruction {
            WorkerInstruction::Bash { title, command, cwd } => {
                self.run_bash(title, command, cwd.as_deref()).await
            }
            WorkerInstruction::Edit { title, file, find, replace } => {
                self.surgical_edit(title, file, find, replace)
            }
            WorkerInstruction::Create { title, file, content } => {
                self.create_file(title, file, content)
            }
        }
    }

    // ── Bash execution ───────────────────────────────────────────────────────

    async fn run_bash(&self, title: &str, command: &str, cwd: Option<&str>) -> ExecutionResult {
        let work_dir = cwd
            .map(|d| self.workspace.join(d))
            .unwrap_or_else(|| self.workspace.clone());

        info!("[T4 Worker] BASH: {}", command);

        let result = Command::new("bash")
            .arg("-c")
            .arg(command)
            .current_dir(&work_dir)
            .output()
            .await;

        match result {
            Ok(output) => {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                let combined = format!("{}{}", stdout, stderr);

                if output.status.success() {
                    info!("[T4 Worker] ✅ {}", title);
                    ExecutionResult { success: true, title: title.into(), output: combined, rnd_queued: false }
                } else {
                    warn!("[T4 Worker] ❌ {} (exit {:?})", title, output.status.code());
                    // Auto-push to R&D queue
                    let error_msg = format!("Command: {}\nSTDERR: {}\nSTDOUT: {}", command, stderr, stdout);
                    let queued = self.push_to_rnd(&error_msg, title, output.status.code().unwrap_or(1)).await;
                    ExecutionResult { success: false, title: title.into(), output: combined, rnd_queued: queued }
                }
            }
            Err(e) => {
                warn!("[T4 Worker] ❌ {} — spawn error: {}", title, e);
                ExecutionResult { success: false, title: title.into(), output: e.to_string(), rnd_queued: false }
            }
        }
    }

    // ── Surgical edit — NO full rewrites, EVER ───────────────────────────────

    fn surgical_edit(&self, title: &str, file: &str, find: &str, replace: &str) -> ExecutionResult {
        let path = self.workspace.join(file);

        let content = match std::fs::read_to_string(&path) {
            Ok(c) => c,
            Err(e) => {
                warn!("[T4 Worker] ❌ Cannot read {}: {}", file, e);
                return ExecutionResult {
                    success: false,
                    title: title.into(),
                    output: format!("File not found: {}", file),
                    rnd_queued: false,
                };
            }
        };

        if !content.contains(find) {
            warn!("[T4 Worker] ❌ Target text not found in {}", file);
            return ExecutionResult {
                success: false,
                title: title.into(),
                output: format!("Text to replace not found in {}", file),
                rnd_queued: false,
            };
        }

        // Surgical replacement — first occurrence only for safety
        let new_content = content.replacen(find, replace, 1);
        if let Err(e) = std::fs::write(&path, &new_content) {
            return ExecutionResult {
                success: false,
                title: title.into(),
                output: format!("Write error: {}", e),
                rnd_queued: false,
            };
        }

        info!("[T4 Worker] ✅ Surgical edit in {}", file);
        ExecutionResult {
            success: true,
            title: title.into(),
            output: format!("Edited {} — replaced {} chars with {} chars", file, find.len(), replace.len()),
            rnd_queued: false,
        }
    }

    // ── Create new file ──────────────────────────────────────────────────────

    fn create_file(&self, title: &str, file: &str, content: &str) -> ExecutionResult {
        let path = self.workspace.join(file);

        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                return ExecutionResult {
                    success: false,
                    title: title.into(),
                    output: format!("Cannot create dirs: {}", e),
                    rnd_queued: false,
                };
            }
        }

        if let Err(e) = std::fs::write(&path, content) {
            return ExecutionResult {
                success: false,
                title: title.into(),
                output: format!("Write error: {}", e),
                rnd_queued: false,
            };
        }

        info!("[T4 Worker] ✅ Created {}", file);
        ExecutionResult {
            success: true,
            title: title.into(),
            output: format!("Created {} ({} bytes)", file, content.len()),
            rnd_queued: false,
        }
    }

    // ── R&D queue push ───────────────────────────────────────────────────────

    async fn push_to_rnd(&self, error: &str, context: &str, exit_code: i32) -> bool {
        match self.gateway.push_error(error, context, exit_code).await {
            Ok(_) => {
                info!("[T4 Worker] Error queued to R&D engine for self-healing");
                true
            }
            Err(e) => {
                warn!("[T4 Worker] Failed to push to R&D queue: {}", e);
                false
            }
        }
    }
}
