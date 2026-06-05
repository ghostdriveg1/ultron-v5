// crates/orchestrator/src/board.rs
//
// Tier 2: The Board of Directors
// Splits Senator epics into concrete sub-tasks.
// Runs the 3:00 AM Dream State Refactoring (holistic cleanup pass).
// Uses same high-quality model as Senator via direct reqwest.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::llm::{LlmClient, Message};
use crate::config::OrchestratorConfig;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SubTask {
    pub task_id: String,
    pub title: String,
    pub description: String,
    pub files: Vec<String>,
    pub acceptance_criterion: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RefactorItem {
    pub priority: u8,
    pub issue: String,
    pub fix: String,
    pub files: Vec<String>,
}

pub struct Board {
    llm: LlmClient,
    config: OrchestratorConfig,
}

impl Board {
    pub fn new(llm: LlmClient, config: OrchestratorConfig) -> Self {
        Board { llm, config }
    }

    /// Split a high-level epic into 3-8 concrete sub-tasks for Managers.
    pub async fn split_epic(&self, epic: &str, codebase_context: &str) -> Result<Vec<SubTask>> {
        info!("[T2 Board] Splitting epic: {:.60}...", epic);

        let context_section = if !codebase_context.is_empty() {
            format!("\nCodebase context:\n{}", codebase_context)
        } else {
            String::new()
        };

        let messages = vec![
            Message::system(
                "You are a Board Director in a 4-tier coding swarm. \
                Respond ONLY with a valid JSON array. No markdown. No explanation."
            ),
            Message::user(format!(
                "Split this epic into 3-8 concrete sub-tasks. \
                Each task must be completable in under 30 minutes of coding.\n\n\
                Epic: {}{}\n\n\
                Respond with a JSON array:\n\
                [\n\
                  {{\n\
                    \"task_id\": \"T001\",\n\
                    \"title\": \"Short title\",\n\
                    \"description\": \"What to do\",\n\
                    \"files\": [\"path/to/file.rs\"],\n\
                    \"acceptance_criterion\": \"How to verify it works\"\n\
                  }}\n\
                ]",
                epic, context_section
            )),
        ];

        let raw = self.llm.chat(
            &self.config.tier12_provider,
            &self.config.tier12_api_key,
            &self.config.tier12_model,
            messages,
            Some(2048),
            Some(0.3),
        ).await?;

        let clean = strip_fences(&raw);
        let tasks: Vec<SubTask> = serde_json::from_str(clean)
            .map_err(|e| anyhow::anyhow!("Board JSON parse failed: {}. Raw: {}", e, &raw[..200.min(raw.len())]))?;

        info!("[T2 Board] Epic split into {} sub-tasks", tasks.len());
        Ok(tasks)
    }

    /// 3:00 AM Dream State Refactoring — holistic cleanup pass.
    /// Prevents code rot by identifying structural issues across the whole codebase.
    pub async fn dream_state_refactor(&self, codebase_summary: &str) -> Result<Vec<RefactorItem>> {
        info!("[T2 Board] Running Dream State Refactoring analysis...");

        let messages = vec![
            Message::system(
                "You are performing the 3AM Dream State Refactoring — a holistic code quality pass. \
                Identify real structural issues. Respond ONLY with valid JSON array. No markdown."
            ),
            Message::user(format!(
                "Analyze this codebase summary. Identify the top issues:\n\
                1. Code rot (duplicates, dead code, bloated functions)\n\
                2. Missing abstractions\n\
                3. Performance bottlenecks\n\n\
                Codebase:\n{}\n\n\
                Return JSON array sorted by priority (1=highest):\n\
                [{{\"priority\":1,\"issue\":\"...\",\"fix\":\"...\",\"files\":[...]}}]",
                codebase_summary
            )),
        ];

        let raw = self.llm.chat(
            &self.config.tier12_provider,
            &self.config.tier12_api_key,
            &self.config.tier12_model,
            messages,
            Some(2048),
            Some(0.1),
        ).await?;

        let clean = strip_fences(&raw);
        let items: Vec<RefactorItem> = serde_json::from_str(clean)
            .map_err(|e| anyhow::anyhow!("Board refactor JSON parse failed: {}", e))?;

        info!("[T2 Board] Dream State found {} refactor items", items.len());
        Ok(items)
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
