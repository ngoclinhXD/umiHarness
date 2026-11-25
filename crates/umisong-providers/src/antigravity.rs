use crate::discovery::{enriched_path, find_binary, probe_version};
use crate::{
    CommandSpec, LiveProviderInfo, ProviderAdapter, ProviderProbeCache, TurnExecutionRequest, merge_model_options, summarize_json_input,
};
use anyhow::{Context, Result};
use async_trait::async_trait;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use umisong_core::{
    AgentEvent, ApprovalDecision, AuthLoginMode, AuthState, ModelOption, PermissionMode, PlanTier,
    ProviderId, ProviderSettings, ProviderStatus, ToolKind, TurnExitReason,
};

pub struct AntigravityAdapter;

impl AntigravityAdapter {
    fn default_models(configured_default: Option<&str>) -> Vec<ModelOption> {
        let mut base = vec![ModelOption::plan_default(
            "Uses active Google Antigravity account plan model",
        )];
        if let Some(cfg) = configured_default {
            let trimmed = cfg.trim();
            if !trimmed.is_empty() {
                base.push(ModelOption {
                    id: trimmed.to_string(),
                    label: format!("{trimmed} (CLI setting)"),
                    tier_hint: "Configured in ~/.gemini/antigravity-cli/settings.json".into(),
                    is_plan_default: false,
                    discovered_live: true,
                });
            }
        }
        for (id, label, hint) in [
            ("gemini-pro", "Gemini Pro (Plan)", "High-reasoning plan model"),
            ("gemini-flash", "Gemini Flash (Plan)", "Fast interactive plan model"),
        ] {
            if !base.iter().any(|m| m.id == id) {
                base.push(ModelOption {
                    id: id.into(),
                    label: label.into(),
                    tier_hint: hint.into(),
                    is_plan_default: false,
                    discovered_live: false,
                });
            }
        }
        base
    }

    fn settings_paths() -> Vec<PathBuf> {
        let Some(home) = dirs::home_dir() else {
            return Vec::new();
        };
        vec![
            home.join(".gemini/antigravity-cli/settings.json"),
            home.join(".gemini/config/config.json"),
        ]
    }
}

#[derive(Debug, Default)]
pub struct AntigravityConfigInfo {
    pub default_model: Option<String>,
    pub adc_enabled: bool,
    pub account_hint: Option<String>,
    pub plan_hint: Option<String>,
    pub has_local_config: bool,
}

/// Map a raw Google plan string to a human-readable plan name.
pub fn normalize_antigravity_plan(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    let l = trimmed.to_ascii_lowercase();
    if l.is_empty() || l.contains("local auth") {
        return None;
    }
    let name = if l.contains("ultra") {
        if l.contains("20x") {
            "Google AI Ultra (20x)"
        } else if l.contains("5x") {
            "Google AI Ultra (5x)"
        } else {
            "Google AI Ultra"
        }
    } else if l.contains("adc") || l.contains("vertex") {
        "Vertex ADC / Enterprise"
    } else if l.contains("enterprise") {
        "Gemini Enterprise"
    } else if l.contains("workspace") {
        "Gemini with Google Workspace"
    } else if l.contains("student") {
        "Google AI Pro for Students"
    } else if l.contains("plus") {
        "Google AI Plus"
    } else if l.contains("free") {
        "Google AI Free"
    } else if l.contains("pro") {
        "Google AI Pro"
    } else {
        return Some(trimmed.to_string());
    };
    Some(name.to_string())
}

/// Parse the `session/new` response of `agy acp` into models + authoritative tier.
pub fn parse_agy_acp_session_new(val: &Value) -> LiveProviderInfo {
    let mut info = LiveProviderInfo::default();
    let Some(result) = val.get("result") else {
        return info;
    };
    let antigravity = result.get("_meta").and_then(|m| m.get("antigravity"));
    let tier = antigravity.and_then(|a| a.get("tier"));
    info.plan_hint = tier
        .and_then(|t| t.get("name").and_then(Value::as_str))
        .and_then(normalize_antigravity_plan)
        .or_else(|| {
            tier.and_then(|t| t.get("id").and_then(Value::as_str))
                .and_then(normalize_antigravity_plan)
        });
    info.account_hint = antigravity
        .and_then(|a| a.get("email"))
        .or_else(|| tier.and_then(|t| t.get("email")))
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToString::to_string);

    let mut ids: Vec<String> = result
        .get("models")
        .and_then(|m| m.get("availableModels"))
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(|m| m.get("modelId").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_default();
    if ids.is_empty() {
        if let Some(opts) = result
            .get("configOptions")
            .and_then(Value::as_array)
            .and_then(|a| a.iter().find(|o| o.get("id").and_then(Value::as_str) == Some("model")))
            .and_then(|o| o.get("options"))
            .and_then(Value::as_array)
        {
            ids = opts
                .iter()
                .filter_map(|o| o.get("value").and_then(Value::as_str))
                .map(ToString::to_string)
                .collect();
        }
    }
    for id in ids {
        if !info.models.contains(&id) {
            info.models.push(id);
        }
    }
    info
}

async fn wait_for_json_id(
    lines: &mut tokio::io::Lines<BufReader<tokio::process::ChildStdout>>,
    id: u64,
) -> Option<Value> {
    while let Ok(Some(line)) = lines.next_line().await {
        if let Ok(val) = serde_json::from_str::<Value>(line.trim()) {
            if val.get("id").and_then(Value::as_u64) == Some(id) {
                return Some(val);
            }
        }
    }
    None
}

pub fn inspect_antigravity_settings(raw_json: &str) -> AntigravityConfigInfo {
    let Ok(val) = serde_json::from_str::<Value>(raw_json) else {
        return AntigravityConfigInfo::default();
    };
    let default_model = val
        .get("model")
        .or_else(|| val.get("defaultModel"))
        .or_else(|| val.get("default_model"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let adc_enabled = val
        .get("enableAdc")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let account_hint = val
        .get("email")
        .or_else(|| val.get("account"))
        .and_then(Value::as_str)
        .map(ToString::to_string);
    let plan_hint = [
        "plan",
        "tier",
        "planTier",
        "subscription",
        "subscriptionTier",
    ]
    .iter()
    .find_map(|k| val.get(*k).and_then(Value::as_str))
    .map(ToString::to_string);
    AntigravityConfigInfo {
        default_model,
        adc_enabled,
        account_hint,
        plan_hint,
        has_local_config: true,
    }
}

#[async_trait]
impl ProviderAdapter for AntigravityAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::Antigravity
    }

    async fn probe_status(
        &self,
        settings: &ProviderSettings,
        cache: &ProviderProbeCache,
    ) -> ProviderStatus {
        let mut cfg_info = AntigravityConfigInfo::default();
        for path in Self::settings_paths() {
            if let Ok(content) = std::fs::read_to_string(&path) {
                let parsed = inspect_antigravity_settings(&content);
                if parsed.default_model.is_some() && cfg_info.default_model.is_none() {
                    cfg_info.default_model = parsed.default_model;
                }
                if parsed.account_hint.is_some() && cfg_info.account_hint.is_none() {
                    cfg_info.account_hint = parsed.account_hint;
                }
                if parsed.plan_hint.is_some() && cfg_info.plan_hint.is_none() {
                    cfg_info.plan_hint = parsed.plan_hint;
                }
                cfg_info.adc_enabled |= parsed.adc_enabled;
                cfg_info.has_local_config |= parsed.has_local_config;
            }
        }
        if std::env::var("AGY_ADC_AUTH")
            .map(|v| v.eq_ignore_ascii_case("true") || v == "1")
            .unwrap_or(false)
        {
            cfg_info.adc_enabled = true;
        }

        let models = merge_model_options(
            Self::default_models(cfg_info.default_model.as_deref()),
            &cache.discovered_models,
            &settings.custom_models,
        );

        let Some(bin) = find_binary("agy", settings.binary_override.as_deref()) else {
            return ProviderStatus {
                provider: ProviderId::Antigravity,
                installed: false,
                binary_path: None,
                version: None,
                auth: AuthState::NotInstalled,
                effective_plan_tier: PlanTier::AutoDetect,
                models,
                active_default_model: settings.default_model.clone(),
                settings: settings.clone(),
                notes: Some(ProviderId::Antigravity.install_hint().to_string()),
            };
        };

        let version = probe_version(&bin).await;

        // Check `agy auth status` if the installed CLI exposes it; otherwise infer from
        // local Antigravity credential layer & config.
        let probe_out = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            Command::new(&bin)
                .args(["auth", "status"])
                .env("PATH", enriched_path())
                .stdin(Stdio::null())
                .output(),
        )
        .await;

        let auth = match probe_out {
            Ok(Ok(out)) if out.status.success() => {
                let text = String::from_utf8_lossy(&out.stdout);
                parse_antigravity_auth_output(&text, &cfg_info)
            }
            _ if cfg_info.adc_enabled => AuthState::Authenticated {
                method: "google_adc".to_string(),
                account_hint: cfg_info.account_hint,
                plan_hint: Some("Vertex ADC / Enterprise".to_string()),
            },
            _ if has_antigravity_local_auth() => AuthState::Authenticated {
                method: "google_antigravity".to_string(),
                account_hint: cfg_info
                    .account_hint
                    .or_else(|| cache.detected_account.clone()),
                plan_hint: cfg_info
                    .plan_hint
                    .as_deref()
                    .and_then(normalize_antigravity_plan)
                    .or_else(|| cache.detected_plan.as_deref().and_then(normalize_antigravity_plan))
                    .or_else(|| Some("Google Antigravity".to_string())),
            },
            _ => AuthState::LoggedOut,
        };

        let effective_plan_tier = match settings.plan_tier_override {
            Some(t) if t != PlanTier::AutoDetect => t,
            _ => PlanTier::infer_from_auth(ProviderId::Antigravity, &auth),
        };

        ProviderStatus {
            provider: ProviderId::Antigravity,
            installed: true,
            binary_path: Some(bin.to_string_lossy().into_owned()),
            version,
            auth,
            effective_plan_tier,
            models,
            active_default_model: settings.default_model.clone(),
            settings: settings.clone(),
            notes: None,
        }
    }

    fn login_command(
        &self,
        settings: &ProviderSettings,
        _mode: AuthLoginMode,
    ) -> Result<CommandSpec> {
        let bin = find_binary("agy", settings.binary_override.as_deref()).context(
            "Antigravity CLI (`agy`) not found. Install it from https://antigravity.google/docs/cli/reference",
        )?;
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), enriched_path());
        // Launching `agy` interactively triggers the native Google OAuth / device-code flow.
        Ok(CommandSpec {
            program: bin.to_string_lossy().into_owned(),
            args: Vec::new(),
            env,
            cwd: None,
        })
    }

    async fn logout(&self, settings: &ProviderSettings) -> Result<()> {
        let bin = find_binary("agy", settings.binary_override.as_deref())
            .context("Antigravity CLI (`agy`) not found")?;
        let _ = Command::new(bin)
            .args(["auth", "logout"])
            .env("PATH", enriched_path())
            .output()
            .await;
        Ok(())
    }

    async fn discover_live_info(&self, settings: &ProviderSettings) -> Option<LiveProviderInfo> {
        let bin = find_binary("agy", settings.binary_override.as_deref())?;
        let mut child = Command::new(&bin)
            .arg("acp")
            .env("PATH", enriched_path())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .ok()?;
        let mut stdin = child.stdin.take()?;
        let stdout = child.stdout.take()?;

        let result = tokio::time::timeout(std::time::Duration::from_secs(25), async {
            let mut lines = BufReader::new(stdout).lines();
            let init = json!({
                "jsonrpc": "2.0", "id": 1, "method": "initialize",
                "params": {
                    "protocolVersion": 1,
                    "clientCapabilities": {},
                    "clientInfo": { "name": "umisong-harness", "version": "0.1.0" }
                }
            });
            stdin.write_all(format!("{init}\n").as_bytes()).await.ok()?;
            stdin.flush().await.ok()?;
            wait_for_json_id(&mut lines, 1).await?;

            let cwd = std::env::temp_dir().to_string_lossy().into_owned();
            let new_session = json!({
                "jsonrpc": "2.0", "id": 2, "method": "session/new",
                "params": { "cwd": cwd, "mcpServers": [] }
            });
            stdin
                .write_all(format!("{new_session}\n").as_bytes())
                .await
                .ok()?;
            stdin.flush().await.ok()?;
            let resp = wait_for_json_id(&mut lines, 2).await?;
            let info = parse_agy_acp_session_new(&resp);

            if let Some(sid) = resp
                .get("result")
                .and_then(|r| r.get("sessionId"))
                .and_then(Value::as_str)
            {
                let close = json!({
                    "jsonrpc": "2.0", "id": 3, "method": "session/close",
                    "params": { "sessionId": sid }
                });
                let _ = stdin.write_all(format!("{close}\n").as_bytes()).await;
                let _ = stdin.flush().await;
            }
            (info.plan_hint.is_some() || !info.models.is_empty()).then_some(info)
        })
        .await
        .ok()
        .flatten();

        let _ = child.kill().await;
        let _ = child.wait().await;
        result
    }

    async fn run_turn(
        &self,
        req: TurnExecutionRequest,
        tx: mpsc::UnboundedSender<AgentEvent>,
        mut approval_rx: mpsc::UnboundedReceiver<ApprovalDecision>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let bin = find_binary("agy", req.settings.binary_override.as_deref()).context(
            "Antigravity CLI (`agy`) is not installed. Configure or install it in the Accounts tab.",
        )?;

        let mut active_conv_id = req
            .provider_session_id
            .clone()
            .filter(|s| !s.trim().is_empty());
        let mut current_prompt = req.prompt.clone();
        let mut effective_mode = req.permission_mode;

        loop {
            let mut cmd = Command::new(&bin);
            cmd.current_dir(&req.workspace_path)
                .env("PATH", enriched_path())
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped());

            cmd.args(["-p", &current_prompt, "--output-format", "stream-json"]);

            if !req.model.trim().is_empty() {
                cmd.args(["--model", req.model.trim()]);
            }
            if let Some(effort) = req.thinking_level.as_agy_level() {
                cmd.args(["--effort", effort]);
            }
            if let Some(conv_id) = active_conv_id.as_deref().filter(|s| !s.trim().is_empty()) {
                cmd.args(["--conversation", conv_id]);
            }

            match effective_mode {
                PermissionMode::PlanReadOnly => {
                    cmd.args(["--mode", "plan"]);
                }
                PermissionMode::AskSafe => {
                    cmd.args(["--mode", "accept-edits"]);
                }
                PermissionMode::FullAuto => {
                    cmd.args(["--mode", "auto", "--dangerously-skip-permissions"]);
                }
            }

            let mut child = cmd.spawn().context("Failed to spawn `agy` CLI")?;
            let stdout = child.stdout.take().context("Missing stdout on agy")?;
            let stderr = child.stderr.take().context("Missing stderr on agy")?;

            let stderr_handle = tokio::spawn(async move {
                let mut reader = BufReader::new(stderr).lines();
                let mut buf = String::new();
                while let Ok(Some(line)) = reader.next_line().await {
                    if !buf.is_empty() {
                        buf.push('\n');
                    }
                    buf.push_str(&line);
                }
                buf
            });

            let mut lines = BufReader::new(stdout).lines();
            let mut emitted_complete = false;
            let mut last_tool: Option<(String, String, String)> = None;
            let mut denied_tool: Option<(String, String, String)> = None;
            let mut saw_denied_result = false;

            loop {
                tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = child.kill().await;
                        let _ = tx.send(AgentEvent::TurnCompleted {
                            final_text: None,
                            exit_reason: TurnExitReason::Cancelled,
                        });
                        return Ok(());
                    }
                    line_res = lines.next_line() => {
                        match line_res {
                            Ok(Some(line)) => {
                                for ev in parse_antigravity_stream_line(&line) {
                                    if let AgentEvent::SessionInit {
                                        provider_session_id: Some(ref cid),
                                        ..
                                    } = ev
                                    {
                                        if !cid.trim().is_empty() {
                                            active_conv_id = Some(cid.clone());
                                        }
                                    }

                                    if let AgentEvent::ToolCallStarted {
                                        ref call_id,
                                        ref name,
                                        ref input_summary,
                                        ..
                                    } = ev
                                    {
                                        last_tool = Some((
                                            call_id.clone(),
                                            name.clone(),
                                            input_summary.clone(),
                                        ));
                                    }

                                    if effective_mode == PermissionMode::AskSafe {
                                        if let AgentEvent::ToolCallCompleted {
                                            ref call_id,
                                            success: false,
                                            ref output,
                                            ..
                                        } = ev
                                        {
                                            let lower = output.to_ascii_lowercase();
                                            if lower.contains("permission check failed")
                                                || lower.contains("user denied permission")
                                                || lower.contains("permission auto-denied")
                                            {
                                                // Hold failed status and ask user interactively!
                                                denied_tool = last_tool.clone().or_else(|| {
                                                    Some((
                                                        call_id.clone(),
                                                        "run_command".to_string(),
                                                        output.clone(),
                                                    ))
                                                });
                                                continue;
                                            }
                                        }

                                        if let AgentEvent::Error { ref message, .. } = ev {
                                            if message.contains("headless Safe mode (permission auto-denied)") {
                                                saw_denied_result = true;
                                                continue;
                                            }
                                        }

                                        if matches!(ev, AgentEvent::TurnCompleted { .. })
                                            && (denied_tool.is_some() || saw_denied_result)
                                        {
                                            // Intercept completion so we can prompt the user for approval first
                                            continue;
                                        }
                                    }

                                    if matches!(ev, AgentEvent::TurnCompleted { .. }) {
                                        emitted_complete = true;
                                    }
                                    let _ = tx.send(ev);
                                }
                            }
                            Ok(None) => break,
                            Err(e) => {
                                let _ = tx.send(AgentEvent::Error {
                                    message: format!("Error reading agy output: {e}"),
                                    recoverable: false,
                                });
                                break;
                            }
                        }
                    }
                }
            }

            let status = child.wait().await?;
            let stderr_text = stderr_handle.await.unwrap_or_default();

            // If Safe mode intercepted a permission denial, ask the user via the Approval Banner!
            if effective_mode == PermissionMode::AskSafe
                && (denied_tool.is_some()
                    || saw_denied_result
                    || stderr_text.contains("headless mode cannot prompt for"))
            {
                let (call_id, tool_name, cmd_summary) = denied_tool
                    .or(last_tool)
                    .unwrap_or_else(|| {
                        (
                            "agy-safe-cmd".to_string(),
                            "run_command".to_string(),
                            "Execute shell command".to_string(),
                        )
                    });
                let approval_id = format!("agy-appr-{call_id}");
                let _ = tx.send(AgentEvent::ApprovalRequested {
                    approval_id: approval_id.clone(),
                    tool_name: tool_name.clone(),
                    summary: cmd_summary.clone(),
                    detail: "Safe mode requires confirmation before executing this command.".to_string(),
                });

                let decision = tokio::select! {
                    _ = cancel.cancelled() => {
                        let _ = tx.send(AgentEvent::TurnCompleted {
                            final_text: None,
                            exit_reason: TurnExitReason::Cancelled,
                        });
                        return Ok(());
                    }
                    d = approval_rx.recv() => d,
                };

                match decision {
                    Some(d) if d.approved => {
                        let _ = tx.send(AgentEvent::ToolCallCompleted {
                            call_id,
                            success: true,
                            output: "Approved by user — executing…".to_string(),
                            duration_ms: None,
                        });
                        effective_mode = PermissionMode::FullAuto;
                        current_prompt = format!(
                            "Permission approved by user to execute `{cmd_summary}`. Run it now and complete the task."
                        );
                        continue;
                    }
                    _ => {
                        let _ = tx.send(AgentEvent::ToolCallCompleted {
                            call_id,
                            success: false,
                            output: "Permission rejected by user in Safe mode.".to_string(),
                            duration_ms: None,
                        });
                        let _ = tx.send(AgentEvent::TurnCompleted {
                            final_text: Some(
                                "Command execution was rejected in Safe mode.".to_string(),
                            ),
                            exit_reason: TurnExitReason::Completed,
                        });
                        return Ok(());
                    }
                }
            }

            if !status.success() && !emitted_complete {
                let msg = if stderr_text.trim().is_empty() {
                    format!("`agy` exited with status {status}")
                } else {
                    stderr_text.trim().to_string()
                };
                let _ = tx.send(AgentEvent::Error {
                    message: msg,
                    recoverable: false,
                });
                let _ = tx.send(AgentEvent::TurnCompleted {
                    final_text: None,
                    exit_reason: TurnExitReason::Failed,
                });
            } else if !emitted_complete {
                let _ = tx.send(AgentEvent::TurnCompleted {
                    final_text: None,
                    exit_reason: TurnExitReason::Completed,
                });
            }

            return Ok(());
        }
    }
}

fn has_antigravity_local_auth() -> bool {
    let Some(home) = dirs::home_dir() else {
        return false;
    };
    home.join(".gemini/antigravity-cli/settings.json").exists()
        || home.join(".gemini/antigravity").exists()
}

fn parse_antigravity_auth_output(stdout: &str, cfg: &AntigravityConfigInfo) -> AuthState {
    if let Ok(val) = serde_json::from_str::<Value>(stdout.trim()) {
        let logged_in = val
            .get("loggedIn")
            .or_else(|| val.get("authenticated"))
            .and_then(Value::as_bool)
            .unwrap_or(true);
        if !logged_in {
            return AuthState::LoggedOut;
        }
        return AuthState::Authenticated {
            method: val
                .get("method")
                .and_then(Value::as_str)
                .unwrap_or("google_antigravity")
                .to_string(),
            account_hint: val
                .get("email")
                .and_then(Value::as_str)
                .map(ToString::to_string)
                .or_else(|| cfg.account_hint.clone()),
            plan_hint: val
                .get("plan")
                .or_else(|| val.get("tier"))
                .and_then(Value::as_str)
                .and_then(normalize_antigravity_plan),
        };
    }

    let lower = stdout.to_ascii_lowercase();
    if lower.contains("not logged in") || lower.contains("unauthenticated") {
        AuthState::LoggedOut
    } else {
        AuthState::Authenticated {
            method: "google_antigravity".to_string(),
            account_hint: cfg.account_hint.clone(),
            plan_hint: None,
        }
    }
}

/// Parse one NDJSON line emitted by `agy -p --output-format stream-json`.
pub fn parse_antigravity_stream_line(line: &str) -> Vec<AgentEvent> {
    let trimmed = line.trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    let Ok(val) = serde_json::from_str::<Value>(trimmed) else {
        return vec![AgentEvent::TextDelta {
            text: format!("{trimmed}\n"),
        }];
    };

    let mut out = Vec::new();
    let ev_type = val
        .get("type")
        .or_else(|| val.get("event"))
        .and_then(Value::as_str)
        .unwrap_or("");

    match ev_type {
        "init" | "session" | "system" => {
            let init_obj = val.get("init").unwrap_or(&val);
            out.push(AgentEvent::SessionInit {
                provider_session_id: val
                    .get("conversation_id")
                    .or_else(|| init_obj.get("conversation_id"))
                    .or_else(|| val.get("session_id"))
                    .or_else(|| val.get("sessionId"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
                resolved_model: init_obj
                    .get("model")
                    .or_else(|| val.get("model"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string),
            });
        }
        "step_update" => {
            let step = val.get("step_update").unwrap_or(&val);
            let step_type = step
                .get("step_type")
                .and_then(Value::as_str)
                .unwrap_or("");
            let state = step.get("state").and_then(Value::as_str).unwrap_or("");
            let step_idx = step
                .get("step_index")
                .and_then(Value::as_u64)
                .unwrap_or(0);

            if let Some(thought) = step
                .get("thinking_delta")
                .or_else(|| step.get("thought_delta"))
                .or_else(|| step.get("reasoning_delta"))
                .or_else(|| step.get("thinking"))
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                out.push(AgentEvent::ThoughtDelta {
                    text: thought.to_string(),
                });
            }

            if let Some(delta) = step
                .get("text_delta")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
            {
                out.push(AgentEvent::TextDelta {
                    text: delta.to_string(),
                });
            }

            if step_type == "tool"
                || step.get("tool_info").is_some()
                || step.get("tool_name").is_some()
            {
                let info = step.get("tool_info").unwrap_or(step);
                let name = info
                    .get("tool_name")
                    .or_else(|| info.get("name"))
                    .or_else(|| step.get("tool_name"))
                    .and_then(Value::as_str)
                    .unwrap_or("tool")
                    .to_string();
                let call_id = info
                    .get("tool_id")
                    .or_else(|| info.get("call_id"))
                    .or_else(|| info.get("id"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .unwrap_or_else(|| format!("agy-step-{step_idx}"));
                let duration_ms = step
                    .get("duration_seconds")
                    .and_then(Value::as_f64)
                    .map(|s| (s * 1000.0).round() as u64)
                    .or_else(|| info.get("duration_ms").and_then(Value::as_u64));

                if state.eq_ignore_ascii_case("DONE")
                    || state.eq_ignore_ascii_case("COMPLETED")
                    || state.eq_ignore_ascii_case("ERROR")
                    || state.eq_ignore_ascii_case("FAILED")
                {
                    let err_obj_msg = info.get("error").and_then(|e| match e {
                        Value::String(s) if !s.is_empty() => Some(s.clone()),
                        Value::Object(map) => map
                            .get("message")
                            .and_then(Value::as_str)
                            .map(ToString::to_string),
                        _ => None,
                    });
                    let failed = state.eq_ignore_ascii_case("ERROR")
                        || state.eq_ignore_ascii_case("FAILED")
                        || err_obj_msg.is_some();
                    let output = err_obj_msg
                        .or_else(|| {
                            info.get("output")
                                .or_else(|| info.get("result"))
                                .map(|v| match v {
                                    Value::String(s) => s.clone(),
                                    other => other.to_string(),
                                })
                        })
                        .unwrap_or_default();
                    out.push(AgentEvent::ToolCallCompleted {
                        call_id,
                        success: !failed,
                        output,
                        duration_ms,
                    });
                } else {
                    let params = info
                        .get("parameters")
                        .or_else(|| info.get("args"))
                        .or_else(|| info.get("input"))
                        .cloned()
                        .unwrap_or(Value::Null);
                    out.push(AgentEvent::ToolCallStarted {
                        call_id,
                        kind: ToolKind::classify(&name),
                        name,
                        input_summary: summarize_json_input(&params),
                        raw_input: Some(params.to_string()),
                    });
                }
            }

            if let Some(usage) = step.get("usage") {
                out.push(extract_agy_usage(usage));
            }
        }
        "thought" | "thinking" | "thought_delta" | "reasoning" => {
            if let Some(text) = val
                .get("text")
                .or_else(|| val.get("delta"))
                .or_else(|| val.get("content"))
                .and_then(Value::as_str)
            {
                out.push(AgentEvent::ThoughtDelta {
                    text: text.to_string(),
                });
            }
        }
        "message" | "text" | "text_delta" | "assistant" | "content_delta" => {
            if let Some(role) = val.get("role").and_then(Value::as_str) {
                if role == "user" {
                    return out;
                }
            }
            if let Some(text) = val
                .get("delta")
                .or_else(|| val.get("text"))
                .or_else(|| val.get("content"))
                .and_then(Value::as_str)
            {
                out.push(AgentEvent::TextDelta {
                    text: text.to_string(),
                });
            }
        }
        "tool_use" | "tool_call" | "tool_call_started" => {
            let name = val
                .get("tool_name")
                .or_else(|| val.get("name"))
                .and_then(Value::as_str)
                .unwrap_or("tool")
                .to_string();
            let call_id = val
                .get("tool_id")
                .or_else(|| val.get("call_id"))
                .or_else(|| val.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("agy-tool")
                .to_string();
            let params = val
                .get("parameters")
                .or_else(|| val.get("args"))
                .or_else(|| val.get("input"))
                .cloned()
                .unwrap_or(Value::Null);
            out.push(AgentEvent::ToolCallStarted {
                call_id,
                kind: ToolKind::classify(&name),
                name,
                input_summary: summarize_json_input(&params),
                raw_input: Some(params.to_string()),
            });
        }
        "tool_result" | "tool_call_completed" => {
            let call_id = val
                .get("tool_id")
                .or_else(|| val.get("call_id"))
                .or_else(|| val.get("id"))
                .and_then(Value::as_str)
                .unwrap_or("agy-tool")
                .to_string();
            let status_str = val.get("status").and_then(Value::as_str).unwrap_or("success");
            let success = val
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or_else(|| {
                    !status_str.eq_ignore_ascii_case("error")
                        && !status_str.eq_ignore_ascii_case("failed")
                });
            let output = val
                .get("output")
                .or_else(|| val.get("result"))
                .map(|v| match v {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                })
                .unwrap_or_default();
            out.push(AgentEvent::ToolCallCompleted {
                call_id,
                success,
                output,
                duration_ms: val.get("duration_ms").and_then(Value::as_u64),
            });
        }
        "result" | "done" | "complete" => {
            let res_obj = val.get("result").filter(|v| v.is_object()).unwrap_or(&val);
            if let Some(stats) = res_obj
                .get("usage")
                .or_else(|| res_obj.get("stats"))
                .or_else(|| val.get("stats"))
                .or_else(|| val.get("usage"))
            {
                out.push(extract_agy_usage(stats));
            }
            let status_str = res_obj
                .get("status")
                .or_else(|| val.get("status"))
                .and_then(Value::as_str)
                .unwrap_or("SUCCESS");
            let failed = status_str.eq_ignore_ascii_case("error")
                || status_str.eq_ignore_ascii_case("failed");
            let final_text = res_obj
                .get("response")
                .or_else(|| val.get("response"))
                .or_else(|| val.get("result"))
                .and_then(Value::as_str)
                .map(ToString::to_string);
            let denied: Vec<String> = res_obj
                .get("denied_actions")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(|item| {
                            item.get("display_name")
                                .or_else(|| item.get("action"))
                                .and_then(Value::as_str)
                                .map(ToString::to_string)
                        })
                        .collect()
                })
                .unwrap_or_default();
            if failed {
                let err_msg = res_obj
                    .get("error")
                    .or_else(|| val.get("error"))
                    .and_then(Value::as_str)
                    .map(ToString::to_string)
                    .or_else(|| final_text.clone())
                    .unwrap_or_else(|| "Antigravity returned an error result".to_string());
                out.push(AgentEvent::Error {
                    message: err_msg,
                    recoverable: false,
                });
            } else if !denied.is_empty()
                && final_text.as_deref().unwrap_or("").trim().is_empty()
            {
                out.push(AgentEvent::Error {
                    message: format!(
                        "Antigravity could not execute {} in headless Safe mode (permission auto-denied). Switch to Auto mode or add an allow rule in ~/.gemini/antigravity-cli/settings.json.",
                        denied.join(", ")
                    ),
                    recoverable: true,
                });
            }
            out.push(AgentEvent::TurnCompleted {
                final_text,
                exit_reason: if failed {
                    TurnExitReason::Failed
                } else {
                    TurnExitReason::Completed
                },
            });
        }
        "error" => {
            let message = val
                .get("message")
                .or_else(|| val.get("error"))
                .and_then(Value::as_str)
                .unwrap_or(trimmed)
                .to_string();
            out.push(AgentEvent::Error {
                message,
                recoverable: false,
            });
        }
        _ => {
            if let Some(t) = val.get("text").and_then(Value::as_str) {
                out.push(AgentEvent::TextDelta {
                    text: t.to_string(),
                });
            }
        }
    }

    out
}

fn extract_agy_usage(stats: &Value) -> AgentEvent {
    AgentEvent::UsageUpdate {
        input_tokens: stats
            .get("input_tokens")
            .or_else(|| stats.get("prompt_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: stats
            .get("output_tokens")
            .or_else(|| stats.get("completion_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_tokens: stats
            .get("cache_read_tokens")
            .or_else(|| stats.get("cached_tokens"))
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cost_usd: stats.get("cost_usd").and_then(Value::as_f64),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_live_agy_v1_3_stream_json_events() {
        let lines = [
            r#"{"event":"init","conversation_id":"2b5deb90-e27e-4990-8e8b-af331bc5f00f","init":{"model":"gemini-4-hydrogen","cwd":"/tmp"}}"#,
            r#"{"event":"step_update","step_update":{"conversation_id":"2b5deb90-e27e-4990-8e8b-af331bc5f00f","step_index":0,"state":"DONE","step_type":"user_input"}}"#,
            r#"{"event":"step_update","step_update":{"conversation_id":"2b5deb90-e27e-4990-8e8b-af331bc5f00f","step_index":1,"state":"ACTIVE","step_type":"agent_response","text_delta":"ok"}}"#,
            r#"{"event":"step_update","step_update":{"conversation_id":"2b5deb90-e27e-4990-8e8b-af331bc5f00f","step_index":1,"state":"DONE","step_type":"agent_response","text_delta":"\n","duration_seconds":2.8,"usage":{"input_tokens":14798,"output_tokens":60,"cache_read_tokens":0}}}"#,
            r#"{"event":"result","result":{"conversation_id":"2b5deb90-e27e-4990-8e8b-af331bc5f00f","status":"SUCCESS","response":"ok\n","usage":{"input_tokens":14798,"output_tokens":60,"cache_read_tokens":0}}}"#,
        ];

        let mut all = Vec::new();
        for l in lines {
            all.extend(parse_antigravity_stream_line(l));
        }

        assert!(all.iter().any(|e| matches!(
            e,
            AgentEvent::SessionInit {
                provider_session_id: Some(s),
                resolved_model: Some(m)
            } if s == "2b5deb90-e27e-4990-8e8b-af331bc5f00f" && m == "gemini-4-hydrogen"
        )));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::TextDelta { text } if text == "ok")));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::UsageUpdate { input_tokens: 14798, output_tokens: 60, .. })));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { final_text: Some(t), exit_reason: TurnExitReason::Completed } if t == "ok\n")));
    }

    #[test]
    fn parses_agy_acp_session_new_tier_and_models() {
        let v: Value = serde_json::from_str(
            r#"{"jsonrpc":"2.0","id":2,"result":{"sessionId":"s1","models":{"availableModels":[{"modelId":"gemini-4-neon-high"},{"modelId":"gemini-4-carbon-high"},{"modelId":"gemini-4-neon-high"}]},"_meta":{"antigravity":{"tier":{"name":"Google AI Pro","id":"g1-pro-tier","upgradeUri":"https://x"}}}}}"#,
        )
        .unwrap();
        let info = parse_agy_acp_session_new(&v);
        // Pro accounts also list Carbon: tier must come from _meta, not the models.
        assert_eq!(info.plan_hint.as_deref(), Some("Google AI Pro"));
        assert_eq!(info.models, vec!["gemini-4-neon-high", "gemini-4-carbon-high"]);

        let by_id = |id: &str| {
            let v = json!({"result":{"_meta":{"antigravity":{"tier":{"id": id}}}}});
            parse_agy_acp_session_new(&v).plan_hint
        };
        assert_eq!(by_id("g1-ultra-tier").as_deref(), Some("Google AI Ultra"));
        assert_eq!(by_id("free-tier").as_deref(), Some("Google AI Free"));
        assert_eq!(parse_agy_acp_session_new(&json!({})).plan_hint, None);
    }

    #[test]
    fn normalizes_antigravity_plans() {
        assert_eq!(normalize_antigravity_plan("ultra 20x").as_deref(), Some("Google AI Ultra (20x)"));
        assert_eq!(normalize_antigravity_plan("Google AI Pro for Students").as_deref(), Some("Google AI Pro for Students"));
        assert_eq!(normalize_antigravity_plan("plus").as_deref(), Some("Google AI Plus"));
        assert_eq!(normalize_antigravity_plan("pro").as_deref(), Some("Google AI Pro"));
        assert_eq!(normalize_antigravity_plan("Antigravity Local Auth"), None);
        let cfg = inspect_antigravity_settings(r#"{"tier":"ultra"}"#);
        assert_eq!(cfg.plan_hint.as_deref(), Some("ultra"));
    }

    #[test]
    fn inspects_antigravity_settings_file() {
        let cfg = inspect_antigravity_settings(
            r#"{"model":"gemini-3-pro","enableAdc":true,"email":"user@google.com"}"#,
        );
        assert_eq!(cfg.default_model.as_deref(), Some("gemini-3-pro"));
        assert!(cfg.adc_enabled);
        assert_eq!(cfg.account_hint.as_deref(), Some("user@google.com"));
    }

    #[test]
    fn parses_antigravity_stream_events() {
        let lines = [
            r#"{"type":"init","session_id":"agy-42","model":"gemini-pro"}"#,
            r#"{"type":"thought","text":"Mapping module graph..."}"#,
            r#"{"type":"tool_use","tool_id":"t1","tool_name":"read_file","parameters":{"path":"Cargo.toml"}}"#,
            r#"{"type":"tool_result","tool_id":"t1","status":"success","output":"[workspace]"}"#,
            r#"{"type":"message","role":"assistant","delta":"Here is the workspace overview."}"#,
            r#"{"type":"result","status":"success","stats":{"input_tokens":200,"output_tokens":90}}"#,
        ];

        let mut all = Vec::new();
        for l in lines {
            all.extend(parse_antigravity_stream_line(l));
        }

        assert!(all.iter().any(|e| matches!(
            e,
            AgentEvent::SessionInit {
                provider_session_id: Some(s),
                resolved_model: Some(m)
            } if s == "agy-42" && m == "gemini-pro"
        )));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::ThoughtDelta { text } if text.contains("Mapping"))));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::ToolCallStarted { kind: ToolKind::FileRead, .. })));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::ToolCallCompleted { success: true, .. })));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::TextDelta { text } if text.contains("overview"))));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { exit_reason: TurnExitReason::Completed, .. })));
    }
}
