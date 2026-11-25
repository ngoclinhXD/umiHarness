pub mod antigravity;
pub mod claude;
pub mod codex;
pub mod discovery;
pub mod installer;

use anyhow::Result;
use async_trait::async_trait;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use umisong_core::{
    AgentEvent, ApprovalDecision, AuthLoginMode, ModelOption, PermissionMode, ProviderId,
    ProviderInstallState, ProviderSettings, ProviderStatus, ThinkingLevel,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LiveProviderInfo {
    pub models: Vec<String>,
    pub plan_hint: Option<String>,
    pub account_hint: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderProbeCache {
    pub discovered_models: Vec<String>,
    pub detected_plan: Option<String>,
    pub detected_account: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CommandSpec {
    pub program: String,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
    pub cwd: Option<PathBuf>,
}

impl CommandSpec {
    pub fn display_command(&self) -> String {
        if self.args.is_empty() {
            self.program.clone()
        } else {
            format!("{} {}", self.program, self.args.join(" "))
        }
    }
}

#[derive(Debug, Clone)]
pub struct TurnExecutionRequest {
    pub workspace_path: PathBuf,
    pub prompt: String,
    pub model: String,
    pub thinking_level: ThinkingLevel,
    pub permission_mode: PermissionMode,
    pub provider_session_id: Option<String>,
    pub settings: ProviderSettings,
}

#[async_trait]
pub trait ProviderAdapter: Send + Sync {
    fn id(&self) -> ProviderId;

    async fn probe_status(
        &self,
        settings: &ProviderSettings,
        cache: &ProviderProbeCache,
    ) -> ProviderStatus;

    fn login_command(
        &self,
        settings: &ProviderSettings,
        mode: AuthLoginMode,
    ) -> Result<CommandSpec>;

    async fn logout(&self, settings: &ProviderSettings) -> Result<()>;

    /// Live-discover models / plan / account for the signed-in user (may be slow).
    async fn discover_live_info(&self, _settings: &ProviderSettings) -> Option<LiveProviderInfo> {
        None
    }

    async fn install_cli(
        &self,
        on_progress: &(dyn Fn(ProviderInstallState) + Send + Sync),
    ) -> Result<PathBuf> {
        crate::installer::install_provider_cli(self.id(), on_progress).await
    }

    async fn run_turn(
        &self,
        req: TurnExecutionRequest,
        tx: mpsc::UnboundedSender<AgentEvent>,
        approval_rx: mpsc::UnboundedReceiver<ApprovalDecision>,
        cancel: CancellationToken,
    ) -> Result<()>;
}

pub struct ProviderRegistry {
    adapters: HashMap<ProviderId, Arc<dyn ProviderAdapter>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        let mut adapters: HashMap<ProviderId, Arc<dyn ProviderAdapter>> = HashMap::new();
        adapters.insert(ProviderId::ClaudeCode, Arc::new(claude::ClaudeCodeAdapter));
        adapters.insert(
            ProviderId::Antigravity,
            Arc::new(antigravity::AntigravityAdapter),
        );
        adapters.insert(ProviderId::Codex, Arc::new(codex::CodexAdapter));

        Self { adapters }
    }

    pub fn with_adapters(adapters: HashMap<ProviderId, Arc<dyn ProviderAdapter>>) -> Self {
        Self { adapters }
    }

    pub fn adapter(&self, provider: ProviderId) -> Arc<dyn ProviderAdapter> {
        Arc::clone(self.adapters.get(&provider).expect("registered provider"))
    }
}

/// Merge built-in plan model options with live-discovered models and user custom models,
/// preserving ordering and deduplicating by model ID.
pub fn merge_model_options(
    mut base: Vec<ModelOption>,
    discovered: &[String],
    custom: &[String],
) -> Vec<ModelOption> {
    for id in discovered {
        let trimmed = id.trim();
        if trimmed.is_empty() || base.iter().any(|m| m.id == trimmed) {
            continue;
        }
        base.push(ModelOption {
            id: trimmed.to_string(),
            label: trimmed.to_string(),
            tier_hint: "Discovered from account".to_string(),
            is_plan_default: false,
            discovered_live: true,
        });
    }
    for id in custom {
        let trimmed = id.trim();
        if trimmed.is_empty() || base.iter().any(|m| m.id == trimmed) {
            continue;
        }
        base.push(ModelOption {
            id: trimmed.to_string(),
            label: trimmed.to_string(),
            tier_hint: "Custom model".to_string(),
            is_plan_default: false,
            discovered_live: false,
        });
    }
    base
}

/// Extract a compact one-line preview of a JSON tool input payload.
pub fn summarize_json_input(val: &serde_json::Value) -> String {
    if let Some(s) = val.as_str() {
        return truncate_chars(s, 140);
    }
    if let Some(obj) = val.as_object() {
        for key in [
            "CommandLine",
            "command",
            "cmd",
            "Path",
            "TargetFile",
            "DirectoryPath",
            "SearchDirectory",
            "SearchPath",
            "path",
            "file_path",
            "file",
            "Query",
            "Pattern",
            "pattern",
            "query",
            "Url",
            "url",
            "description",
        ] {
            if let Some(v) = obj.get(key) {
                if let Some(s) = v.as_str() {
                    return truncate_chars(s, 130);
                }
            }
        }
    }
    truncate_chars(&val.to_string(), 140)
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    let single_line = s.replace('\n', " ");
    let mut chars = single_line.chars();
    let collected: String = chars.by_ref().take(max).collect();
    if chars.next().is_some() {
        format!("{collected}…")
    } else {
        collected
    }
}
