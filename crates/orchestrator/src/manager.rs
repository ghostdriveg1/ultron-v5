// crates/orchestrator/src/manager.rs
//
// Tier 3: The Manager
// Routes requests through the Gateway's KeyPool (1.5B token pool).
// Breaks Board sub-tasks into surgical Worker instructions.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::llm::{LlmClient, Message};
use crate::board::SubTask;
use crate::config::OrchestratorConfig;

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WorkerInstruction {
    /// Execute a bash command
    Bash {
        title: String,
        command: String,
        cwd: Option<String>,
    },
    /// Surgical line replacement — NEVER full rewrites
    Edit {
        title: String,
        file: String,
        find: String,
        replace: String,
    },
    /// Create a new file with content
    Create {
        title: String,
        file: String,
        content: String,
    },
}

pub struct Manager {
    llm: LlmClient,
    config: OrchestratorConfig,
}

impl Manager {
    pub fn new(llm: LlmClient, config: OrchestratorConfig) -> Self {
        Manager { llm, config }
    }

    /// Plans a sub-task into surgical worker instructions.
    /// Uses the Gateway's KeyPool — no direct API key needed for Tier 3.
    pub async fn plan_task(
        &self,
        task: &SubTask,
        l3_rules: &str,
    ) -> Result<Vec<WorkerInstruction>> {
        info!("[T3 Manager] Planning: {}", task.title);

        let rules_section = if !l3_rules.is_empty() {
            format!("\n\nProject Rules (L3):\n{}", l3_rules)
        } else {
            String::new()
        };

        let messages = vec![
            Message::system(format!(
                "You are a Senior Engineer Manager in a coding swarm. \
                CRITICAL RULES:\n\
                1. Only produce SURGICAL edits — find a specific string and replace it\n\
                2. Never suggest full file rewrites\n\
                3. Prefer bash commands for creating new files\n\
                4. Respond ONLY with valid JSON array. No markdown. No explanation.\
                {}",
                rules_section
            )),
            Message::user(format!(
                "Break this task into worker instructions:\n\
                Task: {}\n\
                Description: {}\n\
                Files: {}\n\
                Acceptance: {}\n\n\
                Return JSON array of instructions. Each must be one of:\n\
                {{\"type\":\"bash\",\"title\":\"...\",\"command\":\"...\",\"cwd\":null}}\n\
                {{\"type\":\"edit\",\"title\":\"...\",\"file\":\"...\",\"find\":\"exact text\",\"replace\":\"new text\"}}\n\
                {{\"type\":\"create\",\"title\":\"...\",\"file\":\"...\",\"content\":\"full file content\"}}",
                task.title,
                task.description,
                task.files.join(", "),
                task.acceptance_criterion,
            )),
        ];

        // Uses Gateway KeyPool — free tier keys
        let raw = self.llm.chat_via_gateway(
            &self.config.gateway_url,
            &self.config.hf_token,
            &self.config.tier34_model,
            &self.config.tier34_provider,
            messages,
            Some(2048),
        ).await?;

        let clean = strip_fences(&raw);
        let instructions: Vec<WorkerInstruction> = serde_json::from_str(clean)
            .map_err(|e| anyhow::anyhow!("Manager JSON parse failed: {}. Raw: {}", e, &raw[..300.min(raw.len())]))?;

        info!("[T3 Manager] {} instructions planned", instructions.len());
        Ok(instructions)
    }
}

fn strip_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("```json") {
        if let Some(end) = inner.rfind("```") { return inner[..end].trim(); }
    }
    if let Some(inner) = s.strip_prefix("```") {
        if let Some(end) = inner.rfind("```") { return inner[..end].trim(); }
    }
    s
}
