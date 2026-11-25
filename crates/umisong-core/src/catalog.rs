use crate::{AuthState, ProviderId, ProviderStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ThinkingLevel {
    Off,
    Low,
    Medium,
    #[default]
    High,
    Extra,
    Max,
}

impl ThinkingLevel {
    pub fn label(self) -> &'static str {
        match self {
            ThinkingLevel::Off => "Off",
            ThinkingLevel::Low => "Low",
            ThinkingLevel::Medium => "Medium",
            ThinkingLevel::High => "High",
            ThinkingLevel::Extra => "Extra",
            ThinkingLevel::Max => "Max",
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            ThinkingLevel::Off => "off",
            ThinkingLevel::Low => "low",
            ThinkingLevel::Medium => "medium",
            ThinkingLevel::High => "high",
            ThinkingLevel::Extra => "extra",
            ThinkingLevel::Max => "max",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "off" => ThinkingLevel::Off,
            "low" => ThinkingLevel::Low,
            "medium" => ThinkingLevel::Medium,
            "extra" => ThinkingLevel::Extra,
            "max" => ThinkingLevel::Max,
            _ => ThinkingLevel::High,
        }
    }

    /// Maps to Claude Code `--effort` flag values (`low`, `medium`, `high`, `xhigh`, `max`).
    pub fn as_claude_effort(self) -> Option<&'static str> {
        match self {
            ThinkingLevel::Off => None,
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High => Some("high"),
            ThinkingLevel::Extra => Some("xhigh"),
            ThinkingLevel::Max => Some("max"),
        }
    }

    /// Maps to OpenAI Codex `model_reasoning_effort` (`low`, `medium`, `high`, `xhigh`).
    pub fn as_codex_effort(self) -> Option<&'static str> {
        match self {
            ThinkingLevel::Off => None,
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High => Some("high"),
            ThinkingLevel::Extra | ThinkingLevel::Max => Some("xhigh"),
        }
    }

    /// Maps to Antigravity CLI `--effort` flag values (`low`, `medium`, `high`, `xhigh`, `max`).
    pub fn as_agy_level(self) -> Option<&'static str> {
        match self {
            ThinkingLevel::Off => None,
            ThinkingLevel::Low => Some("low"),
            ThinkingLevel::Medium => Some("medium"),
            ThinkingLevel::High => Some("high"),
            ThinkingLevel::Extra => Some("xhigh"),
            ThinkingLevel::Max => Some("max"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelFamily {
    Claude,
    Gemini,
    Codex,
}

impl ModelFamily {
    pub const ALL: [ModelFamily; 3] = [
        ModelFamily::Claude,
        ModelFamily::Gemini,
        ModelFamily::Codex,
    ];

    pub fn display_name(self) -> &'static str {
        match self {
            ModelFamily::Claude => "Claude",
            ModelFamily::Gemini => "Gemini",
            ModelFamily::Codex => "Codex",
        }
    }

    pub fn section_header(self) -> &'static str {
        match self {
            ModelFamily::Claude => "---- Claude ------",
            ModelFamily::Gemini => "---- Gemini ------",
            ModelFamily::Codex => "---- Codex -------",
        }
    }

    /// Native provider that gets token/billing priority when multiple logged-in accounts
    /// offer a model in this family (e.g., Claude Code beats Antigravity for Claude Sonnet).
    pub fn native_provider(self) -> ProviderId {
        match self {
            ModelFamily::Claude => ProviderId::ClaudeCode,
            ModelFamily::Gemini => ProviderId::Antigravity,
            ModelFamily::Codex => ProviderId::Codex,
        }
    }

    pub fn infer_from_model_id(id: &str, fallback_provider: ProviderId) -> Self {
        let lower = id.to_ascii_lowercase();
        if lower.contains("claude")
            || lower.contains("fable")
            || lower.contains("opus")
            || lower.contains("sonnet")
            || lower.contains("haiku")
        {
            ModelFamily::Claude
        } else if lower.contains("gemini")
            || lower.contains("carbon")
            || lower.contains("neon")
            || lower.contains("argon")
            || lower.contains("hydrogen")
        {
            ModelFamily::Gemini
        } else if lower.contains("gpt")
            || lower.contains("codex")
            || lower.contains("o1")
            || lower.contains("o3")
            || lower.contains("o4")
        {
            ModelFamily::Codex
        } else {
            match fallback_provider {
                ProviderId::ClaudeCode => ModelFamily::Claude,
                ProviderId::Antigravity => ModelFamily::Gemini,
                ProviderId::Codex => ModelFamily::Codex,
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum PlanTier {
    #[default]
    AutoDetect,
    // Claude Code plans
    ClaudeFree,
    ClaudePro,
    ClaudeMax,
    ClaudeUltra,
    ClaudeTeam,
    #[serde(alias = "claude_team_enterprise")]
    ClaudeEnterprise,
    ClaudeEducation,
    ClaudeScientists,
    AnthropicApi,
    // Antigravity / Google AI plans
    GoogleAiFree,
    GoogleAiPlus,
    GoogleAiPro,
    GoogleAiProStudents,
    GoogleAiUltra,
    GoogleAiUltra5x,
    GoogleAiUltra20x,
    GeminiWorkspace,
    GeminiEnterprise,
    GoogleEnterpriseAdc,
    // Codex / OpenAI plans
    ChatGptFree,
    ChatGptGo,
    ChatGptPlus,
    ChatGptPro,
    ChatGptPro100,
    ChatGptPro200,
    ChatGptPro500,
    #[serde(alias = "chatgpt_team_enterprise", alias = "chatgpt_team")]
    ChatGptBusiness,
    ChatGptEnterprise,
    ChatGptRosalind,
    OpenAiApi,
}

impl PlanTier {
    pub fn label(self) -> &'static str {
        match self {
            PlanTier::AutoDetect => "Auto-Detect from CLI",
            PlanTier::ClaudeFree => "Claude Free",
            PlanTier::ClaudePro => "Claude Pro",
            PlanTier::ClaudeMax => "Claude Max",
            PlanTier::ClaudeUltra => "Claude Ultra",
            PlanTier::ClaudeTeam => "Claude Team",
            PlanTier::ClaudeEnterprise => "Claude Enterprise",
            PlanTier::ClaudeEducation => "Claude for Education",
            PlanTier::ClaudeScientists => "Claude for Scientists",
            PlanTier::AnthropicApi => "Anthropic API",
            PlanTier::GoogleAiFree => "Google AI Free",
            PlanTier::GoogleAiPlus => "Google AI Plus",
            PlanTier::GoogleAiPro => "Google AI Pro",
            PlanTier::GoogleAiProStudents => "Google AI Pro for Students",
            PlanTier::GoogleAiUltra => "Google AI Ultra",
            PlanTier::GoogleAiUltra5x => "Google AI Ultra (5x)",
            PlanTier::GoogleAiUltra20x => "Google AI Ultra (20x)",
            PlanTier::GeminiWorkspace => "Gemini with Google Workspace",
            PlanTier::GeminiEnterprise => "Gemini Enterprise",
            PlanTier::GoogleEnterpriseAdc => "Vertex ADC / Enterprise",
            PlanTier::ChatGptFree => "ChatGPT Free",
            PlanTier::ChatGptGo => "ChatGPT Go",
            PlanTier::ChatGptPlus => "ChatGPT Plus",
            PlanTier::ChatGptPro => "ChatGPT Pro",
            PlanTier::ChatGptPro100 => "ChatGPT Pro 100",
            PlanTier::ChatGptPro200 => "ChatGPT Pro 200",
            PlanTier::ChatGptPro500 => "ChatGPT Pro 500",
            PlanTier::ChatGptBusiness => "ChatGPT Business",
            PlanTier::ChatGptEnterprise => "ChatGPT Enterprise",
            PlanTier::ChatGptRosalind => "Rosalind Academic Plan",
            PlanTier::OpenAiApi => "OpenAI API",
        }
    }

    pub fn options_for(provider: ProviderId) -> &'static [PlanTier] {
        match provider {
            ProviderId::ClaudeCode => &[
                PlanTier::AutoDetect,
                PlanTier::ClaudeFree,
                PlanTier::ClaudePro,
                PlanTier::ClaudeMax,
                PlanTier::ClaudeUltra,
                PlanTier::ClaudeTeam,
                PlanTier::ClaudeEnterprise,
                PlanTier::ClaudeEducation,
                PlanTier::ClaudeScientists,
                PlanTier::AnthropicApi,
            ],
            ProviderId::Antigravity => &[
                PlanTier::AutoDetect,
                PlanTier::GoogleAiFree,
                PlanTier::GoogleAiPlus,
                PlanTier::GoogleAiPro,
                PlanTier::GoogleAiProStudents,
                PlanTier::GoogleAiUltra,
                PlanTier::GoogleAiUltra5x,
                PlanTier::GoogleAiUltra20x,
                PlanTier::GeminiWorkspace,
                PlanTier::GeminiEnterprise,
                PlanTier::GoogleEnterpriseAdc,
            ],
            ProviderId::Codex => &[
                PlanTier::AutoDetect,
                PlanTier::ChatGptFree,
                PlanTier::ChatGptGo,
                PlanTier::ChatGptPlus,
                PlanTier::ChatGptPro,
                PlanTier::ChatGptPro100,
                PlanTier::ChatGptPro200,
                PlanTier::ChatGptPro500,
                PlanTier::ChatGptBusiness,
                PlanTier::ChatGptEnterprise,
                PlanTier::ChatGptRosalind,
                PlanTier::OpenAiApi,
            ],
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            PlanTier::AutoDetect => "auto_detect",
            PlanTier::ClaudeFree => "claude_free",
            PlanTier::ClaudePro => "claude_pro",
            PlanTier::ClaudeMax => "claude_max",
            PlanTier::ClaudeUltra => "claude_ultra",
            PlanTier::ClaudeTeam => "claude_team",
            PlanTier::ClaudeEnterprise => "claude_enterprise",
            PlanTier::ClaudeEducation => "claude_education",
            PlanTier::ClaudeScientists => "claude_scientists",
            PlanTier::AnthropicApi => "anthropic_api",
            PlanTier::GoogleAiFree => "google_ai_free",
            PlanTier::GoogleAiPlus => "google_ai_plus",
            PlanTier::GoogleAiPro => "google_ai_pro",
            PlanTier::GoogleAiProStudents => "google_ai_pro_students",
            PlanTier::GoogleAiUltra => "google_ai_ultra",
            PlanTier::GoogleAiUltra5x => "google_ai_ultra_5x",
            PlanTier::GoogleAiUltra20x => "google_ai_ultra_20x",
            PlanTier::GeminiWorkspace => "gemini_workspace",
            PlanTier::GeminiEnterprise => "gemini_enterprise",
            PlanTier::GoogleEnterpriseAdc => "google_enterprise_adc",
            PlanTier::ChatGptFree => "chatgpt_free",
            PlanTier::ChatGptGo => "chatgpt_go",
            PlanTier::ChatGptPlus => "chatgpt_plus",
            PlanTier::ChatGptPro => "chatgpt_pro",
            PlanTier::ChatGptPro100 => "chatgpt_pro_100",
            PlanTier::ChatGptPro200 => "chatgpt_pro_200",
            PlanTier::ChatGptPro500 => "chatgpt_pro_500",
            PlanTier::ChatGptBusiness => "chatgpt_business",
            PlanTier::ChatGptEnterprise => "chatgpt_enterprise",
            PlanTier::ChatGptRosalind => "chatgpt_rosalind",
            PlanTier::OpenAiApi => "openai_api",
        }
    }

    pub fn parse(value: &str) -> Self {
        match value {
            "claude_free" => PlanTier::ClaudeFree,
            "claude_pro" => PlanTier::ClaudePro,
            "claude_max" => PlanTier::ClaudeMax,
            "claude_ultra" => PlanTier::ClaudeUltra,
            "claude_team" => PlanTier::ClaudeTeam,
            "claude_enterprise" | "claude_team_enterprise" => PlanTier::ClaudeEnterprise,
            "claude_education" => PlanTier::ClaudeEducation,
            "claude_scientists" => PlanTier::ClaudeScientists,
            "anthropic_api" => PlanTier::AnthropicApi,
            "google_ai_free" => PlanTier::GoogleAiFree,
            "google_ai_plus" => PlanTier::GoogleAiPlus,
            "google_ai_pro" => PlanTier::GoogleAiPro,
            "google_ai_pro_students" => PlanTier::GoogleAiProStudents,
            "google_ai_ultra" => PlanTier::GoogleAiUltra,
            "google_ai_ultra_5x" => PlanTier::GoogleAiUltra5x,
            "google_ai_ultra_20x" => PlanTier::GoogleAiUltra20x,
            "gemini_workspace" => PlanTier::GeminiWorkspace,
            "gemini_enterprise" => PlanTier::GeminiEnterprise,
            "google_enterprise_adc" => PlanTier::GoogleEnterpriseAdc,
            "chatgpt_free" => PlanTier::ChatGptFree,
            "chatgpt_go" => PlanTier::ChatGptGo,
            "chatgpt_plus" => PlanTier::ChatGptPlus,
            "chatgpt_pro" => PlanTier::ChatGptPro,
            "chatgpt_pro_100" => PlanTier::ChatGptPro100,
            "chatgpt_pro_200" => PlanTier::ChatGptPro200,
            "chatgpt_pro_500" => PlanTier::ChatGptPro500,
            "chatgpt_business" | "chatgpt_team" | "chatgpt_team_enterprise" => {
                PlanTier::ChatGptBusiness
            }
            "chatgpt_enterprise" => PlanTier::ChatGptEnterprise,
            "chatgpt_rosalind" => PlanTier::ChatGptRosalind,
            "openai_api" => PlanTier::OpenAiApi,
            _ => PlanTier::AutoDetect,
        }
    }

    pub fn infer_from_auth(provider: ProviderId, auth: &AuthState) -> Self {
        match auth {
            AuthState::Authenticated {
                method, plan_hint, ..
            } => {
                let raw = format!(
                    "{} {}",
                    method.to_ascii_lowercase(),
                    plan_hint.as_deref().unwrap_or("").to_ascii_lowercase()
                );
                let has = |keys: &[&str]| keys.iter().any(|k| raw.contains(k));
                match provider {
                    ProviderId::ClaudeCode => {
                        if has(&["api", "console", "bedrock", "vertex", "foundry"]) {
                            PlanTier::AnthropicApi
                        } else if has(&["ultra"]) {
                            PlanTier::ClaudeUltra
                        } else if has(&["max"]) {
                            PlanTier::ClaudeMax
                        } else if has(&["scientist"]) {
                            PlanTier::ClaudeScientists
                        } else if has(&["education", "edu", "teacher"]) {
                            PlanTier::ClaudeEducation
                        } else if has(&["enterprise"]) {
                            PlanTier::ClaudeEnterprise
                        } else if has(&["team"]) {
                            PlanTier::ClaudeTeam
                        } else if has(&["free"]) {
                            PlanTier::ClaudeFree
                        } else {
                            PlanTier::ClaudePro
                        }
                    }
                    ProviderId::Antigravity => {
                        if has(&["ultra"]) {
                            if has(&["20x"]) {
                                PlanTier::GoogleAiUltra20x
                            } else if has(&["5x"]) {
                                PlanTier::GoogleAiUltra5x
                            } else {
                                PlanTier::GoogleAiUltra
                            }
                        } else if has(&["adc", "vertex"]) {
                            PlanTier::GoogleEnterpriseAdc
                        } else if has(&["enterprise"]) {
                            PlanTier::GeminiEnterprise
                        } else if has(&["workspace"]) {
                            PlanTier::GeminiWorkspace
                        } else if has(&["student"]) {
                            PlanTier::GoogleAiProStudents
                        } else if has(&["plus"]) {
                            PlanTier::GoogleAiPlus
                        } else if has(&["free"]) {
                            PlanTier::GoogleAiFree
                        } else {
                            PlanTier::GoogleAiPro
                        }
                    }
                    ProviderId::Codex => {
                        if has(&["api"]) {
                            PlanTier::OpenAiApi
                        } else if has(&["rosalind", "academic"]) {
                            PlanTier::ChatGptRosalind
                        } else if has(&["enterprise"]) {
                            PlanTier::ChatGptEnterprise
                        } else if has(&["business", "team"]) {
                            PlanTier::ChatGptBusiness
                        } else if has(&["pro 500", "pro500", "pro_500"]) {
                            PlanTier::ChatGptPro500
                        } else if has(&["pro 200", "pro200", "pro_200"]) {
                            PlanTier::ChatGptPro200
                        } else if has(&["pro 100", "pro100", "pro_100", "prolite", "pro_lite"]) {
                            PlanTier::ChatGptPro100
                        } else if has(&["pro"]) {
                            PlanTier::ChatGptPro
                        } else if has(&["chatgpt go"]) {
                            PlanTier::ChatGptGo
                        } else if has(&["free"]) {
                            PlanTier::ChatGptFree
                        } else {
                            PlanTier::ChatGptPlus
                        }
                    }
                }
            }
            _ => PlanTier::AutoDetect,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnifiedModelEntry {
    pub canonical_id: String,
    pub display_name: String,
    pub family: ModelFamily,
    pub strength_rank: u16,
    pub supported_thinking_levels: Vec<ThinkingLevel>,
    pub default_thinking_level: ThinkingLevel,
    pub available: bool,
    pub unavailable_reason: Option<String>,
    /// Winning prioritized provider that will execute this model (using its account tokens).
    pub primary_provider: ProviderId,
    /// Exact CLI `--model` argument to pass to `primary_provider`.
    pub provider_model_id: String,
    /// Other logged-in providers that could also run this model.
    pub alternate_providers: Vec<ProviderId>,
    pub plan_badge: String,
}

impl UnifiedModelEntry {
    pub fn clamp_thinking_level(&self, requested: ThinkingLevel) -> ThinkingLevel {
        if self.supported_thinking_levels.contains(&requested) {
            requested
        } else {
            self.default_thinking_level
        }
    }

    pub fn label_with_level(&self, level: ThinkingLevel) -> String {
        if !self.available {
            format!("{} (Unavailable)", self.display_name)
        } else {
            let eff = self.clamp_thinking_level(level);
            format!("{} {}", self.display_name, eff.label())
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnifiedModelGroup {
    pub family: ModelFamily,
    pub header: String,
    pub models: Vec<UnifiedModelEntry>,
}

struct CatalogSpec {
    canonical_id: &'static str,
    display_name: &'static str,
    family: ModelFamily,
    strength_rank: u16,
    thinking_levels: &'static [ThinkingLevel],
    default_thinking: ThinkingLevel,
}

const KNOWN_MODELS: &[CatalogSpec] = &[
    // ---- Claude ------
    CatalogSpec {
        canonical_id: "claude-fable-5-2",
        display_name: "Claude Fable 5.2",
        family: ModelFamily::Claude,
        strength_rank: 990,
        thinking_levels: &[ThinkingLevel::Medium, ThinkingLevel::High, ThinkingLevel::Max],
        default_thinking: ThinkingLevel::High,
    },
    CatalogSpec {
        canonical_id: "claude-opus-5-5",
        display_name: "Claude Opus 5.5",
        family: ModelFamily::Claude,
        strength_rank: 950,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::Max,
        ],
        default_thinking: ThinkingLevel::High,
    },
    CatalogSpec {
        canonical_id: "claude-sonnet-5-5",
        display_name: "Claude Sonnet 5.5",
        family: ModelFamily::Claude,
        strength_rank: 880,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ],
        default_thinking: ThinkingLevel::High,
    },
    CatalogSpec {
        canonical_id: "claude-haiku-5-5",
        display_name: "Claude Haiku 5.5",
        family: ModelFamily::Claude,
        strength_rank: 760,
        thinking_levels: &[
            ThinkingLevel::Off,
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ],
        default_thinking: ThinkingLevel::High,
    },
    // ---- Gemini ------
    CatalogSpec {
        canonical_id: "gemini-4-carbon",
        display_name: "Gemini 4 Carbon",
        family: ModelFamily::Gemini,
        strength_rank: 970,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::Extra,
        ],
        default_thinking: ThinkingLevel::High,
    },
    CatalogSpec {
        canonical_id: "gemini-4-neon",
        display_name: "Gemini 4 Neon",
        family: ModelFamily::Gemini,
        strength_rank: 910,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::Extra,
        ],
        default_thinking: ThinkingLevel::High,
    },
    CatalogSpec {
        canonical_id: "gemini-4-argon",
        display_name: "Gemini 4 Argon",
        family: ModelFamily::Gemini,
        strength_rank: 865,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ],
        default_thinking: ThinkingLevel::Medium,
    },
    CatalogSpec {
        canonical_id: "gemini-4-hydrogen",
        display_name: "Gemini 4 Hydrogen",
        family: ModelFamily::Gemini,
        strength_rank: 830,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::Extra,
        ],
        default_thinking: ThinkingLevel::Medium,
    },
    CatalogSpec {
        canonical_id: "gemini-3-8-flash",
        display_name: "Gemini 3.8 Flash",
        family: ModelFamily::Gemini,
        strength_rank: 780,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ],
        default_thinking: ThinkingLevel::Medium,
    },
    // ---- Codex -------
    CatalogSpec {
        canonical_id: "gpt-5-3-codex",
        display_name: "GPT-5.3 Codex",
        family: ModelFamily::Codex,
        strength_rank: 940,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
            ThinkingLevel::Extra,
        ],
        default_thinking: ThinkingLevel::High,
    },
    CatalogSpec {
        canonical_id: "gpt-5-codex",
        display_name: "GPT-5 Codex",
        family: ModelFamily::Codex,
        strength_rank: 870,
        thinking_levels: &[
            ThinkingLevel::Low,
            ThinkingLevel::Medium,
            ThinkingLevel::High,
        ],
        default_thinking: ThinkingLevel::Medium,
    },
    CatalogSpec {
        canonical_id: "gpt-5-codex-mini",
        display_name: "GPT-5 Codex Mini",
        family: ModelFamily::Codex,
        strength_rank: 750,
        thinking_levels: &[ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High],
        default_thinking: ThinkingLevel::Low,
    },
];

struct ProviderOffer {
    provider: ProviderId,
    provider_model_id: String,
    available: bool,
    unavailable_reason: Option<String>,
    plan_badge: String,
}

fn evaluate_provider_offer(
    provider: ProviderId,
    tier: PlanTier,
    canonical_id: &str,
) -> Option<ProviderOffer> {
    let plan_badge = tier.label().to_string();
    match (provider, canonical_id) {
        // --- Claude Code account offerings ---
        (ProviderId::ClaudeCode, "claude-fable-5-2") => {
            let (available, reason) = match tier {
                PlanTier::ClaudeMax
                | PlanTier::ClaudeUltra
                | PlanTier::ClaudeTeam
                | PlanTier::ClaudeEnterprise
                | PlanTier::ClaudeScientists
                | PlanTier::AnthropicApi => {
                    (true, None)
                }
                _ => (false, Some("Requires Claude Max, Enterprise, or API plan".into())),
            };
            Some(ProviderOffer {
                provider,
                provider_model_id: "fable".into(),
                available,
                unavailable_reason: reason,
                plan_badge,
            })
        }
        (ProviderId::ClaudeCode, "claude-opus-5-5") => Some(ProviderOffer {
            provider,
            provider_model_id: "opus".into(),
            available: tier != PlanTier::ClaudeFree,
            unavailable_reason: (tier == PlanTier::ClaudeFree)
                .then(|| "Requires a paid Claude plan".to_string()),
            plan_badge,
        }),
        (ProviderId::ClaudeCode, "claude-sonnet-5-5") => Some(ProviderOffer {
            provider,
            provider_model_id: "sonnet".into(),
            available: true,
            unavailable_reason: None,
            plan_badge,
        }),
        (ProviderId::ClaudeCode, "claude-haiku-5-5") => Some(ProviderOffer {
            provider,
            provider_model_id: "haiku".into(),
            available: true,
            unavailable_reason: None,
            plan_badge,
        }),

        // --- Antigravity account offerings (Gemini + partner Claude models) ---
        (ProviderId::Antigravity, "gemini-4-carbon") => {
            let (available, reason) = match tier {
                PlanTier::GoogleAiUltra
                | PlanTier::GoogleAiUltra5x
                | PlanTier::GoogleAiUltra20x
                | PlanTier::GeminiEnterprise
                | PlanTier::GoogleEnterpriseAdc => (true, None),
                _ => (false, Some("Requires Google AI Ultra or Enterprise plan".into())),
            };
            Some(ProviderOffer {
                provider,
                provider_model_id: "gemini-4-carbon".into(),
                available,
                unavailable_reason: reason,
                plan_badge,
            })
        }
        (ProviderId::Antigravity, "gemini-4-neon") => {
            let (available, reason) = match tier {
                t if t != PlanTier::GoogleAiFree => (true, None),
                _ => (false, Some("Requires Google AI Pro or Ultra plan".into())),
            };
            Some(ProviderOffer {
                provider,
                provider_model_id: "gemini-4-neon".into(),
                available,
                unavailable_reason: reason,
                plan_badge,
            })
        }
        (ProviderId::Antigravity, "gemini-4-argon") => {
            let (available, reason) = match tier {
                t if t != PlanTier::GoogleAiFree => (true, None),
                _ => (false, Some("Requires Google AI Pro or Ultra plan".into())),
            };
            Some(ProviderOffer {
                provider,
                provider_model_id: "gemini-4-argon".into(),
                available,
                unavailable_reason: reason,
                plan_badge,
            })
        }
        (ProviderId::Antigravity, "gemini-4-hydrogen") => Some(ProviderOffer {
            provider,
            provider_model_id: "gemini-4-hydrogen".into(),
            available: true,
            unavailable_reason: None,
            plan_badge,
        }),
        (ProviderId::Antigravity, "gemini-3-8-flash") => Some(ProviderOffer {
            provider,
            provider_model_id: "gemini-3.8-flash".into(),
            available: true,
            unavailable_reason: None,
            plan_badge,
        }),
        // Antigravity partner Claude offerings (lower priority than native Claude Code!)
        (ProviderId::Antigravity, "claude-opus-5-5") => {
            let (available, reason) = match tier {
                t if t != PlanTier::GoogleAiFree => (true, None),
                _ => (
                    false,
                    Some("Requires Claude account or Google AI Pro / Ultra plan".into()),
                ),
            };
            Some(ProviderOffer {
                provider,
                provider_model_id: "claude-opus-5-5".into(),
                available,
                unavailable_reason: reason,
                plan_badge,
            })
        }
        (ProviderId::Antigravity, "claude-sonnet-5-5") => {
            let (available, reason) = match tier {
                t if t != PlanTier::GoogleAiFree => (true, None),
                _ => (false, Some("Requires Google AI Pro or Claude account".into())),
            };
            Some(ProviderOffer {
                provider,
                provider_model_id: "claude-sonnet-5-5".into(),
                available,
                unavailable_reason: reason,
                plan_badge,
            })
        }

        // --- Codex / OpenAI account offerings ---
        (ProviderId::Codex, "gpt-5-3-codex") => {
            let (available, reason) = match tier {
                PlanTier::ChatGptPro
                | PlanTier::ChatGptPro100
                | PlanTier::ChatGptPro200
                | PlanTier::ChatGptPro500
                | PlanTier::ChatGptBusiness
                | PlanTier::ChatGptEnterprise
                | PlanTier::ChatGptRosalind
                | PlanTier::OpenAiApi => {
                    (true, None)
                }
                _ => (false, Some("Requires ChatGPT Pro, Team, or API plan".into())),
            };
            Some(ProviderOffer {
                provider,
                provider_model_id: "gpt-5.3-codex".into(),
                available,
                unavailable_reason: reason,
                plan_badge,
            })
        }
        (ProviderId::Codex, "gpt-5-codex") => Some(ProviderOffer {
            provider,
            provider_model_id: "gpt-5-codex".into(),
            available: tier != PlanTier::ChatGptFree,
            unavailable_reason: (tier == PlanTier::ChatGptFree)
                .then(|| "Requires a paid ChatGPT plan".to_string()),
            plan_badge,
        }),
        (ProviderId::Codex, "gpt-5-codex-mini") => Some(ProviderOffer {
            provider,
            provider_model_id: "gpt-5-codex-mini".into(),
            available: true,
            unavailable_reason: None,
            plan_badge,
        }),
        _ => None,
    }
}

/// Build the unified, family-grouped, strength-ordered model catalog from currently
/// authenticated/ready accounts:
/// - Unauthenticated providers are completely excluded.
/// - When multiple logged-in providers offer the same model (e.g. Claude Code & Antigravity
///   both offering Claude Sonnet 5.5), the model's native provider is prioritized so native
///   account tokens are used first.
/// - Each model carries its own supported `ThinkingLevel` list and default level.
/// - Plan entitlements mark locked higher-tier models as `available = false`, ordered below
///   usable models within each family (sorted by strength descending).
pub fn build_unified_model_catalog(providers: &[ProviderStatus]) -> Vec<UnifiedModelGroup> {
    let active_providers: Vec<(&ProviderStatus, PlanTier)> = providers
        .iter()
        .filter(|st| st.auth.is_ready())
        .map(|st| {
            let effective_tier = match st.settings.plan_tier_override {
                Some(t) if t != PlanTier::AutoDetect => t,
                _ => st.effective_plan_tier,
            };
            (st, effective_tier)
        })
        .collect();

    if active_providers.is_empty() {
        return Vec::new();
    }

    let mut entries_by_family: HashMap<ModelFamily, Vec<UnifiedModelEntry>> = HashMap::new();

    for spec in KNOWN_MODELS {
        let mut offers: Vec<ProviderOffer> = Vec::new();
        for (st, tier) in &active_providers {
            if let Some(offer) = evaluate_provider_offer(st.provider, *tier, spec.canonical_id) {
                offers.push(offer);
            }
        }
        if offers.is_empty() {
            continue;
        }

        let native = spec.family.native_provider();

        // Priority sort:
        // 1. Available over Unavailable
        // 2. Native provider first (e.g. ClaudeCode over Antigravity for Claude models)
        offers.sort_by_key(|o| (!o.available, o.provider != native));

        let winner = offers.remove(0);
        let alternate_providers: Vec<ProviderId> = offers
            .into_iter()
            .filter(|o| o.available)
            .map(|o| o.provider)
            .collect();

        let supported_thinking_levels =
            if winner.provider == ProviderId::Antigravity && spec.canonical_id == "claude-opus-5-5" {
                // Live `agy models` exposes claude-opus-5-5-{low,medium,high} (no `max`)
                vec![
                    ThinkingLevel::Low,
                    ThinkingLevel::Medium,
                    ThinkingLevel::High,
                ]
            } else {
                spec.thinking_levels.to_vec()
            };

        entries_by_family
            .entry(spec.family)
            .or_default()
            .push(UnifiedModelEntry {
                canonical_id: spec.canonical_id.to_string(),
                display_name: spec.display_name.to_string(),
                family: spec.family,
                strength_rank: spec.strength_rank,
                supported_thinking_levels,
                default_thinking_level: spec.default_thinking,
                available: winner.available,
                unavailable_reason: winner.unavailable_reason,
                primary_provider: winner.provider,
                provider_model_id: winner.provider_model_id,
                alternate_providers,
                plan_badge: winner.plan_badge,
            });
    }

    // Include custom models added on logged-in accounts
    for (st, tier) in &active_providers {
        for custom_id in &st.settings.custom_models {
            let trimmed = custom_id.trim();
            if trimmed.is_empty() {
                continue;
            }
            let family = ModelFamily::infer_from_model_id(trimmed, st.provider);
            let list = entries_by_family.entry(family).or_default();
            if list
                .iter()
                .any(|e| e.canonical_id == trimmed || e.provider_model_id == trimmed)
            {
                continue;
            }
            list.push(UnifiedModelEntry {
                canonical_id: format!("custom:{}:{trimmed}", st.provider.as_str()),
                display_name: format!("{trimmed} (Custom)"),
                family,
                strength_rank: 700,
                supported_thinking_levels: vec![
                    ThinkingLevel::Low,
                    ThinkingLevel::Medium,
                    ThinkingLevel::High,
                ],
                default_thinking_level: ThinkingLevel::Medium,
                available: true,
                unavailable_reason: None,
                primary_provider: st.provider,
                provider_model_id: trimmed.to_string(),
                alternate_providers: Vec::new(),
                plan_badge: tier.label().to_string(),
            });
        }
    }

    let mut groups = Vec::new();
    for family in ModelFamily::ALL {
        let Some(mut models) = entries_by_family.remove(&family) else {
            continue;
        };
        if models.is_empty() {
            continue;
        }
        // Available first (strength DESC), then Unavailable (strength DESC)
        models.sort_by(|a, b| {
            b.available
                .cmp(&a.available)
                .then_with(|| b.strength_rank.cmp(&a.strength_rank))
        });

        groups.push(UnifiedModelGroup {
            family,
            header: family.section_header().to_string(),
            models,
        });
    }

    groups
}

pub fn find_unified_entry<'a>(
    groups: &'a [UnifiedModelGroup],
    canonical_id: &str,
) -> Option<&'a UnifiedModelEntry> {
    groups
        .iter()
        .flat_map(|g| g.models.iter())
        .find(|m| m.canonical_id == canonical_id)
}

pub fn strongest_available_model(groups: &[UnifiedModelGroup]) -> Option<&UnifiedModelEntry> {
    groups
        .iter()
        .flat_map(|g| g.models.iter())
        .filter(|m| m.available)
        .max_by_key(|m| m.strength_rank)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ProviderSettings;

    fn make_status(provider: ProviderId, auth: AuthState, tier: PlanTier) -> ProviderStatus {
        ProviderStatus {
            provider,
            installed: !matches!(auth, AuthState::NotInstalled),
            binary_path: Some(format!("/usr/local/bin/{}", provider.cli_binary_name())),
            version: Some("1.0.0".into()),
            auth,
            effective_plan_tier: tier,
            models: Vec::new(),
            active_default_model: String::new(),
            settings: ProviderSettings::default_for(provider),
            notes: None,
        }
    }

    #[test]
    fn hides_logged_out_providers_completely() {
        let statuses = vec![
            make_status(ProviderId::ClaudeCode, AuthState::LoggedOut, PlanTier::AutoDetect),
            make_status(ProviderId::Antigravity, AuthState::NotInstalled, PlanTier::AutoDetect),
            make_status(ProviderId::Codex, AuthState::LoggedOut, PlanTier::AutoDetect),
        ];
        assert!(build_unified_model_catalog(&statuses).is_empty());
    }

    #[test]
    fn prioritizes_claude_tokens_ranks_by_strength_and_sets_per_model_thinking_levels() {
        let statuses = vec![
            make_status(
                ProviderId::ClaudeCode,
                AuthState::Authenticated {
                    method: "claude.ai".into(),
                    account_hint: None,
                    plan_hint: Some("max".into()),
                },
                PlanTier::ClaudeMax,
            ),
            make_status(
                ProviderId::Antigravity,
                AuthState::Authenticated {
                    method: "google_antigravity".into(),
                    account_hint: None,
                    plan_hint: Some("Google AI Pro".into()),
                },
                PlanTier::GoogleAiPro,
            ),
            make_status(ProviderId::Codex, AuthState::LoggedOut, PlanTier::AutoDetect),
        ];

        let groups = build_unified_model_catalog(&statuses);
        assert_eq!(groups.len(), 2);
        assert_eq!(groups[0].family, ModelFamily::Claude);
        assert_eq!(groups[1].family, ModelFamily::Gemini);

        let claude_names: Vec<&str> = groups[0]
            .models
            .iter()
            .map(|m| m.display_name.as_str())
            .collect();
        assert_eq!(
            claude_names,
            vec![
                "Claude Fable 5.2",
                "Claude Opus 5.5",
                "Claude Sonnet 5.5",
                "Claude Haiku 5.5",
            ]
        );

        // Sonnet 5.5 prioritizes Claude Code tokens even though Antigravity is also logged in
        let sonnet = find_unified_entry(&groups, "claude-sonnet-5-5").unwrap();
        assert_eq!(sonnet.primary_provider, ProviderId::ClaudeCode);
        assert_eq!(sonnet.alternate_providers, vec![ProviderId::Antigravity]);

        // Gemini on Google AI Pro: Neon, Argon, Hydrogen, 3.8 Flash usable; Carbon Unavailable
        let gemini_names: Vec<&str> = groups[1]
            .models
            .iter()
            .map(|m| m.display_name.as_str())
            .collect();
        assert_eq!(
            gemini_names,
            vec![
                "Gemini 4 Neon",
                "Gemini 4 Argon",
                "Gemini 4 Hydrogen",
                "Gemini 3.8 Flash",
                "Gemini 4 Carbon",
            ]
        );
        assert!(!groups[1].models[4].available);

        // Per-model thinking levels differ cleanly (Neon has Extra; Argon stops at High)
        let neon = find_unified_entry(&groups, "gemini-4-neon").unwrap();
        assert_eq!(
            neon.supported_thinking_levels,
            vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
                ThinkingLevel::Extra,
            ]
        );
        let argon = find_unified_entry(&groups, "gemini-4-argon").unwrap();
        assert_eq!(
            argon.supported_thinking_levels,
            vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
            ]
        );
        assert_eq!(argon.clamp_thinking_level(ThinkingLevel::Extra), ThinkingLevel::Medium);
    }

    #[test]
    fn antigravity_ultra_unlocks_carbon_at_top_and_routes_partner_sonnet() {
        let statuses = vec![
            make_status(ProviderId::ClaudeCode, AuthState::LoggedOut, PlanTier::AutoDetect),
            make_status(
                ProviderId::Antigravity,
                AuthState::Authenticated {
                    method: "google_antigravity".into(),
                    account_hint: None,
                    plan_hint: Some("Google AI Ultra".into()),
                },
                PlanTier::GoogleAiUltra,
            ),
            make_status(ProviderId::Codex, AuthState::LoggedOut, PlanTier::AutoDetect),
        ];

        let groups = build_unified_model_catalog(&statuses);
        let sonnet = find_unified_entry(&groups, "claude-sonnet-5-5").unwrap();
        assert_eq!(sonnet.primary_provider, ProviderId::Antigravity);
        assert!(sonnet.available);

        let gemini = groups.iter().find(|g| g.family == ModelFamily::Gemini).unwrap();
        assert_eq!(gemini.models[0].display_name, "Gemini 4 Carbon");
        assert!(gemini.models[0].available);
    }

    #[test]
    fn enforces_claude_pro_and_chatgpt_plus_plan_tiers_and_effort_mappings() {
        let statuses = vec![
            make_status(
                ProviderId::ClaudeCode,
                AuthState::Authenticated {
                    method: "claude.ai".into(),
                    account_hint: None,
                    plan_hint: Some("pro".into()),
                },
                PlanTier::ClaudePro,
            ),
            make_status(
                ProviderId::Codex,
                AuthState::Authenticated {
                    method: "chatgpt".into(),
                    account_hint: None,
                    plan_hint: Some("ChatGPT Plus".into()),
                },
                PlanTier::ChatGptPlus,
            ),
        ];

        let groups = build_unified_model_catalog(&statuses);
        assert_eq!(groups.len(), 2); // Claude + Codex (Gemini hidden!)

        let fable = find_unified_entry(&groups, "claude-fable-5-2").unwrap();
        assert!(!fable.available);
        assert!(fable.unavailable_reason.as_deref().unwrap().contains("Max"));

        let codex_53 = find_unified_entry(&groups, "gpt-5-3-codex").unwrap();
        assert!(!codex_53.available);
        let codex_5 = find_unified_entry(&groups, "gpt-5-codex").unwrap();
        assert!(codex_5.available);

        assert_eq!(ThinkingLevel::Max.as_claude_effort(), Some("max"));
        assert_eq!(ThinkingLevel::Extra.as_claude_effort(), Some("xhigh"));
        assert_eq!(ThinkingLevel::Extra.as_codex_effort(), Some("xhigh"));
        assert_eq!(ThinkingLevel::Extra.as_agy_level(), Some("xhigh"));
        assert_eq!(ThinkingLevel::Max.as_agy_level(), Some("max"));
        assert_eq!(ThinkingLevel::Off.as_claude_effort(), None);
    }

    #[test]
    fn gemini_and_agy_models_match_live_per_model_thinking_levels() {
        let statuses = vec![make_status(
            ProviderId::Antigravity,
            AuthState::Authenticated {
                method: "OAuth".into(),
                account_hint: None,
                plan_hint: Some("Google AI Pro".into()),
            },
            PlanTier::GoogleAiPro,
        )];
        let groups = build_unified_model_catalog(&statuses);

        let neon = find_unified_entry(&groups, "gemini-4-neon").unwrap();
        assert_eq!(
            neon.supported_thinking_levels,
            vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
                ThinkingLevel::Extra,
            ]
        );

        let argon = find_unified_entry(&groups, "gemini-4-argon").unwrap();
        assert_eq!(
            argon.supported_thinking_levels,
            vec![ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High]
        );

        let hydrogen = find_unified_entry(&groups, "gemini-4-hydrogen").unwrap();
        assert_eq!(
            hydrogen.supported_thinking_levels,
            vec![
                ThinkingLevel::Low,
                ThinkingLevel::Medium,
                ThinkingLevel::High,
                ThinkingLevel::Extra,
            ]
        );

        let flash = find_unified_entry(&groups, "gemini-3-8-flash").unwrap();
        assert_eq!(
            flash.supported_thinking_levels,
            vec![ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High]
        );

        let agy_opus = find_unified_entry(&groups, "claude-opus-5-5").unwrap();
        assert_eq!(
            agy_opus.supported_thinking_levels,
            vec![ThinkingLevel::Low, ThinkingLevel::Medium, ThinkingLevel::High]
        );
    }

    fn authed(provider: ProviderId, method: &str, plan: Option<&str>) -> AuthState {
        let _ = provider;
        AuthState::Authenticated {
            method: method.into(),
            account_hint: Some(" me@example.com ".into()),
            plan_hint: plan.map(Into::into),
        }
    }

    #[test]
    fn provider_status_plan_label_and_overrides() {
        let mut st = make_status(
            ProviderId::Codex,
            authed(ProviderId::Codex, "chatgpt", Some("ChatGPT Pro 200")),
            PlanTier::ChatGptPro200,
        );
        assert_eq!(st.active_plan_label().as_deref(), Some("ChatGPT Pro 200"));
        assert_eq!(st.account_hint(), Some("me@example.com"));
        assert!(!st.has_manual_plan_override());

        st.settings.plan_tier_override = Some(PlanTier::ChatGptBusiness);
        assert!(st.has_manual_plan_override());
        assert_eq!(st.active_plan_label().as_deref(), Some("ChatGPT Business"));

        let mut no_hint = make_status(
            ProviderId::ClaudeCode,
            authed(ProviderId::ClaudeCode, "claude.ai", None),
            PlanTier::ClaudeMax,
        );
        assert_eq!(no_hint.active_plan_label().as_deref(), Some("Claude Max"));
        no_hint.effective_plan_tier = PlanTier::AutoDetect;
        assert_eq!(no_hint.active_plan_label().as_deref(), Some("Claude Code"));

        let out = make_status(ProviderId::Codex, AuthState::LoggedOut, PlanTier::AutoDetect);
        assert_eq!(out.active_plan_label(), None);
        assert_eq!(out.account_hint(), None);
    }

    #[test]
    fn plan_tier_parse_aliases_and_inference() {
        assert_eq!(PlanTier::parse("claude_team_enterprise"), PlanTier::ClaudeEnterprise);
        assert_eq!(PlanTier::parse("chatgpt_team_enterprise"), PlanTier::ChatGptBusiness);
        for p in ProviderId::ALL {
            for t in PlanTier::options_for(p) {
                if *t != PlanTier::AutoDetect {
                    assert_eq!(PlanTier::parse(t.as_str()), *t);
                }
            }
        }
        let infer = |p, m: &str, h: &str| PlanTier::infer_from_auth(p, &authed(p, m, Some(h)));
        assert_eq!(infer(ProviderId::ClaudeCode, "claude.ai", "Claude Max (20x)"), PlanTier::ClaudeMax);
        assert_eq!(infer(ProviderId::ClaudeCode, "claude.ai", "Claude Ultra"), PlanTier::ClaudeUltra);
        assert_eq!(infer(ProviderId::ClaudeCode, "claude.ai", "Claude Team"), PlanTier::ClaudeTeam);
        assert_eq!(infer(ProviderId::ClaudeCode, "claude.ai", "Amazon Bedrock"), PlanTier::AnthropicApi);
        assert_eq!(infer(ProviderId::Antigravity, "x", "Google AI Ultra (20x)"), PlanTier::GoogleAiUltra20x);
        assert_eq!(infer(ProviderId::Antigravity, "x", "Google AI Plus"), PlanTier::GoogleAiPlus);
        assert_eq!(infer(ProviderId::Antigravity, "x", "Google AI Free"), PlanTier::GoogleAiFree);
        assert_eq!(infer(ProviderId::Codex, "chatgpt", "ChatGPT Pro 100"), PlanTier::ChatGptPro100);
        assert_eq!(infer(ProviderId::Codex, "chatgpt", "ChatGPT Pro"), PlanTier::ChatGptPro);
        assert_eq!(infer(ProviderId::Codex, "chatgpt", "ChatGPT Go"), PlanTier::ChatGptGo);
        assert_eq!(infer(ProviderId::Codex, "chatgpt", "ChatGPT Business"), PlanTier::ChatGptBusiness);
        assert_eq!(infer(ProviderId::Codex, "chatgpt", "ChatGPT Plus"), PlanTier::ChatGptPlus);
    }
}
