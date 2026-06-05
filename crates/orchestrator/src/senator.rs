// crates/orchestrator/src/senator.rs
//
// Tier 1: The Senator
// Uses a high-quality model (Gemini/OpenRouter) via direct reqwest call.
// No Python. No SDK. Just HTTP POST to the verified OpenAI-compatible endpoint.

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::llm::{LlmClient, Message};
use crate::config::OrchestratorConfig;

#[derive(Debug, Serialize, Deserialize)]
pub struct ProjectPlan {
    pub goal: String,
    pub epics: Vec<String>,
    pub l3_rules: std::collections::HashMap<String, String>,
    pub required_tier: String,
}

pub struct Senator {
    llm: LlmClient,
    config: OrchestratorConfig,
}

impl Senator {
    pub fn new(llm: LlmClient, config: OrchestratorConfig) -> Self {
        Senator { llm, config }
    }

    /// Takes a project description → returns structured goal + epics + L3 rules.
    /// Calls Gemini (or any configured provider) directly via reqwest.
    pub async fn define_goal(&self, project: &str) -> Result<ProjectPlan> {
        info!("[T1 Senator] Analyzing project: {:.60}...", project);

        let messages = vec![
            Message::system(
                "You are The Senator — the top-level AI strategist in a 4-tier coding swarm. \
                Respond ONLY with valid JSON. No markdown. No preamble. No explanation."
            ),
            Message::user(format!(
                "Analyze this project and produce a JSON object with exactly these fields:\n\
                {{\n\
                  \"goal\": \"one sentence goal\",\n\
                  \"epics\": [\"epic 1\", \"epic 2\", ...],\n\
                  \"l3_rules\": {{\"rule_name\": \"rule_content\", ...}},\n\
                  \"required_tier\": \"premium | fast | cheap\"\n\
                }}\n\n\
                Project: {}\n\n\
                Requirements:\n\
                - 3-7 epics\n\
                - 3-5 L3 architectural rules\n\
                - required_tier: evaluate project complexity. Use 'premium' for complex reasoning/architecture, 'fast' for standard business logic, 'cheap' for boilerplate/simple tasks.\n\
                - Be specific and actionable",
                project
            )),
        ];

        let raw = self.llm.chat(
            &self.config.tier12_provider,
            &self.config.tier12_api_key,
            &self.config.tier12_model,
            messages,
            Some(1024),
            Some(0.2),
        ).await?;

        // Strip markdown code fences if the model includes them
        let clean = strip_code_fences(&raw);
        let plan: ProjectPlan = serde_json::from_str(clean)
            .map_err(|e| anyhow::anyhow!("Senator JSON parse failed: {}. Raw: {}", e, &raw[..200.min(raw.len())]))?;

        info!("[T1 Senator] Goal: {}", plan.goal);
        info!("[T1 Senator] {} epics, {} L3 rules", plan.epics.len(), plan.l3_rules.len());
        Ok(plan)
    }
}

fn strip_code_fences(s: &str) -> &str {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix("```json") {
        if let Some(end) = inner.rfind("```") {
            return inner[..end].trim();
        }
    }
    if let Some(inner) = s.strip_prefix("```") {
        if let Some(end) = inner.rfind("```") {
            return inner[..end].trim();
        }
    }
    s
}
