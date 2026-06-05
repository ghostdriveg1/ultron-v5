// crates/orchestrator/src/main.rs
//
// Olympus V5 — 4-Tier Swarm Orchestrator CLI
// 100% Rust. Zero Python. Zero SDKs.
// All providers called via direct reqwest (OpenAI-compatible REST API — verified June 2026).
//
// Usage:
//   olympus run "Build a REST API for todo items"
//   olympus health
//   olympus status
//   olympus inject-key --provider groq --model llama-3.3-70b-versatile --key gsk_xxx
//   olympus providers          # list all registered providers
//   olympus list-providers     # same as above

mod config;
mod llm;
mod senator;
mod board;
mod manager;
mod worker;
mod gateway;

use anyhow::Result;
use clap::{Parser, Subcommand};
use colored::Colorize;
use tracing::info;
use tracing_subscriber::EnvFilter;

use config::OrchestratorConfig;
use llm::LlmClient;
use senator::Senator;
use board::Board;
use manager::Manager;
use worker::Worker;
use gateway::GatewayClient;

// ─────────────────────────────────────────────────────────────────────────────
// CLI definition (clap derive)
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "olympus",
    about = "Olympus V5 — 4-Tier Autonomous Coding Swarm",
    version = "5.0.0",
    long_about = "Distributed cognitive swarm. 100% Rust. All providers via OpenAI-compatible REST API."
)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Run the full 4-tier swarm on a project description
    Run {
        /// Project description (what to build)
        project: String,
    },
    /// Check cluster health (all 5 HF Spaces)
    Health,
    /// Full admin status — KeyPool, Tantivy, proxy count
    Status,
    /// Inject an API key into the Gateway KeyPool at runtime
    InjectKey {
        #[arg(long)] provider: String,
        #[arg(long)] model: String,
        #[arg(long)] key: String,
    },
    /// List all OpenAI-compatible providers registered in the Gateway
    Providers,
    /// Run the 3AM Dream State Refactoring on the current workspace
    DreamState {
        /// Directory to analyze (defaults to workspace root)
        #[arg(default_value = ".")]
        path: String,
    },
}

// ─────────────────────────────────────────────────────────────────────────────
// Entry point
// ─────────────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Load .env if present
    let _ = dotenvy::dotenv();

    // Init tracing
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .with_target(false)
        .init();

    let cli = Cli::parse();
    let config = OrchestratorConfig::from_env()?;

    let llm = LlmClient::new()?;
    let gateway = GatewayClient::new(
        config.gateway_url.clone(),
        config.hf_token.clone(),
        config.admin_token.clone(),
    )?;

    match cli.command {
        Commands::Run { project } => run_swarm(project, config, llm, gateway).await?,
        Commands::Health => cmd_health(gateway).await?,
        Commands::Status => cmd_status(gateway).await?,
        Commands::InjectKey { provider, model, key } => {
            cmd_inject_key(gateway, &provider, &model, &key).await?
        }
        Commands::Providers => cmd_providers().await,
        Commands::DreamState { path } => cmd_dream_state(path, config, llm).await?,
    }

    Ok(())
}

// ─────────────────────────────────────────────────────────────────────────────
// Command handlers
// ─────────────────────────────────────────────────────────────────────────────

async fn run_swarm(
    project: String,
    config: OrchestratorConfig,
    llm: LlmClient,
    gateway: GatewayClient,
) -> Result<()> {
    println!("\n{}", "╔══════════════════════════════════════════════════╗".purple().bold());
    println!("{}", "║   OLYMPUS V5 — 4-TIER SWARM ACTIVATED            ║".purple().bold());
    println!("{}", "║   100% Rust · Zero Python · Zero SDKs            ║".purple().bold());
    println!("{}\n", "╚══════════════════════════════════════════════════╝".purple().bold());

    // ── Tier 1: Senator ────────────────────────────────────────────────────
    println!("{}", "[T1 Senator] Analyzing project with high-quality model...".cyan().bold());
    println!("  Provider: {} / {}", config.tier12_provider, config.tier12_model);

    let senator = Senator::new(llm.clone(), config.clone());
    let plan = senator.define_goal(&project).await?;

    println!("  {} Goal: {}", "✓".green(), plan.goal);
    println!("  {} {} epics identified", "✓".green(), plan.epics.len());
    println!("  {} {} L3 rules", "✓".green(), plan.l3_rules.len());

    // ── Tier 2: Board ──────────────────────────────────────────────────────
    println!("\n{}", "[T2 Board] Splitting epics into sub-tasks...".cyan().bold());

    let board = Board::new(llm.clone(), config.clone());
    let mut all_tasks = Vec::new();
    for epic in &plan.epics {
        let tasks = board.split_epic(epic, "").await?;
        println!("  {} '{}' → {} tasks", "✓".green(), &epic[..epic.len().min(50)], tasks.len());
        all_tasks.extend(tasks);
    }
    println!("  {} Total: {} tasks", "✓".green(), all_tasks.len());

    // Collect L3 rules as a string for Manager context
    let l3_rules = plan.l3_rules
        .iter()
        .map(|(k, v)| format!("{}: {}", k, v))
        .collect::<Vec<_>>()
        .join("\n");

    // ── Tier 3 & 4: Manager → Worker ──────────────────────────────────────
    println!("\n{}", "[T3/T4] Manager→Worker pipeline executing...".cyan().bold());
    println!("  Gateway: {} / {}", config.tier34_provider, config.tier34_model);

    let manager = Manager::new(llm.clone(), config.clone());
    let worker = Worker::new(&config, gateway);

    let total = all_tasks.len();
    let mut passed = 0;
    let mut failed = 0;
    let mut rnd_queued = 0;

    for (i, task) in all_tasks.iter().enumerate() {
        println!("\n  [{}/{}] {}", i + 1, total, task.title.bold());

        let instructions = match manager.plan_task(task, &l3_rules, &plan.required_tier).await {
            Ok(instrs) => instrs,
            Err(e) => {
                println!("    {} Manager failed: {}", "✗".red(), e);
                failed += 1;
                continue;
            }
        };

        for instr in &instructions {
            let result = worker.execute(instr).await;
            if result.success {
                println!("    {} {}", "✓".green(), result.title);
                passed += 1;
            } else {
                println!("    {} {}", "✗".red(), result.title);
                if result.rnd_queued {
                    println!("    {} Error queued to R&D self-healing engine", "🧬".yellow());
                    rnd_queued += 1;
                }
                failed += 1;
            }
        }
    }

    println!("\n{}", "═══════════════════════════════════════════════════".purple());
    println!("  {} Swarm run complete!", "✓".green().bold());
    println!("  Passed: {}  Failed: {}  R&D Queued: {}", passed.to_string().green(), failed.to_string().red(), rnd_queued.to_string().yellow());
    println!("{}", "═══════════════════════════════════════════════════".purple());

    Ok(())
}

async fn cmd_health(gateway: GatewayClient) -> Result<()> {
    println!("{}", "Checking cluster health...".cyan());
    let health = gateway.health().await?;
    println!("{}", serde_json::to_string_pretty(&health)?);
    Ok(())
}

async fn cmd_status(gateway: GatewayClient) -> Result<()> {
    println!("{}", "Fetching admin status...".cyan());
    let status = gateway.status().await?;
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

async fn cmd_inject_key(gateway: GatewayClient, provider: &str, model: &str, key: &str) -> Result<()> {
    println!("{}", format!("Injecting {} key for model {}...", provider, model).cyan());
    let result = gateway.inject_key(provider, model, key).await?;
    println!("{}", serde_json::to_string_pretty(&result)?);
    Ok(())
}

async fn cmd_providers() {
    println!("{}", "OpenAI-Compatible Providers (verified June 2026):\n".cyan().bold());
    println!("{:<15} {:<55} {}", "NAME".bold(), "BASE URL".bold(), "NOTES".bold());
    println!("{}", "─".repeat(90));
    for p in llm::KNOWN_PROVIDERS {
        let notes = match p.name {
            "openrouter" => "Routes to Claude, GPT-4, Llama, Gemini, 200+ models",
            "gemini"     => "Official OpenAI-compat endpoint by Google",
            "cohere"     => "Path override: /compatibility/v1",
            "anthropic"  => "Use openrouter with model 'anthropic/claude-*' instead",
            _            => "Native OpenAI-compatible",
        };
        println!("{:<15} {:<55} {}", p.name.green(), p.base_url, notes.dimmed());
    }
    println!("\n{}", "Add any future provider via: POST /v1/admin/providers on the Gateway".yellow());
}

async fn cmd_dream_state(path: String, config: OrchestratorConfig, llm: LlmClient) -> Result<()> {
    println!("{}", "Running 3AM Dream State Refactoring...".cyan().bold());

    // Collect codebase summary (file listing with sizes)
    let summary = collect_codebase_summary(&path);

    let board = Board::new(llm, config);
    let items = board.dream_state_refactor(&summary).await?;

    println!("\n{} refactor items found:\n", items.len());
    for item in &items {
        println!("  [P{}] {}", item.priority.to_string().red().bold(), item.issue.bold());
        println!("       Fix: {}", item.fix);
        println!("       Files: {}", item.files.join(", ").dimmed());
        println!();
    }
    Ok(())
}

fn collect_codebase_summary(path: &str) -> String {
    use walkdir::WalkDir;
    let mut lines = Vec::new();
    for entry in WalkDir::new(path)
        .follow_links(false)
        .max_depth(4)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let p = entry.path().display().to_string();
        // Skip binary, git, target dirs
        if p.contains("target/") || p.contains(".git/") || p.contains("node_modules/") {
            continue;
        }
        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        lines.push(format!("{} ({}B)", p, size));
    }
    lines.join("\n")
}
