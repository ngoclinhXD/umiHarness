use crate::PlanTier;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderId {
    ClaudeCode,
    Antigravity,
    Codex,
}

impl ProviderId {
    pub const ALL: [ProviderId; 3] = [
        ProviderId::ClaudeCode,
        ProviderId::Antigravity,
        ProviderId::Codex,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            ProviderId::ClaudeCode => "Claude Code",
            ProviderId::Antigravity => "Antigravity",
            ProviderId::Codex => "Codex",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            ProviderId::ClaudeCode => "Claude",
            ProviderId::Antigravity => "AGY",
            ProviderId::Codex => "Codex",
        }
    }

    pub fn cli_binary_name(self) -> &'static str {
        match self {
            ProviderId::ClaudeCode => "claude",
            ProviderId::Antigravity => "agy",
            ProviderId::Codex => "codex",
        }
    }

    pub fn install_hint(self) -> &'static str {
        match self {
            ProviderId::ClaudeCode => "npm install -g @anthropic-ai/claude-code",
            ProviderId::Antigravity => "Install Antigravity CLI from https://antigravity.google/docs/cli/reference",
            ProviderId::Codex => "npm install -g @openai/codex",
        }
    }

    pub fn plan_description(self) -> &'static str {
        match self {
            ProviderId::ClaudeCode => "Claude Free / Pro / Max / Ultra / Team / Enterprise or Anthropic API",
            ProviderId::Antigravity => "Google AI Free / Plus / Pro / Ultra / Gemini Enterprise or Vertex ADC",
            ProviderId::Codex => "ChatGPT Free / Go / Plus / Pro / Business / Enterprise or OpenAI API",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ProviderId::ClaudeCode => "claude_code",
            ProviderId::Antigravity => "antigravity",
            ProviderId::Codex => "codex",
        }
    }

    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "claude_code" | "claude" => Some(ProviderId::ClaudeCode),
            "antigravity" | "agy" => Some(ProviderId::Antigravity),
            "codex" | "openai" => Some(ProviderId::Codex),
            _ => None,
        }
    }
}

impl fmt::Display for ProviderId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.display_name())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum AuthState {
    NotInstalled,
    LoggedOut,
    Authenticated {
        method: String,
        account_hint: Option<String>,
        plan_hint: Option<String>,
    },
    Unknown {
        detail: String,
    },
}

impl AuthState {
    pub fn is_ready(&self) -> bool {
        matches!(self, AuthState::Authenticated { .. })
    }

    pub fn badge_label(&self) -> String {
        match self {
            AuthState::NotInstalled => "CLI Missing".to_string(),
            AuthState::LoggedOut => "Signed Out".to_string(),
            AuthState::Authenticated {
                method, plan_hint, ..
            } => match plan_hint {
                Some(plan) if !plan.is_empty() => format!("Signed In · {plan}"),
                _ => format!("Signed In · {method}"),
            },
            AuthState::Unknown { .. } => "Check Auth".to_string(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthLoginMode {
    BrowserOAuth,
    DeviceCode,
    ApiKey,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelOption {
    /// Empty string means "use whatever default model comes with the account's plan".
    pub id: String,
    pub label: String,
    pub tier_hint: String,
    pub is_plan_default: bool,
    pub discovered_live: bool,
}

impl ModelOption {
    pub fn plan_default(plan_hint: &str) -> Self {
        Self {
            id: String::new(),
            label: "Account Plan Default".to_string(),
            tier_hint: plan_hint.to_string(),
            is_plan_default: true,
            discovered_live: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderSettings {
    pub provider: ProviderId,
    pub binary_override: Option<String>,
    pub default_model: String,
    pub custom_models: Vec<String>,
    pub api_key_override: Option<String>,
    #[serde(default)]
    pub plan_tier_override: Option<PlanTier>,
}

impl ProviderSettings {
    pub fn default_for(provider: ProviderId) -> Self {
        Self {
            provider,
            binary_override: None,
            default_model: String::new(),
            custom_models: Vec::new(),
            api_key_override: None,
            plan_tier_override: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderStatus {
    pub provider: ProviderId,
    pub installed: bool,
    pub binary_path: Option<String>,
    pub version: Option<String>,
    pub auth: AuthState,
    #[serde(default)]
    pub effective_plan_tier: PlanTier,
    pub models: Vec<ModelOption>,
    pub active_default_model: String,
    pub settings: ProviderSettings,
    pub notes: Option<String>,
}

impl ProviderStatus {
    /// Human-readable plan the signed-in user actually has (None when not signed in).
    pub fn active_plan_label(&self) -> Option<String> {
        let AuthState::Authenticated { plan_hint, .. } = &self.auth else {
            return None;
        };
        if let Some(tier) = self.settings.plan_tier_override {
            if tier != PlanTier::AutoDetect {
                return Some(tier.label().to_string());
            }
        }
        if let Some(hint) = plan_hint.as_deref().map(str::trim).filter(|s| !s.is_empty()) {
            return Some(hint.to_string());
        }
        if self.effective_plan_tier != PlanTier::AutoDetect {
            return Some(self.effective_plan_tier.label().to_string());
        }
        Some(self.provider.display_name().to_string())
    }

    pub fn account_hint(&self) -> Option<&str> {
        match &self.auth {
            AuthState::Authenticated { account_hint, .. } => account_hint
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty()),
            _ => None,
        }
    }

    pub fn has_manual_plan_override(&self) -> bool {
        self.settings
            .plan_tier_override
            .is_some_and(|t| t != PlanTier::AutoDetect)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PermissionMode {
    PlanReadOnly,
    #[default]
    AskSafe,
    FullAuto,
}

impl PermissionMode {
    pub const ALL: [PermissionMode; 3] = [
        PermissionMode::PlanReadOnly,
        PermissionMode::AskSafe,
        PermissionMode::FullAuto,
    ];

    pub fn label(self) -> &'static str {
        match self {
            PermissionMode::PlanReadOnly => "Plan (Read-Only)",
            PermissionMode::AskSafe => "Safe (Ask)",
            PermissionMode::FullAuto => "Full Auto",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            PermissionMode::PlanReadOnly => "Plan",
            PermissionMode::AskSafe => "Safe",
            PermissionMode::FullAuto => "Auto",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PermissionMode::PlanReadOnly => "plan_read_only",
            PermissionMode::AskSafe => "ask_safe",
            PermissionMode::FullAuto => "full_auto",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "plan_read_only" => PermissionMode::PlanReadOnly,
            "full_auto" => PermissionMode::FullAuto,
            _ => PermissionMode::AskSafe,
        }
    }
}
