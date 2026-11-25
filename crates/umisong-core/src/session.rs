use crate::{FileDiff, PermissionMode, ProviderId, ThinkingLevel, ToolKind};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub path: String,
    pub is_git_repo: bool,
    pub git_branch: Option<String>,
    pub last_opened_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionSummary {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub active_provider: ProviderId,
    pub active_model: String,
    pub permission_mode: PermissionMode,
    pub turn_count: usize,
    pub updated_at: DateTime<Utc>,
    #[serde(default)]
    pub pinned: bool,
    #[serde(default)]
    pub unread: bool,
    #[serde(default)]
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionDetail {
    pub id: String,
    pub workspace_id: String,
    pub title: String,
    pub active_provider: ProviderId,
    pub active_model: String,
    pub permission_mode: PermissionMode,
    pub provider_session_ids: HashMap<ProviderId, String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    pub turns: Vec<TurnRecord>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnStatus {
    Running,
    Completed,
    Cancelled,
    Failed,
}

impl TurnStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            TurnStatus::Running => "running",
            TurnStatus::Completed => "completed",
            TurnStatus::Cancelled => "cancelled",
            TurnStatus::Failed => "failed",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "running" => TurnStatus::Running,
            "cancelled" => TurnStatus::Cancelled,
            "failed" => TurnStatus::Failed,
            _ => TurnStatus::Completed,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallRecord {
    pub call_id: String,
    pub name: String,
    pub kind: ToolKind,
    pub input_summary: String,
    pub output_text: String,
    pub status: String,
    pub success: Option<bool>,
    pub duration_ms: Option<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TurnRecord {
    pub id: String,
    pub session_id: String,
    pub seq: i64,
    pub provider: ProviderId,
    pub model: String,
    pub resolved_model: Option<String>,
    pub prompt: String,
    pub thought_text: String,
    pub response_text: String,
    pub status: TurnStatus,
    pub error_message: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub handoff_from_provider: Option<ProviderId>,
    pub handoff_from_turn_id: Option<String>,
    pub tool_calls: Vec<ToolCallRecord>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CompareIsolationMode {
    ReadOnly,
    GitWorktree,
}

impl CompareIsolationMode {
    pub fn label(self) -> &'static str {
        match self {
            CompareIsolationMode::ReadOnly => "Read-Only Compare",
            CompareIsolationMode::GitWorktree => "Isolated Git Worktrees",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            CompareIsolationMode::ReadOnly => "read_only",
            CompareIsolationMode::GitWorktree => "git_worktree",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "git_worktree" => CompareIsolationMode::GitWorktree,
            _ => CompareIsolationMode::ReadOnly,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareTarget {
    pub provider: ProviderId,
    pub model: String,
    #[serde(default)]
    pub thinking_level: ThinkingLevel,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareColumnState {
    pub provider: ProviderId,
    pub model: String,
    pub resolved_model: Option<String>,
    pub status: TurnStatus,
    pub thought_text: String,
    pub response_text: String,
    pub tool_calls: Vec<ToolCallRecord>,
    pub error_message: Option<String>,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: Option<f64>,
    pub duration_ms: Option<u64>,
    pub worktree_path: Option<String>,
    pub diff_patch: Option<String>,
    pub diffs: Vec<FileDiff>,
    pub adopted: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompareRun {
    pub id: String,
    pub workspace_id: String,
    pub session_id: Option<String>,
    pub prompt: String,
    pub isolation_mode: CompareIsolationMode,
    pub columns: Vec<CompareColumnState>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub dark_theme: bool,
    #[serde(default = "default_true")]
    pub auto_install_clis: bool,
    #[serde(default)]
    pub last_canonical_model: Option<String>,
    #[serde(default)]
    pub last_thinking_level: Option<ThinkingLevel>,
    #[serde(default)]
    pub per_model_thinking: HashMap<String, ThinkingLevel>,
}

fn default_true() -> bool {
    true
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            dark_theme: true,
            auto_install_clis: true,
            last_canonical_model: None,
            last_thinking_level: None,
            per_model_thinking: HashMap::new(),
        }
    }
}
