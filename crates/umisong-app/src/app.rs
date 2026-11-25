use crate::client::{DaemonClient, InboundMessage, RequestTag};
use crate::theme::{LogoTextures, Palette, apply_theme, install_system_fonts, paint_status_dot};
use crate::views::{accounts, chat, compare, diff_viewer, sidebar};
use crate::widgets::tool_card::PendingApprovalUi;
use egui::{Margin, RichText, Stroke};
use std::collections::HashMap;
use umisong_core::{
    AgentEvent, AppSettings, AuthLoginMode, CompareIsolationMode, CompareRun, DaemonNotification,
    DaemonRequest, DaemonResponse, DiffScope, GitDiffSnapshot, PermissionMode, PlanTier,
    ProviderId, ProviderInstallState, ProviderStatus, SessionDetail, SessionSummary, ThinkingLevel,
    ToolCallRecord, TurnExitReason, TurnRecord, TurnStatus, UnifiedModelGroup, Workspace,
    build_unified_model_catalog, find_unified_entry, strongest_available_model,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveView {
    Chat,
    Compare,
    Diffs,
    Accounts,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompareColumnTab {
    Response,
    Activity,
    Diff,
}

#[derive(Debug, Clone, Default)]
pub struct ProviderDraft {
    pub binary_override: String,
    pub default_model: String,
    pub custom_models: Vec<String>,
    pub api_key_override: String,
    pub plan_tier_override: PlanTier,
}

#[derive(Debug, Clone)]
pub struct AuthTerminalUi {
    pub auth_session_id: String,
    pub provider: ProviderId,
    pub command: String,
    pub screen_lines: Vec<String>,
    pub detected_urls: Vec<String>,
    pub exited: bool,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone)]
pub struct HandoffModalState {
    pub up_to_turn_id: Option<String>,
    pub target_canonical_id: String,
    pub target_provider: ProviderId,
    pub target_model: String,
    pub target_thinking_level: ThinkingLevel,
    pub instruction: String,
    pub create_new_session: bool,
}

pub struct UmisongApp {
    pub client: DaemonClient,
    pub logo_textures: LogoTextures,
    pub daemon_connected: bool,
    pub daemon_embedded: bool,
    pub settings: AppSettings,
    pub last_applied_dark: Option<bool>,

    pub active_view: ActiveView,
    pub providers: HashMap<ProviderId, ProviderStatus>,
    pub provider_install_states: HashMap<ProviderId, ProviderInstallState>,
    pub provider_drafts: HashMap<ProviderId, ProviderDraft>,
    pub accounts_advanced_open: HashMap<ProviderId, bool>,

    pub workspaces: Vec<Workspace>,
    pub active_workspace_id: Option<String>,

    pub sessions: Vec<SessionSummary>,
    pub active_session: Option<SessionDetail>,
    pub session_search: String,
    pub renaming_session: Option<(String, String)>,
    pub pending_delete_session: Option<(String, String)>,
    pub show_archived_sessions: bool,

    pub selected_canonical_model: String,
    pub selected_provider: ProviderId,
    pub selected_model: String,
    pub selected_thinking_level: ThinkingLevel,
    pub per_model_thinking: HashMap<String, ThinkingLevel>,
    pub selected_permission: PermissionMode,
    pub chat_input: String,
    pub pending_approvals: Vec<PendingApprovalUi>,

    pub compare_runs: Vec<CompareRun>,
    pub active_compare_id: Option<String>,
    pub compare_prompt: String,
    pub compare_enabled: HashMap<ProviderId, bool>,
    pub compare_models: HashMap<ProviderId, String>,
    pub compare_thinking_levels: HashMap<ProviderId, ThinkingLevel>,
    pub compare_isolation: CompareIsolationMode,
    pub compare_col_tabs: HashMap<(String, ProviderId), CompareColumnTab>,

    pub diff_scope: DiffScope,
    pub git_diff: Option<GitDiffSnapshot>,
    pub selected_diff_file: Option<String>,
    pub diff_file_filter: String,
    pub pending_revert_file: Option<String>,

    pub auth_terminal: Option<AuthTerminalUi>,
    pub auth_input_line: String,
    pub handoff_modal: Option<HandoffModalState>,

    pub toast: Option<(String, bool)>,
    pending_first_prompt: Option<String>,
    settings_loaded: bool,
    providers_loaded: bool,
    startup_auto_install_checked: bool,
}

impl UmisongApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        install_system_fonts(&cc.egui_ctx);
        let preview_mode = std::env::var("UMISONG_PREVIEW").ok();
        let client = if preview_mode.is_some() {
            DaemonClient::disconnected_offline()
        } else {
            DaemonClient::connect(cc.egui_ctx.clone())
        };
        let logo_textures = LogoTextures::load(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, true);

        let mut compare_enabled = HashMap::new();
        for pid in ProviderId::ALL {
            compare_enabled.insert(pid, true);
        }

        let mut app = Self {
            client,
            logo_textures,
            daemon_connected: preview_mode.is_some(),
            daemon_embedded: true,
            settings: AppSettings::default(),
            last_applied_dark: Some(true),
            active_view: ActiveView::Chat,
            providers: HashMap::new(),
            provider_install_states: HashMap::new(),
            provider_drafts: HashMap::new(),
            accounts_advanced_open: HashMap::new(),
            workspaces: Vec::new(),
            active_workspace_id: None,
            sessions: Vec::new(),
            active_session: None,
            session_search: String::new(),
            renaming_session: None,
            pending_delete_session: None,
            show_archived_sessions: false,
            selected_canonical_model: String::new(),
            selected_provider: ProviderId::ClaudeCode,
            selected_model: String::new(),
            selected_thinking_level: ThinkingLevel::High,
            per_model_thinking: HashMap::new(),
            selected_permission: PermissionMode::AskSafe,
            chat_input: String::new(),
            pending_approvals: Vec::new(),
            compare_runs: Vec::new(),
            active_compare_id: None,
            compare_prompt: String::new(),
            compare_enabled,
            compare_models: HashMap::new(),
            compare_thinking_levels: HashMap::new(),
            compare_isolation: CompareIsolationMode::ReadOnly,
            compare_col_tabs: HashMap::new(),
            diff_scope: DiffScope::All,
            git_diff: None,
            selected_diff_file: None,
            diff_file_filter: String::new(),
            pending_revert_file: None,
            auth_terminal: None,
            auth_input_line: String::new(),
            handoff_modal: None,
            toast: None,
            pending_first_prompt: None,
            settings_loaded: false,
            providers_loaded: false,
            startup_auto_install_checked: false,
        };

        if let Some(mode) = preview_mode {
            app.apply_preview_fixture(&cc.egui_ctx, &mode);
        }
        app
    }

    fn apply_preview_fixture(&mut self, ctx: &egui::Context, mode: &str) {
        use umisong_core::{AuthState, ProviderSettings};

        let mk_prov = |pid: ProviderId, ver: &str, plan: PlanTier, plan_label: &str| ProviderStatus {
            provider: pid,
            installed: true,
            binary_path: Some(format!("/usr/local/bin/{}", pid.cli_binary_name())),
            version: Some(ver.to_string()),
            auth: AuthState::Authenticated {
                method: "OAuth".into(),
                account_hint: Some("ngoclin.h@umika.dev".into()),
                plan_hint: Some(plan_label.into()),
            },
            effective_plan_tier: plan,
            models: Vec::new(),
            active_default_model: String::new(),
            settings: ProviderSettings::default_for(pid),
            notes: None,
        };

        self.providers.insert(
            ProviderId::ClaudeCode,
            mk_prov(ProviderId::ClaudeCode, "v2.1.185", PlanTier::ClaudePro, "Claude Pro"),
        );
        self.providers.insert(
            ProviderId::Antigravity,
            mk_prov(ProviderId::Antigravity, "v1.3.5", PlanTier::GoogleAiPro, "Google AI Pro"),
        );
        self.providers.insert(
            ProviderId::Codex,
            mk_prov(ProviderId::Codex, "v0.161.0", PlanTier::ChatGptPlus, "ChatGPT Plus"),
        );

        let ws = Workspace {
            id: "ws-preview".into(),
            name: "Umisong Harness".into(),
            path: "/Users/ngoclin.h/Documents/code/Umisong Harness".into(),
            is_git_repo: true,
            git_branch: Some("main".into()),
            last_opened_at: chrono::Utc::now(),
        };
        self.active_workspace_id = Some(ws.id.clone());
        self.workspaces = vec![ws];
        self.sync_selected_model_from_catalog();

        if mode == "model_menu" {
            self.active_view = ActiveView::Chat;
            // Select Gemini 4 Neon (High) and open the unified model menu with thinking submenu
            self.select_unified_model(
                "gemini-4-neon".into(),
                ProviderId::Antigravity,
                "gemini-4-neon".into(),
                ThinkingLevel::High,
            );
            crate::widgets::model_picker::force_open_picker_for_preview(
                ctx,
                "composer_unified_model_picker",
                "gemini-4-neon".into(),
            );
        } else if mode == "chat_running" {
            use umisong_core::ToolKind;

            self.active_view = ActiveView::Chat;
            self.selected_permission = PermissionMode::AskSafe;
            self.select_unified_model(
                "gemini-4-hydrogen".into(),
                ProviderId::Antigravity,
                "gemini-4-hydrogen".into(),
                ThinkingLevel::Medium,
            );

            if let Some(ws) = self.workspaces.first_mut() {
                ws.name = "test".into();
                ws.path = "/Users/ngoclin.h/Documents/test".into();
                ws.git_branch = None;
            }

            let now = chrono::Utc::now();
            self.sessions = vec![SessionSummary {
                id: "sess-active".into(),
                workspace_id: "ws-preview".into(),
                title: "make me a python simple calculator".into(),
                active_provider: ProviderId::Antigravity,
                active_model: "gemini-4-hydrogen".into(),
                permission_mode: PermissionMode::AskSafe,
                turn_count: 1,
                updated_at: now,
                pinned: false,
                unread: false,
                archived: false,
            }];

            let turn1 = TurnRecord {
                id: "turn-1".into(),
                session_id: "sess-active".into(),
                seq: 1,
                provider: ProviderId::Antigravity,
                model: "gemini-4-hydrogen".into(),
                resolved_model: Some("gemini-4-hydrogen".into()),
                prompt: "make me a python simple calculator".into(),
                thought_text: "Creating an interactive CLI calculator in calculator.py with calculate(a, op, b), format_result(value), and main() prompt loop, then verifying arithmetic operations.".into(),
                response_text: "Created a simple interactive CLI calculator in `calculator.py`.\n\n### What it includes\n- `calculate(a, op, b)` supporting `+` `-` `*` `/` `%` and `**`, with explicit error handling for unsupported operators and division or modulo by zero.\n- `format_result(value)` to show whole-number results without a trailing `.0`.\n- `main()` interactive prompt loop (enter `q`, `quit` or `exit` at any prompt to leave).\n\n### Usage\nRun it from `/Users/ngoclin.h/Documents/test`:\n\n```bash\npython3 calculator.py\n```\n\n### Verification\nRan `python3 -c \"import calculator; assert calculator.calculate(2, '+', 3) == 5\"` and both checks passed.".into(),
                status: TurnStatus::Completed,
                error_message: None,
                input_tokens: 1840,
                output_tokens: 342,
                cache_read_tokens: 0,
                cost_usd: None,
                duration_ms: Some(12000),
                handoff_from_provider: None,
                handoff_from_turn_id: None,
                tool_calls: vec![
                    ToolCallRecord {
                        call_id: "tc-1".into(),
                        name: "write_file".into(),
                        kind: ToolKind::FileEdit,
                        input_summary: "/Users/ngoclin.h/Documents/test/calculator.py".into(),
                        output_text: "Created calculator.py (68 lines)".into(),
                        status: "completed".into(),
                        success: Some(true),
                        duration_ms: Some(2000),
                    },
                    ToolCallRecord {
                        call_id: "tc-2".into(),
                        name: "run_command".into(),
                        kind: ToolKind::Shell,
                        input_summary: "python3 -c \"import calculator; assert calculator.calculate(2, '+', 3) == 5\"".into(),
                        output_text: "exit 0\nOK".into(),
                        status: "completed".into(),
                        success: Some(true),
                        duration_ms: Some(1100),
                    },
                    ToolCallRecord {
                        call_id: "tc-3".into(),
                        name: "run_command".into(),
                        kind: ToolKind::Shell,
                        input_summary: "python3 -c \"import calculator; assert calculator.calculate(10, '/', 4) == 2.5\"".into(),
                        output_text: "exit 0\nOK".into(),
                        status: "completed".into(),
                        success: Some(true),
                        duration_ms: Some(2000),
                    },
                ],
                created_at: now,
            };

            self.active_session = Some(SessionDetail {
                id: "sess-active".into(),
                workspace_id: "ws-preview".into(),
                title: "make me a python simple calculator".into(),
                active_provider: ProviderId::Antigravity,
                active_model: "gemini-4-hydrogen".into(),
                permission_mode: PermissionMode::AskSafe,
                provider_session_ids: HashMap::new(),
                created_at: now,
                updated_at: now,
                turns: vec![turn1],
            });

            self.pending_approvals.clear();
            crate::widgets::tool_card::force_open_tool_summary_for_preview(
                ctx,
                "turn-1",
                "tc-3",
                2,
            );
        } else if mode == "session_menu" {
            self.apply_preview_fixture(ctx, "chat_running");
            self.pending_approvals.clear();
            crate::views::sidebar::force_open_session_menu_for_preview(ctx, "sess-active".into());
            self.pending_delete_session = Some((
                "sess-prev".into(),
                "Audit Git worktree cleanup".into(),
            ));
        } else if mode == "versus" {
            use umisong_core::{
                CompareColumnState, CompareIsolationMode, CompareRun, DiffHunk, DiffLine,
                DiffLineKind, FileChangeKind, FileDiff, ToolKind,
            };

            self.active_view = ActiveView::Compare;
            self.compare_isolation = CompareIsolationMode::GitWorktree;
            self.compare_prompt =
                "Write a thread-safe token bucket rate limiter in calculator.py with unit tests."
                    .into();

            for pid in ProviderId::ALL {
                self.compare_enabled.insert(pid, true);
            }
            self.compare_models
                .insert(ProviderId::ClaudeCode, "claude-sonnet-5-5".into());
            self.compare_models
                .insert(ProviderId::Antigravity, "gemini-4-neon".into());
            self.compare_models
                .insert(ProviderId::Codex, "gpt-5.6-codex".into());

            let now = chrono::Utc::now();
            self.sessions = vec![SessionSummary {
                id: "sess-active".into(),
                workspace_id: "ws-preview".into(),
                title: "make me a python simple calculator".into(),
                active_provider: ProviderId::Antigravity,
                active_model: "gemini-4-hydrogen".into(),
                permission_mode: PermissionMode::AskSafe,
                turn_count: 1,
                updated_at: now,
                pinned: false,
                unread: false,
                archived: false,
            }];

            let sample_diff = FileDiff {
                path: "calculator.py".into(),
                old_path: None,
                kind: FileChangeKind::Modified,
                additions: 28,
                deletions: 2,
                is_binary: false,
                hunks: vec![DiffHunk {
                    header: "@@ -1,6 +1,32 @@".into(),
                    old_start: 1,
                    new_start: 1,
                    lines: vec![
                        DiffLine {
                            kind: DiffLineKind::Added,
                            old_line: None,
                            new_line: Some(1),
                            content: "import threading, time".into(),
                        },
                        DiffLine {
                            kind: DiffLineKind::Added,
                            old_line: None,
                            new_line: Some(2),
                            content: "class TokenBucket:".into(),
                        },
                    ],
                }],
            };

            let run1 = CompareRun {
                id: "cmp-run-1".into(),
                workspace_id: "ws-preview".into(),
                session_id: Some("sess-active".into()),
                prompt: "Write a thread-safe token bucket rate limiter in calculator.py with unit tests.".into(),
                isolation_mode: CompareIsolationMode::GitWorktree,
                columns: vec![
                    CompareColumnState {
                        provider: ProviderId::ClaudeCode,
                        model: "claude-sonnet-5-5".into(),
                        resolved_model: Some("claude-sonnet-5-5".into()),
                        status: TurnStatus::Completed,
                        thought_text: "Using monotonic clock and threading.Lock for lock-free-style refill calculation.".into(),
                        response_text: "Implemented `TokenBucket` in `calculator.py` using `time.monotonic()` and `threading.Lock`.\n\n### Highlights\n- Lazy token refill on `allow(tokens=1)`\n- Thread-safe under concurrent burst load\n\n```python\nbucket = TokenBucket(rate=10.0, capacity=20)\nassert bucket.allow(5)\n```".into(),
                        tool_calls: vec![
                            ToolCallRecord {
                                call_id: "c1".into(),
                                name: "edit_file".into(),
                                kind: ToolKind::FileEdit,
                                input_summary: "calculator.py".into(),
                                output_text: "Updated calculator.py".into(),
                                status: "completed".into(),
                                success: Some(true),
                                duration_ms: Some(420),
                            },
                            ToolCallRecord {
                                call_id: "c2".into(),
                                name: "bash".into(),
                                kind: ToolKind::Shell,
                                input_summary: "pytest -q".into(),
                                output_text: "4 passed in 0.08s".into(),
                                status: "completed".into(),
                                success: Some(true),
                                duration_ms: Some(610),
                            },
                        ],
                        error_message: None,
                        input_tokens: 1420,
                        output_tokens: 286,
                        cost_usd: None,
                        duration_ms: Some(5400),
                        worktree_path: None,
                        diff_patch: Some("diff --git a/calculator.py b/calculator.py".into()),
                        diffs: vec![sample_diff.clone()],
                        adopted: false,
                    },
                    CompareColumnState {
                        provider: ProviderId::Antigravity,
                        model: "gemini-4-neon".into(),
                        resolved_model: Some("gemini-4-neon".into()),
                        status: TurnStatus::Completed,
                        thought_text: "Designing TokenBucket with monotonic clock, wait_time helper, and deterministic tests.".into(),
                        response_text: "Added `TokenBucket` with both `allow()` and `time_until_available()` in `calculator.py`.\n\n### Verification\n- Tested concurrent threads and zero-division guards\n- All 6 unit assertions pass.\n\n```python\nlimiter = TokenBucket(rate=50, capacity=100)\nlimiter.allow(1)\n```".into(),
                        tool_calls: vec![
                            ToolCallRecord {
                                call_id: "a1".into(),
                                name: "write_file".into(),
                                kind: ToolKind::FileEdit,
                                input_summary: "calculator.py".into(),
                                output_text: "Updated calculator.py".into(),
                                status: "completed".into(),
                                success: Some(true),
                                duration_ms: Some(380),
                            },
                            ToolCallRecord {
                                call_id: "a2".into(),
                                name: "run_command".into(),
                                kind: ToolKind::Shell,
                                input_summary: "python3 -m unittest".into(),
                                output_text: "Ran 6 tests in 0.02s\nOK".into(),
                                status: "completed".into(),
                                success: Some(true),
                                duration_ms: Some(510),
                            },
                        ],
                        error_message: None,
                        input_tokens: 1390,
                        output_tokens: 312,
                        cost_usd: None,
                        duration_ms: Some(4800),
                        worktree_path: None,
                        diff_patch: Some("diff --git a/calculator.py b/calculator.py".into()),
                        diffs: vec![sample_diff.clone()],
                        adopted: true,
                    },
                    CompareColumnState {
                        provider: ProviderId::Codex,
                        model: "gpt-5.6-codex".into(),
                        resolved_model: Some("gpt-5.6-codex".into()),
                        status: TurnStatus::Completed,
                        thought_text: "Implementing compact TokenBucket class with context-manager support.".into(),
                        response_text: "Added `TokenBucket` to `calculator.py` with a `.consume()` method and unit tests.\n\n### Summary\n- Rejects negative token requests\n- Clamps bursts to `capacity`\n\n```python\nwith TokenBucket(10, 10) as tb:\n    tb.consume(2)\n```".into(),
                        tool_calls: vec![ToolCallRecord {
                            call_id: "x1".into(),
                            name: "apply_patch".into(),
                            kind: ToolKind::FileEdit,
                            input_summary: "calculator.py".into(),
                            output_text: "Applied patch".into(),
                            status: "completed".into(),
                            success: Some(true),
                            duration_ms: Some(520),
                        }],
                        error_message: None,
                        input_tokens: 1510,
                        output_tokens: 245,
                        cost_usd: None,
                        duration_ms: Some(6100),
                        worktree_path: None,
                        diff_patch: Some("diff --git a/calculator.py b/calculator.py".into()),
                        diffs: vec![sample_diff],
                        adopted: false,
                    },
                ],
                created_at: now,
            };

            let run2 = CompareRun {
                id: "cmp-run-0".into(),
                workspace_id: "ws-preview".into(),
                session_id: Some("sess-active".into()),
                prompt: "Benchmark AST expression parser vs shunting-yard in Python".into(),
                isolation_mode: CompareIsolationMode::ReadOnly,
                columns: vec![],
                created_at: now,
            };

            self.active_compare_id = Some("cmp-run-1".into());
            self.compare_runs = vec![run1, run2];
        }
    }

    pub fn unified_catalog(&self) -> Vec<UnifiedModelGroup> {
        let statuses: Vec<ProviderStatus> = ProviderId::ALL
            .into_iter()
            .filter_map(|pid| self.providers.get(&pid).cloned())
            .collect();
        build_unified_model_catalog(&statuses)
    }

    pub fn sync_selected_model_from_catalog(&mut self) {
        let catalog = self.unified_catalog();
        if let Some(existing) = find_unified_entry(&catalog, &self.selected_canonical_model) {
            if existing.available {
                self.selected_provider = existing.primary_provider;
                self.selected_model = existing.provider_model_id.clone();
                let lvl = self
                    .per_model_thinking
                    .get(&existing.canonical_id)
                    .copied()
                    .unwrap_or(existing.default_thinking_level);
                self.selected_thinking_level = existing.clamp_thinking_level(lvl);
                return;
            }
        }
        if let Some(best) = strongest_available_model(&catalog) {
            self.selected_canonical_model = best.canonical_id.clone();
            self.selected_provider = best.primary_provider;
            self.selected_model = best.provider_model_id.clone();
            let lvl = self
                .per_model_thinking
                .get(&best.canonical_id)
                .copied()
                .unwrap_or(best.default_thinking_level);
            self.selected_thinking_level = best.clamp_thinking_level(lvl);
        }
    }

    pub fn select_unified_model(
        &mut self,
        canonical_id: String,
        provider: ProviderId,
        provider_model_id: String,
        thinking_level: ThinkingLevel,
    ) {
        self.per_model_thinking
            .insert(canonical_id.clone(), thinking_level);
        self.selected_canonical_model = canonical_id.clone();
        self.selected_provider = provider;
        self.selected_model = provider_model_id;
        self.selected_thinking_level = thinking_level;

        // Persist user's model + thinking choice in AppSettings across restarts & sessions
        self.settings.last_canonical_model = Some(canonical_id);
        self.settings.last_thinking_level = Some(thinking_level);
        self.settings.per_model_thinking = self.per_model_thinking.clone();
        self.save_app_settings();
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        let id = self.active_workspace_id.as_deref()?;
        self.workspaces.iter().find(|w| w.id == id)
    }

    pub fn active_compare_run(&self) -> Option<&CompareRun> {
        let id = self.active_compare_id.as_deref()?;
        self.compare_runs.iter().find(|r| r.id == id)
    }

    pub fn active_running_turn_id(&self) -> Option<String> {
        self.active_session.as_ref().and_then(|s| {
            s.turns
                .iter()
                .rev()
                .find(|t| t.status == TurnStatus::Running)
                .map(|t| t.id.clone())
        })
    }

    fn bootstrap(&mut self) {
        self.client
            .send(RequestTag::Bootstrap, DaemonRequest::GetSettings);
        self.client
            .send(RequestTag::RefreshProviders, DaemonRequest::ListProviders);
        self.client
            .send(RequestTag::Bootstrap, DaemonRequest::ListWorkspaces);
    }

    pub fn refresh_all_providers(&mut self) {
        self.client
            .send(RequestTag::RefreshProviders, DaemonRequest::ListProviders);
    }

    pub fn install_provider_cli(&mut self, provider: ProviderId) {
        self.provider_install_states.insert(
            provider,
            ProviderInstallState::Installing {
                method: "starting".into(),
                detail: format!("Installing {} CLI…", provider.display_name()),
            },
        );
        self.client.send(
            RequestTag::InstallProviders,
            DaemonRequest::InstallProvider { provider },
        );
    }

    pub fn install_missing_providers(&mut self) {
        self.client.send(
            RequestTag::InstallProviders,
            DaemonRequest::InstallMissingProviders,
        );
    }

    fn maybe_trigger_startup_auto_install(&mut self) {
        if self.startup_auto_install_checked || !self.settings_loaded || !self.providers_loaded {
            return;
        }
        self.startup_auto_install_checked = true;
        if !self.settings.auto_install_clis {
            return;
        }
        let any_missing = self
            .providers
            .values()
            .any(|st| !st.installed && st.settings.binary_override.is_none());
        if any_missing {
            self.install_missing_providers();
        }
    }

    pub fn save_app_settings(&mut self) {
        self.client.send(
            RequestTag::SaveSettings,
            DaemonRequest::UpdateSettings {
                settings: self.settings.clone(),
            },
        );
    }

    pub fn select_workspace(&mut self, workspace_id: String) {
        self.active_workspace_id = Some(workspace_id.clone());
        self.active_session = None;
        self.sessions.clear();
        self.compare_runs.clear();
        self.active_compare_id = None;
        self.client.send(
            RequestTag::LoadSessions,
            DaemonRequest::ListSessions {
                workspace_id: workspace_id.clone(),
            },
        );
        self.client.send(
            RequestTag::LoadCompares,
            DaemonRequest::ListCompareRuns { workspace_id },
        );
        self.refresh_git_diff();
    }

    pub fn create_new_session(&mut self) {
        self.active_view = ActiveView::Chat;
        // Lazy draft state: switch to a clean blank chat without creating empty
        // "New Session" rows in SQLite until the user actually sends a message.
        self.active_session = None;
        self.pending_approvals.clear();
        self.chat_input.clear();
    }

    pub fn send_chat_prompt(&mut self) {
        let prompt = self.chat_input.trim().to_string();
        if prompt.is_empty() {
            return;
        }
        self.chat_input.clear();

        if let Some(sess) = &self.active_session {
            self.client.send(
                RequestTag::SendTurn,
                DaemonRequest::SendTurn {
                    session_id: sess.id.clone(),
                    provider: self.selected_provider,
                    model: self.selected_model.clone(),
                    thinking_level: self.selected_thinking_level,
                    permission_mode: self.selected_permission,
                    prompt,
                },
            );
        } else if let Some(ws_id) = self.active_workspace_id.clone() {
            self.pending_first_prompt = Some(prompt);
            self.client.send(
                RequestTag::CreateSession,
                DaemonRequest::CreateSession {
                    workspace_id: ws_id,
                    title: "New Session".into(),
                    provider: self.selected_provider,
                    model: self.selected_model.clone(),
                    permission_mode: self.selected_permission,
                },
            );
        }
    }

    pub fn open_handoff_modal(&mut self, up_to_turn_id: Option<String>) {
        let catalog = self.unified_catalog();
        // Prefer an available model from a DIFFERENT provider than the current one
        let candidate = catalog
            .iter()
            .flat_map(|g| g.models.iter())
            .find(|m| m.available && m.primary_provider != self.selected_provider)
            .or_else(|| strongest_available_model(&catalog));

        let (canonical_id, target_provider, target_model, target_thinking_level) =
            if let Some(m) = candidate {
                let lvl = self
                    .per_model_thinking
                    .get(&m.canonical_id)
                    .copied()
                    .map(|l| m.clamp_thinking_level(l))
                    .unwrap_or(m.default_thinking_level);
                (
                    m.canonical_id.clone(),
                    m.primary_provider,
                    m.provider_model_id.clone(),
                    lvl,
                )
            } else {
                let next_provider = match self.selected_provider {
                    ProviderId::ClaudeCode => ProviderId::Antigravity,
                    ProviderId::Antigravity => ProviderId::Codex,
                    ProviderId::Codex => ProviderId::ClaudeCode,
                };
                (String::new(), next_provider, String::new(), ThinkingLevel::High)
            };

        self.handoff_modal = Some(HandoffModalState {
            up_to_turn_id,
            target_canonical_id: canonical_id,
            target_provider,
            target_model,
            target_thinking_level,
            instruction: String::new(),
            create_new_session: false,
        });
    }

    pub fn load_compare_runs(&mut self) {
        if let Some(ws_id) = self.active_workspace_id.clone() {
            self.client.send(
                RequestTag::LoadCompares,
                DaemonRequest::ListCompareRuns { workspace_id: ws_id },
            );
        }
    }

    pub fn refresh_git_diff(&mut self) {
        if let Some(ws) = self.active_workspace() {
            self.client.send(
                RequestTag::GetDiff,
                DaemonRequest::GetGitDiff {
                    workspace_path: ws.path.clone(),
                    scope: self.diff_scope,
                },
            );
        }
    }

    pub fn start_auth_login(&mut self, provider: ProviderId, mode: AuthLoginMode) {
        self.client.send(
            RequestTag::StartAuth,
            DaemonRequest::StartAuthTerminal {
                provider,
                mode,
                cols: 100,
                rows: 24,
            },
        );
    }

    fn process_inbound(&mut self) {
        for msg in self.client.drain() {
            match msg {
                InboundMessage::Connected { embedded } => {
                    self.daemon_connected = true;
                    self.daemon_embedded = embedded;
                    self.bootstrap();
                }
                InboundMessage::ConnectionError(err) => {
                    self.daemon_connected = false;
                    self.toast = Some((err, true));
                }
                InboundMessage::Response { tag, result } => {
                    self.handle_response(tag, result);
                }
                InboundMessage::Notification(notif) => {
                    self.handle_notification(notif);
                }
            }
        }
    }

    fn handle_response(&mut self, tag: RequestTag, result: Result<DaemonResponse, String>) {
        let resp = match result {
            Ok(r) => r,
            Err(err) => {
                self.toast = Some((err, true));
                return;
            }
        };

        match resp {
            DaemonResponse::Settings(s) => {
                for (k, v) in &s.per_model_thinking {
                    self.per_model_thinking.entry(k.clone()).or_insert(*v);
                }
                if let Some(last_model) = &s.last_canonical_model {
                    if !last_model.is_empty() {
                        self.selected_canonical_model = last_model.clone();
                        if let Some(lvl) = s.last_thinking_level {
                            self.selected_thinking_level = lvl;
                            self.per_model_thinking.insert(last_model.clone(), lvl);
                        }
                    }
                }
                self.settings = s;
                self.settings_loaded = true;
                self.sync_selected_model_from_catalog();
                self.maybe_trigger_startup_auto_install();
            }
            DaemonResponse::Providers(list) => {
                for st in list {
                    self.sync_provider_draft(&st);
                    self.providers.insert(st.provider, st);
                }
                self.providers_loaded = true;
                self.sync_selected_model_from_catalog();
                self.maybe_trigger_startup_auto_install();
            }
            DaemonResponse::Provider(st) => {
                self.sync_provider_draft(&st);
                let pid = st.provider;
                self.providers.insert(pid, st);
                self.sync_selected_model_from_catalog();
                if matches!(tag, RequestTag::SaveProviderConfig) {
                    self.toast = Some((format!("Saved {} settings.", pid.display_name()), false));
                }
            }
            DaemonResponse::InstallStarted { providers } => {
                for pid in providers {
                    self.provider_install_states.entry(pid).or_insert_with(|| {
                        ProviderInstallState::Installing {
                            method: "auto".into(),
                            detail: format!("Installing {} CLI…", pid.display_name()),
                        }
                    });
                }
            }
            DaemonResponse::AuthTerminalStarted {
                auth_session_id,
                command,
            } => {
                let provider = if command.contains("codex") {
                    ProviderId::Codex
                } else if command.contains("agy") {
                    ProviderId::Antigravity
                } else {
                    ProviderId::ClaudeCode
                };
                self.auth_terminal = Some(AuthTerminalUi {
                    auth_session_id,
                    provider,
                    command,
                    screen_lines: Vec::new(),
                    detected_urls: Vec::new(),
                    exited: false,
                    exit_code: None,
                });
            }
            DaemonResponse::Workspaces(list) => {
                self.workspaces = list;
                if self.active_workspace_id.is_none() {
                    if let Some(first) = self.workspaces.first().cloned() {
                        self.select_workspace(first.id);
                    } else if let Ok(cwd) = std::env::current_dir() {
                        self.client.send(
                            RequestTag::OpenWorkspace,
                            DaemonRequest::OpenWorkspace {
                                path: cwd.to_string_lossy().into_owned(),
                            },
                        );
                    }
                }
            }
            DaemonResponse::Workspace(ws) => {
                let ws_id = ws.id.clone();
                if let Some(existing) = self.workspaces.iter_mut().find(|w| w.id == ws.id) {
                    *existing = ws;
                } else {
                    self.workspaces.insert(0, ws);
                }
                self.select_workspace(ws_id);
            }
            DaemonResponse::Sessions(list) => {
                self.sessions = list;
                if self.active_session.is_none() {
                    if let Some(first) = self.sessions.first() {
                        self.client.send(
                            RequestTag::GetSession,
                            DaemonRequest::GetSession {
                                session_id: first.id.clone(),
                            },
                        );
                    }
                }
            }
            DaemonResponse::Session(mut sess) => {
                // Preserve in-flight streaming buffers if SQLite returned an older `Running`
                // snapshot while tokens are already streaming in memory.
                if let Some(prev) = &self.active_session {
                    if prev.id == sess.id {
                        for incoming in &mut sess.turns {
                            if let Some(existing) = prev.turns.iter().find(|t| t.id == incoming.id)
                            {
                                if incoming.status == TurnStatus::Running {
                                    if existing.response_text.len() > incoming.response_text.len() {
                                        incoming.response_text = existing.response_text.clone();
                                    }
                                    if existing.thought_text.len() > incoming.thought_text.len() {
                                        incoming.thought_text = existing.thought_text.clone();
                                    }
                                    if existing.tool_calls.len() > incoming.tool_calls.len() {
                                        incoming.tool_calls = existing.tool_calls.clone();
                                    }
                                    if incoming.resolved_model.is_none() {
                                        incoming.resolved_model = existing.resolved_model.clone();
                                    }
                                    if existing.status != TurnStatus::Running {
                                        incoming.status = existing.status;
                                    }
                                }
                            }
                        }
                    }
                }

                // Keep the user's chosen model if one is already selected; only fall back to the
                // session's stored model when no model has been selected yet.
                if self.selected_canonical_model.is_empty() {
                    self.selected_provider = sess.active_provider;
                    self.selected_model = sess.active_model.clone();
                } else {
                    self.sync_selected_model_from_catalog();
                }
                self.selected_permission = sess.permission_mode;
                let sid = sess.id.clone();
                let ws_id = sess.workspace_id.clone();
                self.active_session = Some(sess);

                if matches!(tag, RequestTag::CreateSession) {
                    self.client.send(
                        RequestTag::LoadSessions,
                        DaemonRequest::ListSessions {
                            workspace_id: ws_id,
                        },
                    );
                    if let Some(prompt) = self.pending_first_prompt.take() {
                        self.client.send(
                            RequestTag::SendTurn,
                            DaemonRequest::SendTurn {
                                session_id: sid,
                                provider: self.selected_provider,
                                model: self.selected_model.clone(),
                                thinking_level: self.selected_thinking_level,
                                permission_mode: self.selected_permission,
                                prompt,
                            },
                        );
                    }
                }
            }
            DaemonResponse::TurnStarted { session_id, .. }
            | DaemonResponse::HandoffStarted { session_id, .. } => {
                self.active_view = ActiveView::Chat;
                self.client.send(
                    RequestTag::GetSession,
                    DaemonRequest::GetSession { session_id },
                );
                if let Some(ws_id) = self.active_workspace_id.clone() {
                    self.client.send(
                        RequestTag::LoadSessions,
                        DaemonRequest::ListSessions { workspace_id: ws_id },
                    );
                }
            }
            DaemonResponse::CompareRuns(runs) => {
                self.compare_runs = runs;
                if self.active_compare_id.is_none() {
                    self.active_compare_id = self.compare_runs.first().map(|r| r.id.clone());
                }
            }
            DaemonResponse::CompareStarted(run) => {
                self.active_compare_id = Some(run.id.clone());
                self.compare_runs.insert(0, run);
            }
            DaemonResponse::CompareAdopted {
                session_id,
                message,
                ..
            } => {
                self.toast = Some((message, false));
                self.active_view = ActiveView::Chat;
                self.client.send(
                    RequestTag::GetSession,
                    DaemonRequest::GetSession { session_id },
                );
                if let Some(ws_id) = self.active_workspace_id.clone() {
                    self.client.send(
                        RequestTag::LoadSessions,
                        DaemonRequest::ListSessions { workspace_id: ws_id },
                    );
                }
                self.load_compare_runs();
                self.refresh_git_diff();
            }
            DaemonResponse::GitDiff(snap) => {
                self.git_diff = Some(snap);
            }
            DaemonResponse::Ack => {
                if matches!(tag, RequestTag::DeleteSession | RequestTag::UpdateSessionMeta) {
                    if let Some(ws_id) = self.active_workspace_id.clone() {
                        self.client.send(
                            RequestTag::LoadSessions,
                            DaemonRequest::ListSessions { workspace_id: ws_id },
                        );
                    }
                } else if matches!(tag, RequestTag::RevertFile) {
                    self.toast = Some(("Reverted file changes.".into(), false));
                    self.refresh_git_diff();
                }
            }
            DaemonResponse::Pong { .. } => {}
        }
    }

    fn sync_provider_draft(&mut self, st: &ProviderStatus) {
        let entry = self.provider_drafts.entry(st.provider).or_default();
        if entry.binary_override.is_empty() {
            entry.binary_override = st.settings.binary_override.clone().unwrap_or_default();
        }
        entry.default_model = st.settings.default_model.clone();
        entry.custom_models = st.settings.custom_models.clone();
        entry.plan_tier_override = st
            .settings
            .plan_tier_override
            .unwrap_or(PlanTier::AutoDetect);
        if entry.api_key_override.is_empty() {
            entry.api_key_override = st.settings.api_key_override.clone().unwrap_or_default();
        }
    }

    fn handle_notification(&mut self, notif: DaemonNotification) {
        match notif {
            DaemonNotification::ProviderStatusUpdated(st) => {
                self.sync_provider_draft(&st);
                self.providers.insert(st.provider, st);
                self.sync_selected_model_from_catalog();
            }
            DaemonNotification::ProviderInstallUpdated { provider, state } => {
                if matches!(state, ProviderInstallState::Installed) {
                    self.toast = Some((
                        format!("Installed {} CLI — ready to sign in.", provider.display_name()),
                        false,
                    ));
                }
                self.provider_install_states.insert(provider, state);
            }
            DaemonNotification::TurnEvent {
                session_id,
                turn_id,
                provider,
                event,
            } => {
                if let AgentEvent::ApprovalRequested {
                    approval_id,
                    tool_name,
                    summary,
                    detail,
                } = &event
                {
                    self.pending_approvals.push(PendingApprovalUi {
                        approval_id: approval_id.clone(),
                        tool_name: tool_name.clone(),
                        summary: summary.clone(),
                        detail: detail.clone(),
                    });
                }

                let mut reload_session = false;
                if let Some(sess) = self.active_session.as_mut() {
                    if sess.id == session_id {
                        if let Some(turn) = sess.turns.iter_mut().find(|t| t.id == turn_id) {
                            apply_event_to_ui_turn(turn, &event);
                            if matches!(event, AgentEvent::TurnCompleted { .. }) {
                                reload_session = true;
                            }
                        } else {
                            // Insert live placeholder immediately instead of spamming GetSession per token!
                            let seq = sess.turns.last().map(|t| t.seq + 1).unwrap_or(1);
                            let mut live_turn = TurnRecord {
                                id: turn_id.clone(),
                                session_id: session_id.clone(),
                                seq,
                                provider,
                                model: self.selected_model.clone(),
                                resolved_model: None,
                                prompt: String::new(),
                                thought_text: String::new(),
                                response_text: String::new(),
                                status: TurnStatus::Running,
                                error_message: None,
                                input_tokens: 0,
                                output_tokens: 0,
                                cache_read_tokens: 0,
                                cost_usd: None,
                                duration_ms: None,
                                handoff_from_provider: None,
                                handoff_from_turn_id: None,
                                tool_calls: Vec::new(),
                                created_at: chrono::Utc::now(),
                            };
                            apply_event_to_ui_turn(&mut live_turn, &event);
                            sess.turns.push(live_turn);
                            reload_session = true;
                        }
                    }
                }

                if reload_session {
                    self.client.send(
                        RequestTag::GetSession,
                        DaemonRequest::GetSession { session_id },
                    );
                }
            }
            DaemonNotification::CompareEvent {
                compare_id,
                provider,
                event,
            } => {
                if let Some(run) = self.compare_runs.iter_mut().find(|r| r.id == compare_id) {
                    if let Some(col) = run.columns.iter_mut().find(|c| c.provider == provider) {
                        match event {
                            AgentEvent::SessionInit { resolved_model, .. } => {
                                if resolved_model.is_some() {
                                    col.resolved_model = resolved_model;
                                }
                            }
                            AgentEvent::ThoughtDelta { text } => col.thought_text.push_str(&text),
                            AgentEvent::TextDelta { text } => col.response_text.push_str(&text),
                            AgentEvent::ToolCallStarted {
                                call_id,
                                name,
                                kind,
                                input_summary,
                                ..
                            } => {
                                col.tool_calls.push(ToolCallRecord {
                                    call_id,
                                    name,
                                    kind,
                                    input_summary,
                                    output_text: String::new(),
                                    status: "running".into(),
                                    success: None,
                                    duration_ms: None,
                                });
                            }
                            AgentEvent::ToolCallCompleted {
                                call_id,
                                success,
                                output,
                                duration_ms,
                            } => {
                                if let Some(tc) =
                                    col.tool_calls.iter_mut().find(|c| c.call_id == call_id)
                                {
                                    tc.success = Some(success);
                                    tc.status = if success { "completed" } else { "failed" }.into();
                                    tc.output_text = output;
                                    tc.duration_ms = duration_ms;
                                }
                            }
                            AgentEvent::UsageUpdate {
                                input_tokens,
                                output_tokens,
                                cost_usd,
                                ..
                            } => {
                                col.input_tokens = input_tokens.max(col.input_tokens);
                                col.output_tokens = output_tokens.max(col.output_tokens);
                                if cost_usd.is_some() {
                                    col.cost_usd = cost_usd;
                                }
                            }
                            AgentEvent::Error { message, .. } => {
                                col.error_message = Some(message);
                            }
                            AgentEvent::TurnCompleted {
                                final_text,
                                exit_reason,
                            } => {
                                if col.response_text.is_empty() {
                                    if let Some(ft) = final_text {
                                        col.response_text = ft;
                                    }
                                }
                                col.status = match exit_reason {
                                    TurnExitReason::Completed => TurnStatus::Completed,
                                    TurnExitReason::Cancelled => TurnStatus::Cancelled,
                                    TurnExitReason::Failed => TurnStatus::Failed,
                                };
                            }
                            _ => {}
                        }
                    }
                }
            }
            DaemonNotification::CompareColumnFinished {
                compare_id,
                provider,
                diff_patch,
                diffs,
            } => {
                if let Some(run) = self.compare_runs.iter_mut().find(|r| r.id == compare_id) {
                    if let Some(col) = run.columns.iter_mut().find(|c| c.provider == provider) {
                        col.diff_patch = diff_patch;
                        col.diffs = diffs;
                        if col.status == TurnStatus::Running {
                            col.status = TurnStatus::Completed;
                        }
                    }
                }
            }
            DaemonNotification::AuthTerminalOutput {
                auth_session_id,
                provider,
                screen_lines,
                detected_urls,
                exited,
                exit_code,
            } => {
                if let Some(auth) = self.auth_terminal.as_mut() {
                    if auth.auth_session_id == auth_session_id {
                        auth.provider = provider;
                        auth.screen_lines = screen_lines;
                        auth.detected_urls = detected_urls;
                        auth.exited = exited;
                        auth.exit_code = exit_code;
                    }
                }
            }
            DaemonNotification::WorkspaceDiffChanged { workspace_path } => {
                if self
                    .active_workspace()
                    .is_some_and(|w| w.path == workspace_path)
                    && self.active_view == ActiveView::Diffs
                {
                    self.refresh_git_diff();
                }
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        ctx.input(|i| {
            if i.modifiers.command && i.key_pressed(egui::Key::N) {
                self.create_new_session();
            } else if i.modifiers.command && i.key_pressed(egui::Key::Num1) {
                self.active_view = ActiveView::Chat;
            } else if i.modifiers.command && i.key_pressed(egui::Key::Num2) {
                self.active_view = ActiveView::Compare;
                self.load_compare_runs();
            } else if i.modifiers.command && i.key_pressed(egui::Key::Num3) {
                self.active_view = ActiveView::Diffs;
                self.refresh_git_diff();
            } else if i.modifiers.command && i.key_pressed(egui::Key::Num4) {
                self.active_view = ActiveView::Accounts;
                self.refresh_all_providers();
            }
        });
    }
}

fn apply_event_to_ui_turn(turn: &mut TurnRecord, ev: &AgentEvent) {
    match ev {
        AgentEvent::SessionInit { resolved_model, .. } => {
            if resolved_model.is_some() {
                turn.resolved_model = resolved_model.clone();
            }
        }
        AgentEvent::ThoughtDelta { text } => turn.thought_text.push_str(text),
        AgentEvent::TextDelta { text } => turn.response_text.push_str(text),
        AgentEvent::ToolCallStarted {
            call_id,
            name,
            kind,
            input_summary,
            ..
        } => {
            if let Some(existing) = turn.tool_calls.iter_mut().find(|c| c.call_id == *call_id) {
                existing.input_summary = input_summary.clone();
            } else {
                turn.tool_calls.push(ToolCallRecord {
                    call_id: call_id.clone(),
                    name: name.clone(),
                    kind: *kind,
                    input_summary: input_summary.clone(),
                    output_text: String::new(),
                    status: "running".into(),
                    success: None,
                    duration_ms: None,
                });
            }
        }
        AgentEvent::ToolCallUpdated {
            call_id,
            status,
            output_chunk,
        } => {
            if let Some(existing) = turn.tool_calls.iter_mut().find(|c| c.call_id == *call_id) {
                existing.status = status.clone();
                if let Some(chunk) = output_chunk {
                    existing.output_text.push_str(chunk);
                }
            }
        }
        AgentEvent::ToolCallCompleted {
            call_id,
            success,
            output,
            duration_ms,
        } => {
            if let Some(existing) = turn.tool_calls.iter_mut().find(|c| c.call_id == *call_id) {
                existing.success = Some(*success);
                existing.status = if *success { "completed" } else { "failed" }.into();
                if !output.is_empty() {
                    existing.output_text = output.clone();
                }
                existing.duration_ms = *duration_ms;
            }
        }
        AgentEvent::UsageUpdate {
            input_tokens,
            output_tokens,
            cache_read_tokens,
            cost_usd,
        } => {
            turn.input_tokens = (*input_tokens).max(turn.input_tokens);
            turn.output_tokens = (*output_tokens).max(turn.output_tokens);
            turn.cache_read_tokens = (*cache_read_tokens).max(turn.cache_read_tokens);
            if cost_usd.is_some() {
                turn.cost_usd = *cost_usd;
            }
        }
        AgentEvent::Error { message, .. } => {
            turn.error_message = Some(message.clone());
        }
        AgentEvent::TurnCompleted {
            final_text,
            exit_reason,
        } => {
            if turn.response_text.is_empty() {
                if let Some(ft) = final_text {
                    turn.response_text = ft.clone();
                }
            }
            turn.status = match exit_reason {
                TurnExitReason::Completed => TurnStatus::Completed,
                TurnExitReason::Cancelled => TurnStatus::Cancelled,
                TurnExitReason::Failed => TurnStatus::Failed,
            };
        }
        AgentEvent::ApprovalRequested { .. } => {}
    }
}

impl UmisongApp {
    pub fn render_frame(&mut self, ctx: &egui::Context) {
        self.process_inbound();

        if self.last_applied_dark != Some(self.settings.dark_theme) {
            apply_theme(ctx, self.settings.dark_theme);
            self.last_applied_dark = Some(self.settings.dark_theme);
        }

        self.handle_shortcuts(ctx);

        let palette = Palette::for_mode(self.settings.dark_theme);

        // Left Sidebar
        sidebar::render_sidebar(self, ctx, &palette);

        // Minimal Bottom Status Bar (matches mockup: "● Daemon running (umisongd)")
        egui::TopBottomPanel::bottom("umisong_status_bar")
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_canvas)
                    .stroke(Stroke::new(1.0_f32, palette.border))
                    .inner_margin(Margin::symmetric(14, 6)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let conn_col = if self.daemon_connected {
                        palette.success
                    } else {
                        palette.danger
                    };
                    let conn_label = if self.daemon_connected {
                        if self.daemon_embedded {
                            "Daemon running (umisongd)"
                        } else {
                            "Daemon running (umisongd · external)"
                        }
                    } else {
                        "Connecting to daemon (umisongd)…"
                    };
                    paint_status_dot(ui, conn_col, true);
                    ui.label(RichText::new(conn_label).size(11.5).color(palette.text_muted));

                    let installing_names: Vec<&'static str> = ProviderId::ALL
                        .iter()
                        .copied()
                        .filter(|pid| {
                            matches!(
                                self.provider_install_states.get(pid),
                                Some(ProviderInstallState::Installing { .. })
                            )
                        })
                        .map(|pid| pid.display_name())
                        .collect();
                    if !installing_names.is_empty() {
                        ui.separator();
                        ui.add(egui::Spinner::new().size(11.0));
                        let label = format!("Installing CLIs: {}…", installing_names.join(", "));
                        if ui
                            .add(
                                egui::Button::new(
                                    RichText::new(label).size(11.5).color(palette.accent),
                                )
                                .frame(false),
                            )
                            .clicked()
                        {
                            self.active_view = ActiveView::Accounts;
                        }
                    }

                    if let Some((msg, is_err)) = self.toast.clone() {
                        ui.separator();
                        let col = if is_err {
                            palette.danger
                        } else {
                            palette.success
                        };
                        ui.label(RichText::new(msg).size(11.5).color(col));
                        if ui.small_button("×").clicked() {
                            self.toast = None;
                        }
                    }
                });
            });

        // Main Central Panel
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(palette.bg_canvas)
                    .inner_margin(Margin::same(14)),
            )
            .show(ctx, |ui| match self.active_view {
                ActiveView::Chat => chat::render_chat_view(self, ui, &palette),
                ActiveView::Compare => compare::render_compare_view(self, ui, &palette),
                ActiveView::Diffs => diff_viewer::render_diff_viewer(self, ui, &palette),
                ActiveView::Accounts => accounts::render_accounts_view(self, ui, &palette),
            });

        // Handoff Modal
        if let Some(mut modal) = self.handoff_modal.take() {
            let keep = chat::render_handoff_modal(self, ctx, &mut modal, &palette);
            if keep {
                self.handoff_modal = Some(modal);
            }
        }

        // Delete Session Confirmation Modal
        if let Some((sid, title)) = self.pending_delete_session.clone() {
            let mut close = ctx.input(|i| i.key_pressed(egui::Key::Escape));
            let mut confirm_delete = false;

            // Subtle modal backdrop scrim
            let screen = ctx.screen_rect();
            egui::Area::new(egui::Id::new("delete_session_backdrop"))
                .order(egui::Order::Middle)
                .fixed_pos(screen.left_top())
                .show(ctx, |ui| {
                    let (_, scrim_resp) =
                        ui.allocate_exact_size(screen.size(), egui::Sense::click());
                    ui.painter().rect_filled(
                        screen,
                        egui::CornerRadius::ZERO,
                        egui::Color32::from_rgba_unmultiplied(0, 0, 0, 125),
                    );
                    if scrim_resp.clicked() {
                        close = true;
                    }
                });

            egui::Area::new(egui::Id::new("delete_session_confirm_modal"))
                .order(egui::Order::Foreground)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(palette.bg_card)
                        .stroke(Stroke::new(1.0_f32, palette.border_strong))
                        .corner_radius(egui::CornerRadius::same(14))
                        .inner_margin(Margin::symmetric(20, 18))
                        .shadow(egui::epaint::Shadow {
                            offset: [0, 14],
                            blur: 32,
                            spread: 0,
                            color: egui::Color32::from_rgba_unmultiplied(0, 0, 0, 175),
                        })
                        .show(ui, |ui| {
                            ui.set_width(360.0);
                            ui.label(
                                RichText::new("Delete session?")
                                    .size(15.5)
                                    .strong()
                                    .color(palette.text_primary),
                            );
                            ui.add_space(6.0);
                            ui.add(
                                egui::Label::new(
                                    RichText::new(format!(
                                        "Do you want to delete \"{title}\"? All turns in this session will be permanently removed."
                                    ))
                                    .size(12.5)
                                    .color(palette.text_secondary),
                                )
                                .wrap(),
                            );
                            ui.add_space(16.0);
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.spacing_mut().item_spacing.x = 8.0;

                                    // Destructive Delete button (crimson, keeps pink strictly for primary CTA)
                                    let (del_rect, del_resp) = ui.allocate_exact_size(
                                        egui::vec2(78.0, 32.0),
                                        egui::Sense::click(),
                                    );
                                    let del_hov = del_resp.hovered();
                                    let del_press = del_resp.is_pointer_button_down_on();
                                    if del_hov {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    let dh = crate::theme::hover_factor(
                                        ui.ctx(),
                                        ui.make_persistent_id("modal_del_sess_h"),
                                        del_hov,
                                    );
                                    let del_fill = if del_press {
                                        crate::theme::lerp_color(palette.bg_code, palette.danger, 0.14)
                                    } else {
                                        crate::theme::lerp_color(
                                            crate::theme::lerp_color(palette.bg_code, palette.danger, 0.18),
                                            crate::theme::lerp_color(palette.bg_code, palette.danger, 0.30),
                                            dh,
                                        )
                                    };
                                    let dr = if del_press { del_rect.shrink(1.0) } else { del_rect };
                                    ui.painter().rect(
                                        dr,
                                        egui::CornerRadius::same(8),
                                        del_fill,
                                        Stroke::new(1.0_f32, palette.danger),
                                        egui::StrokeKind::Inside,
                                    );
                                    let dg = ui.painter().layout_no_wrap(
                                        "Delete".to_string(),
                                        egui::FontId::proportional(12.5),
                                        palette.danger,
                                    );
                                    ui.painter().galley(
                                        dr.center() - dg.size() * 0.5,
                                        dg,
                                        palette.danger,
                                    );
                                    if del_resp.clicked() {
                                        confirm_delete = true;
                                        close = true;
                                    }

                                    // Secondary Cancel button
                                    let (can_rect, can_resp) = ui.allocate_exact_size(
                                        egui::vec2(76.0, 32.0),
                                        egui::Sense::click(),
                                    );
                                    let can_hov = can_resp.hovered();
                                    if can_hov {
                                        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                    }
                                    let ch = crate::theme::hover_factor(
                                        ui.ctx(),
                                        ui.make_persistent_id("modal_cancel_sess_h"),
                                        can_hov,
                                    );
                                    ui.painter().rect(
                                        can_rect,
                                        egui::CornerRadius::same(8),
                                        crate::theme::lerp_color(palette.bg_code, palette.bg_elevated, ch),
                                        Stroke::new(
                                            1.0_f32,
                                            crate::theme::lerp_color(palette.border, palette.border_strong, ch),
                                        ),
                                        egui::StrokeKind::Inside,
                                    );
                                    let cg = ui.painter().layout_no_wrap(
                                        "Cancel".to_string(),
                                        egui::FontId::proportional(12.5),
                                        palette.text_primary,
                                    );
                                    ui.painter().galley(
                                        can_rect.center() - cg.size() * 0.5,
                                        cg,
                                        palette.text_primary,
                                    );
                                    if can_resp.clicked() {
                                        close = true;
                                    }
                                },
                            );
                        });
                });

            if confirm_delete {
                if self.active_session.as_ref().is_some_and(|a| a.id == sid) {
                    self.active_session = None;
                }
                self.sessions.retain(|s| s.id != sid);
                self.client.send(
                    RequestTag::DeleteSession,
                    DaemonRequest::DeleteSession { session_id: sid },
                );
            }
            if close {
                self.pending_delete_session = None;
            }
        }

        // Revert File Confirmation Modal
        if let Some(file_to_revert) = self.pending_revert_file.clone() {
            let mut close = false;
            egui::Window::new("Confirm Revert File")
                .collapsible(false)
                .resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label(
                        RichText::new(format!("Discard uncommitted changes to `{file_to_revert}`?"))
                            .size(13.0),
                    );
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        if ui
                            .button(RichText::new("Revert").color(palette.danger).strong())
                            .clicked()
                        {
                            if let Some(ws) = self.active_workspace() {
                                self.client.send(
                                    RequestTag::RevertFile,
                                    DaemonRequest::RevertFile {
                                        workspace_path: ws.path.clone(),
                                        file_path: file_to_revert.clone(),
                                    },
                                );
                            }
                            close = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            if close {
                self.pending_revert_file = None;
            }
        }
    }
}

impl eframe::App for UmisongApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.render_frame(ctx);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_app(ctx: &egui::Context) -> UmisongApp {
        install_system_fonts(ctx);
        let logo_textures = LogoTextures::load(ctx);
        apply_theme(ctx, true);

        let mut compare_enabled = HashMap::new();
        for pid in ProviderId::ALL {
            compare_enabled.insert(pid, true);
        }

        UmisongApp {
            client: DaemonClient::disconnected_offline(),
            logo_textures,
            daemon_connected: true,
            daemon_embedded: true,
            settings: AppSettings::default(),
            last_applied_dark: Some(true),
            active_view: ActiveView::Chat,
            providers: HashMap::new(),
            provider_install_states: HashMap::new(),
            provider_drafts: HashMap::new(),
            accounts_advanced_open: HashMap::new(),
            workspaces: Vec::new(),
            active_workspace_id: None,
            sessions: Vec::new(),
            active_session: None,
            session_search: String::new(),
            renaming_session: None,
            pending_delete_session: None,
            show_archived_sessions: false,
            selected_canonical_model: String::new(),
            selected_provider: ProviderId::ClaudeCode,
            selected_model: String::new(),
            selected_thinking_level: ThinkingLevel::High,
            per_model_thinking: HashMap::new(),
            selected_permission: PermissionMode::AskSafe,
            chat_input: String::new(),
            pending_approvals: Vec::new(),
            compare_runs: Vec::new(),
            active_compare_id: None,
            compare_prompt: String::new(),
            compare_enabled,
            compare_models: HashMap::new(),
            compare_thinking_levels: HashMap::new(),
            compare_isolation: CompareIsolationMode::ReadOnly,
            compare_col_tabs: HashMap::new(),
            diff_scope: DiffScope::All,
            git_diff: None,
            selected_diff_file: None,
            diff_file_filter: String::new(),
            pending_revert_file: None,
            auth_terminal: None,
            auth_input_line: String::new(),
            handoff_modal: None,
            toast: None,
            pending_first_prompt: None,
            settings_loaded: false,
            providers_loaded: false,
            startup_auto_install_checked: false,
        }
    }

    #[test]
    fn headless_renders_all_views_without_panic_or_deadlock() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);

        // Populate a signed-in provider + session so model picker, hover cards,
        // session rows, and all views are exercised across consecutive frames.
        let agy = ProviderStatus {
            provider: ProviderId::Antigravity,
            installed: true,
            binary_path: Some("/usr/local/bin/agy".into()),
            version: Some("v1.3.5".into()),
            auth: umisong_core::AuthState::Authenticated {
                method: "OAuth".into(),
                account_hint: Some("test@example.com".into()),
                plan_hint: Some("Google AI Pro".into()),
            },
            effective_plan_tier: PlanTier::GoogleAiPro,
            models: Vec::new(),
            active_default_model: String::new(),
            settings: umisong_core::ProviderSettings::default_for(ProviderId::Antigravity),
            notes: None,
        };
        app.providers.insert(ProviderId::Antigravity, agy);
        app.providers.insert(
            ProviderId::ClaudeCode,
            ProviderStatus {
                provider: ProviderId::ClaudeCode,
                installed: false,
                binary_path: None,
                version: None,
                auth: umisong_core::AuthState::NotInstalled,
                effective_plan_tier: PlanTier::AutoDetect,
                models: Vec::new(),
                active_default_model: String::new(),
                settings: umisong_core::ProviderSettings::default_for(ProviderId::ClaudeCode),
                notes: None,
            },
        );
        app.provider_install_states.insert(
            ProviderId::ClaudeCode,
            ProviderInstallState::Installing {
                method: "official installer".into(),
                detail: "Installing Claude Code via official installer…".into(),
            },
        );
        app.providers.insert(
            ProviderId::Codex,
            ProviderStatus {
                provider: ProviderId::Codex,
                installed: false,
                binary_path: None,
                version: None,
                auth: umisong_core::AuthState::NotInstalled,
                effective_plan_tier: PlanTier::AutoDetect,
                models: Vec::new(),
                active_default_model: String::new(),
                settings: umisong_core::ProviderSettings::default_for(ProviderId::Codex),
                notes: None,
            },
        );
        app.provider_install_states.insert(
            ProviderId::Codex,
            ProviderInstallState::Failed {
                message: "network timeout".into(),
            },
        );
        app.sync_selected_model_from_catalog();
        app.sessions.push(SessionSummary {
            id: "sess-1".into(),
            workspace_id: "ws-1".into(),
            title: "Verify hover and layout".into(),
            active_provider: ProviderId::Antigravity,
            active_model: app.selected_model.clone(),
            permission_mode: PermissionMode::AskSafe,
            turn_count: 1,
            updated_at: chrono::Utc::now(),
            pinned: false,
            unread: false,
            archived: false,
        });

        for dark in [true, false] {
            app.settings.dark_theme = dark;
            for view in [
                ActiveView::Chat,
                ActiveView::Compare,
                ActiveView::Diffs,
                ActiveView::Accounts,
            ] {
                app.active_view = view;
                // Run 2 consecutive frames per size so frame-1 stored rects & hover paths execute
                for screen_size in [egui::vec2(1440.0, 900.0), egui::vec2(800.0, 600.0)] {
                    for frame_idx in 0..2 {
                        let mut raw = egui::RawInput {
                            screen_rect: Some(egui::Rect::from_min_size(
                                egui::Pos2::ZERO,
                                screen_size,
                            )),
                            ..Default::default()
                        };
                        if frame_idx == 1 {
                            raw.events.push(egui::Event::PointerMoved(egui::pos2(120.0, 220.0)));
                        }
                        let _ = ctx.run(raw, |ctx| {
                            app.render_frame(ctx);
                        });
                    }
                }
            }
        }

        // Also exercise the active streaming Chat fixture (thinking + tool cards + Safe approval banner)
        app.apply_preview_fixture(&ctx, "chat_running");
        for _ in 0..2 {
            let raw = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            };
            let _ = ctx.run(raw, |ctx| {
                app.render_frame(ctx);
            });
        }
    }

    #[test]
    fn enter_sends_and_shift_enter_inserts_newline() {
        let ctx = egui::Context::default();
        let mut app = test_app(&ctx);
        app.apply_preview_fixture(&ctx, "model_menu");
        app.chat_input = "hello".into();

        // Render frame 0 so the composer TextEdit registers its ID, then focus it
        let screen = Some(egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(1440.0, 900.0),
        ));
        let _ = ctx.run(
            egui::RawInput {
                screen_rect: screen,
                ..Default::default()
            },
            |ctx| app.render_frame(ctx),
        );

        // Find and focus the composer input ID
        let mut focus_id = None;
        ctx.memory(|m| {
            // Focus via known id_salt isn't scoped the same without parent path; use tab or direct test
            focus_id = m.focused();
        });
        let _ = focus_id;

        // Verify Shift+Enter on a focused TextEdit inserts '\n' and plain Enter consumes & sends
        let test_ctx = egui::Context::default();
        let mut text = "line1".to_string();
        let mut sent = false;
        let run_once = |ctx: &egui::Context, text: &mut String, sent: &mut bool, ev: Option<egui::Event>| {
            let mut raw = egui::RawInput {
                screen_rect: screen,
                ..Default::default()
            };
            if let Some(e) = ev {
                raw.events.push(e);
            }
            let _ = ctx.run(raw, |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    let edit_id = ui.make_persistent_id("test_enter_input");
                    let had_focus = ui.ctx().memory(|m| m.has_focus(edit_id));
                    if had_focus && crate::theme::consume_enter_to_send(ui) {
                        *sent = true;
                    }
                    let resp = ui.add(egui::TextEdit::multiline(text).id(edit_id));
                    resp.request_focus();
                });
            });
        };

        // Frame 1: acquire focus
        run_once(&test_ctx, &mut text, &mut sent, None);
        assert!(!sent);

        // Frame 2: Shift+Enter inserts newline, does NOT send
        run_once(
            &test_ctx,
            &mut text,
            &mut sent,
            Some(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::SHIFT,
            }),
        );
        assert!(!sent, "Shift+Enter must not send");
        assert!(text.contains('\n'), "Shift+Enter must insert newline, got: {text:?}");

        // Frame 3: Plain Enter sends and does NOT append another newline
        let len_before = text.len();
        run_once(
            &test_ctx,
            &mut text,
            &mut sent,
            Some(egui::Event::Key {
                key: egui::Key::Enter,
                physical_key: None,
                pressed: true,
                repeat: false,
                modifiers: egui::Modifiers::NONE,
            }),
        );
        assert!(sent, "Plain Enter must trigger send");
        assert_eq!(text.len(), len_before, "Plain Enter must not insert a trailing newline");
    }
}
