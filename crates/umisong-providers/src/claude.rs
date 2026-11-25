use crate::discovery::{enriched_path, find_binary, probe_version};
use crate::{
    CommandSpec, ProviderAdapter, ProviderProbeCache, TurnExecutionRequest, merge_model_options, summarize_json_input,
};
use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use serde_json::Value;
use std::collections::HashMap;
use std::process::Stdio;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use umisong_core::{
    AgentEvent, ApprovalDecision, AuthLoginMode, AuthState, ModelOption, PermissionMode, PlanTier,
    ProviderId, ProviderSettings, ProviderStatus, ToolKind, TurnExitReason,
};

pub struct ClaudeCodeAdapter;

impl ClaudeCodeAdapter {
    fn default_models() -> Vec<ModelOption> {
        vec![
            ModelOption::plan_default("Uses active Claude Pro / Max / Team plan model"),
            ModelOption {
                id: "sonnet".into(),
                label: "Sonnet (Latest)".into(),
                tier_hint: "Balanced daily coding".into(),
                is_plan_default: false,
                discovered_live: false,
            },
            ModelOption {
                id: "opus".into(),
                label: "Opus (Latest)".into(),
                tier_hint: "Deep reasoning & architecture".into(),
                is_plan_default: false,
                discovered_live: false,
            },
            ModelOption {
                id: "haiku".into(),
                label: "Haiku (Latest)".into(),
                tier_hint: "Fast lightweight tasks".into(),
                is_plan_default: false,
                discovered_live: false,
            },
            ModelOption {
                id: "best".into(),
                label: "Best Available".into(),
                tier_hint: "Highest capability tier on plan".into(),
                is_plan_default: false,
                discovered_live: false,
            },
            ModelOption {
                id: "sonnet[1m]".into(),
                label: "Sonnet 1M Context".into(),
                tier_hint: "Extended context (if supported by plan)".into(),
                is_plan_default: false,
                discovered_live: false,
            },
            ModelOption {
                id: "opus[1m]".into(),
                label: "Opus 1M Context".into(),
                tier_hint: "Extended context (if supported by plan)".into(),
                is_plan_default: false,
                discovered_live: false,
            },
        ]
    }
}

#[async_trait]
impl ProviderAdapter for ClaudeCodeAdapter {
    fn id(&self) -> ProviderId {
        ProviderId::ClaudeCode
    }

    async fn probe_status(
        &self,
        settings: &ProviderSettings,
        cache: &ProviderProbeCache,
    ) -> ProviderStatus {
        let models = merge_model_options(
            Self::default_models(),
            &cache.discovered_models,
            &settings.custom_models,
        );
        let Some(bin) = find_binary("claude", settings.binary_override.as_deref()) else {
            return ProviderStatus {
                provider: ProviderId::ClaudeCode,
                installed: false,
                binary_path: None,
                version: None,
                auth: AuthState::NotInstalled,
                effective_plan_tier: PlanTier::AutoDetect,
                models,
                active_default_model: settings.default_model.clone(),
                settings: settings.clone(),
                notes: Some(format!("Install via: {}", ProviderId::ClaudeCode.install_hint())),
            };
        };

        let version = probe_version(&bin).await;
        let mut cmd = Command::new(&bin);
        cmd.args(["auth", "status"])
            .env("PATH", enriched_path())
            .stdin(Stdio::null());
        if let Some(key) = settings.api_key_override.as_deref().filter(|s| !s.trim().is_empty()) {
            cmd.env("ANTHROPIC_API_KEY", key.trim());
        }

        let auth = match tokio::time::timeout(std::time::Duration::from_secs(6), cmd.output()).await
        {
            Ok(Ok(out)) => {
                let stdout = String::from_utf8_lossy(&out.stdout);
                let stderr = String::from_utf8_lossy(&out.stderr);
                parse_claude_auth_status(&stdout, &stderr, out.status.success())
            }
            Ok(Err(e)) => AuthState::Unknown {
                detail: format!("Failed to invoke `claude auth status`: {e}"),
            },
            Err(_) => AuthState::Unknown {
                detail: "Timed out checking `claude auth status`".to_string(),
            },
        };

        let auth = apply_api_key_plan(auth, settings.api_key_override.as_deref());

        let effective_plan_tier = match settings.plan_tier_override {
            Some(t) if t != PlanTier::AutoDetect => t,
            _ => PlanTier::infer_from_auth(ProviderId::ClaudeCode, &auth),
        };

        ProviderStatus {
            provider: ProviderId::ClaudeCode,
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
        mode: AuthLoginMode,
    ) -> Result<CommandSpec> {
        let bin = find_binary("claude", settings.binary_override.as_deref()).context(
            "Claude Code CLI (`claude`) not found. Install it via `npm install -g @anthropic-ai/claude-code`.",
        )?;
        let mut args = vec!["auth".to_string(), "login".to_string()];
        if matches!(mode, AuthLoginMode::ApiKey) {
            args.push("--console".to_string());
        }
        let mut env = HashMap::new();
        env.insert("PATH".to_string(), enriched_path());
        Ok(CommandSpec {
            program: bin.to_string_lossy().into_owned(),
            args,
            env,
            cwd: None,
        })
    }

    async fn logout(&self, settings: &ProviderSettings) -> Result<()> {
        let bin = find_binary("claude", settings.binary_override.as_deref())
            .context("Claude Code CLI (`claude`) not found")?;
        let out = Command::new(bin)
            .args(["auth", "logout"])
            .env("PATH", enriched_path())
            .output()
            .await?;
        if !out.status.success() {
            bail!(
                "claude auth logout failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        Ok(())
    }

    async fn run_turn(
        &self,
        req: TurnExecutionRequest,
        tx: mpsc::UnboundedSender<AgentEvent>,
        mut approval_rx: mpsc::UnboundedReceiver<ApprovalDecision>,
        cancel: CancellationToken,
    ) -> Result<()> {
        let bin = find_binary("claude", req.settings.binary_override.as_deref()).context(
            "Claude Code CLI (`claude`) is not installed. Configure or install it in the Accounts tab.",
        )?;

        let mut active_sess_id = req
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

            if let Some(key) = req
                .settings
                .api_key_override
                .as_deref()
                .filter(|s| !s.trim().is_empty())
            {
                cmd.env("ANTHROPIC_API_KEY", key.trim());
            }

            cmd.args([
                "-p",
                &current_prompt,
                "--output-format",
                "stream-json",
                "--verbose",
                "--include-partial-messages",
            ]);

            if !req.model.trim().is_empty() {
                cmd.args(["--model", req.model.trim()]);
            }
            if let Some(effort) = req.thinking_level.as_claude_effort() {
                cmd.args(["--effort", effort]);
            }
            if let Some(sess_id) = active_sess_id.as_deref().filter(|s| !s.trim().is_empty()) {
                cmd.args(["--resume", sess_id]);
            }

            match effective_mode {
                PermissionMode::PlanReadOnly => {
                    cmd.args(["--permission-mode", "plan"]);
                }
                PermissionMode::AskSafe => {
                    cmd.args(["--permission-mode", "acceptEdits"]);
                }
                PermissionMode::FullAuto => {
                    cmd.args(["--permission-mode", "bypassPermissions"]);
                }
            }

            let mut child = cmd.spawn().context("Failed to spawn `claude` CLI")?;
            let stdout = child.stdout.take().context("Missing stdout on claude")?;
            let stderr = child.stderr.take().context("Missing stderr on claude")?;

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
            let mut saw_stream_deltas = false;
            let mut last_tool: Option<(String, String, String)> = None;
            let mut denied_tool: Option<(String, String, String)> = None;

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
                                let events = parse_claude_stream_line(&line, &mut saw_stream_deltas);
                                for ev in events {
                                    if let AgentEvent::SessionInit {
                                        provider_session_id: Some(ref sid),
                                        ..
                                    } = ev
                                    {
                                        if !sid.trim().is_empty() {
                                            active_sess_id = Some(sid.clone());
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
                                            if lower.contains("permission")
                                                || lower.contains("not allowed")
                                                || lower.contains("approval")
                                            {
                                                denied_tool = last_tool.clone().or_else(|| {
                                                    Some((
                                                        call_id.clone(),
                                                        "Bash".to_string(),
                                                        output.clone(),
                                                    ))
                                                });
                                                continue;
                                            }
                                        }

                                        if matches!(ev, AgentEvent::TurnCompleted { .. })
                                            && denied_tool.is_some()
                                        {
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
                                    message: format!("Error reading claude output: {e}"),
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

            if effective_mode == PermissionMode::AskSafe {
                if let Some((call_id, tool_name, cmd_summary)) = denied_tool {
                    let approval_id = format!("claude-appr-{call_id}");
                    let _ = tx.send(AgentEvent::ApprovalRequested {
                        approval_id: approval_id.clone(),
                        tool_name: tool_name.clone(),
                        summary: cmd_summary.clone(),
                        detail: "Safe mode requires confirmation before executing this action.".to_string(),
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
                                    "Action execution was rejected in Safe mode.".to_string(),
                                ),
                                exit_reason: TurnExitReason::Completed,
                            });
                            return Ok(());
                        }
                    }
                }
            }

            if !status.success() && !emitted_complete {
                let msg = if stderr_text.trim().is_empty() {
                    format!("`claude` exited with status {status}")
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

/// Turn raw `claude auth status` fields into a human-readable plan name.
pub fn normalize_claude_plan(
    raw_sub: Option<&str>,
    rate_limit_tier: Option<&str>,
    auth_method: &str,
    api_provider: Option<&str>,
) -> Option<String> {
    let provider = api_provider.unwrap_or("").to_ascii_lowercase();
    if provider.contains("bedrock") {
        return Some("Amazon Bedrock".into());
    }
    if provider.contains("vertex") {
        return Some("Google Vertex AI".into());
    }
    if provider.contains("foundry") {
        return Some("Azure AI Foundry".into());
    }
    let method = auth_method.to_ascii_lowercase();
    if method.contains("api") || method.contains("console") {
        return Some("Anthropic API".into());
    }
    let sub = raw_sub.unwrap_or("").trim();
    let sub_l = sub.to_ascii_lowercase();
    let tier_l = rate_limit_tier.unwrap_or("").to_ascii_lowercase();
    let both = format!("{sub_l} {tier_l}");
    let has = |k: &str| both.contains(k);
    if has("ultra") {
        return Some("Claude Ultra".into());
    }
    if has("max") {
        return Some(
            if has("20x") {
                "Claude Max (20x)"
            } else if has("5x") {
                "Claude Max (5x)"
            } else {
                "Claude Max"
            }
            .into(),
        );
    }
    if has("scientist") {
        return Some("Claude for Scientists".into());
    }
    if has("education") || has("edu") || has("teacher") {
        return Some("Claude for Education".into());
    }
    if has("enterprise") {
        return Some("Claude Enterprise".into());
    }
    if has("team") {
        return Some("Claude Team".into());
    }
    if has("pro") {
        return Some("Claude Pro".into());
    }
    if has("free") {
        return Some("Claude Free".into());
    }
    if !sub.is_empty() && !sub_l.contains("stripe") && !sub_l.contains("apple") {
        return Some(sub.to_string());
    }
    Some("Claude.ai Subscription".into())
}

fn apply_api_key_plan(auth: AuthState, api_key: Option<&str>) -> AuthState {
    let has_key = api_key.is_some_and(|k| !k.trim().is_empty());
    match auth {
        AuthState::Authenticated {
            method,
            account_hint,
            plan_hint,
        } if has_key => {
            let cloud = plan_hint.as_deref().is_some_and(|p| {
                matches!(
                    p,
                    "Anthropic API" | "Amazon Bedrock" | "Google Vertex AI" | "Azure AI Foundry"
                )
            });
            AuthState::Authenticated {
                method,
                account_hint,
                plan_hint: if cloud {
                    plan_hint
                } else {
                    Some("Anthropic API".into())
                },
            }
        }
        other => other,
    }
}

/// Parse `claude auth status` JSON (or fallback text) output into a structured `AuthState`.
pub fn parse_claude_auth_status(stdout: &str, stderr: &str, exit_ok: bool) -> AuthState {
    if let Ok(val) = serde_json::from_str::<Value>(stdout.trim()) {
        let logged_in = val
            .get("loggedIn")
            .and_then(Value::as_bool)
            .unwrap_or(exit_ok);
        if !logged_in {
            return AuthState::LoggedOut;
        }
        let method = val
            .get("authMethod")
            .and_then(Value::as_str)
            .unwrap_or("claude.ai")
            .to_string();
        let account_hint = val
            .get("email")
            .or_else(|| val.get("accountEmail"))
            .or_else(|| val.get("orgName"))
            .and_then(Value::as_str)
            .map(ToString::to_string);
        let plan_hint = normalize_claude_plan(
            val.get("subscriptionType")
                .or_else(|| val.get("plan"))
                .and_then(Value::as_str),
            val.get("rateLimitTier").and_then(Value::as_str),
            &method,
            val.get("apiProvider").and_then(Value::as_str),
        );
        return AuthState::Authenticated {
            method,
            account_hint,
            plan_hint,
        };
    }

    let combined = format!("{stdout}\n{stderr}").to_ascii_lowercase();
    if combined.contains("not logged in") || combined.contains("logged out") || !exit_ok {
        AuthState::LoggedOut
    } else if combined.contains("logged in") || combined.contains("authenticated") {
        AuthState::Authenticated {
            method: "claude.ai".to_string(),
            account_hint: None,
            plan_hint: None,
        }
    } else {
        AuthState::Unknown {
            detail: stdout.trim().to_string(),
        }
    }
}

/// Parse one NDJSON line emitted by `claude -p --output-format stream-json --verbose --include-partial-messages`.
pub fn parse_claude_stream_line(line: &str, saw_stream_deltas: &mut bool) -> Vec<AgentEvent> {
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
    let ev_type = val.get("type").and_then(Value::as_str).unwrap_or("");

    match ev_type {
        "system" => {
            let subtype = val.get("subtype").and_then(Value::as_str).unwrap_or("");
            if subtype == "init" || val.get("session_id").is_some() || val.get("model").is_some() {
                out.push(AgentEvent::SessionInit {
                    provider_session_id: val
                        .get("session_id")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                    resolved_model: val
                        .get("model")
                        .and_then(Value::as_str)
                        .map(ToString::to_string),
                });
            }
        }
        "stream_event" => {
            if let Some(inner) = val.get("event") {
                let inner_type = inner.get("type").and_then(Value::as_str).unwrap_or("");
                match inner_type {
                    "content_block_delta" => {
                        if let Some(delta) = inner.get("delta") {
                            let d_type = delta.get("type").and_then(Value::as_str).unwrap_or("");
                            if d_type == "text_delta" {
                                if let Some(t) = delta.get("text").and_then(Value::as_str) {
                                    *saw_stream_deltas = true;
                                    out.push(AgentEvent::TextDelta {
                                        text: t.to_string(),
                                    });
                                }
                            } else if d_type == "thinking_delta" {
                                if let Some(t) = delta.get("thinking").and_then(Value::as_str) {
                                    out.push(AgentEvent::ThoughtDelta {
                                        text: t.to_string(),
                                    });
                                }
                            }
                        }
                    }
                    "content_block_start" => {
                        if let Some(block) = inner.get("content_block") {
                            if block.get("type").and_then(Value::as_str) == Some("tool_use") {
                                let name = block
                                    .get("name")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool")
                                    .to_string();
                                let call_id = block
                                    .get("id")
                                    .and_then(Value::as_str)
                                    .unwrap_or("tool-call")
                                    .to_string();
                                let input_val = block.get("input").cloned().unwrap_or(Value::Null);
                                out.push(AgentEvent::ToolCallStarted {
                                    call_id,
                                    kind: ToolKind::classify(&name),
                                    name,
                                    input_summary: summarize_json_input(&input_val),
                                    raw_input: Some(input_val.to_string()),
                                });
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        "assistant" => {
            if let Some(content) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_array)
            {
                for block in content {
                    let b_type = block.get("type").and_then(Value::as_str).unwrap_or("");
                    match b_type {
                        "text" if !*saw_stream_deltas => {
                            if let Some(t) = block.get("text").and_then(Value::as_str) {
                                out.push(AgentEvent::TextDelta {
                                    text: t.to_string(),
                                });
                            }
                        }
                        "thinking" if !*saw_stream_deltas => {
                            if let Some(t) = block.get("thinking").and_then(Value::as_str) {
                                out.push(AgentEvent::ThoughtDelta {
                                    text: t.to_string(),
                                });
                            }
                        }
                        "tool_use" => {
                            let name = block
                                .get("name")
                                .and_then(Value::as_str)
                                .unwrap_or("tool")
                                .to_string();
                            let call_id = block
                                .get("id")
                                .and_then(Value::as_str)
                                .unwrap_or("tool-call")
                                .to_string();
                            let input_val = block.get("input").cloned().unwrap_or(Value::Null);
                            out.push(AgentEvent::ToolCallStarted {
                                call_id,
                                kind: ToolKind::classify(&name),
                                name,
                                input_summary: summarize_json_input(&input_val),
                                raw_input: Some(input_val.to_string()),
                            });
                        }
                        _ => {}
                    }
                }
            }
            if let Some(usage) = val.get("message").and_then(|m| m.get("usage")) {
                out.push(extract_claude_usage(usage, None));
            }
        }
        "user" => {
            if let Some(content) = val
                .get("message")
                .and_then(|m| m.get("content"))
                .and_then(Value::as_array)
            {
                for block in content {
                    if block.get("type").and_then(Value::as_str) == Some("tool_result") {
                        let call_id = block
                            .get("tool_use_id")
                            .and_then(Value::as_str)
                            .unwrap_or("tool-call")
                            .to_string();
                        let is_error = block
                            .get("is_error")
                            .and_then(Value::as_bool)
                            .unwrap_or(false);
                        let output = match block.get("content") {
                            Some(Value::String(s)) => s.clone(),
                            Some(v) => v.to_string(),
                            None => String::new(),
                        };
                        out.push(AgentEvent::ToolCallCompleted {
                            call_id,
                            success: !is_error,
                            output,
                            duration_ms: None,
                        });
                    }
                }
            }
        }
        "result" => {
            if let Some(usage) = val.get("usage") {
                let cost = val.get("total_cost_usd").and_then(Value::as_f64);
                out.push(extract_claude_usage(usage, cost));
            }
            let is_error = val
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let final_text = val
                .get("result")
                .and_then(Value::as_str)
                .map(ToString::to_string);
            if is_error {
                out.push(AgentEvent::Error {
                    message: final_text
                        .clone()
                        .unwrap_or_else(|| "Claude Code returned an error result".to_string()),
                    recoverable: false,
                });
            }
            out.push(AgentEvent::TurnCompleted {
                final_text,
                exit_reason: if is_error {
                    TurnExitReason::Failed
                } else {
                    TurnExitReason::Completed
                },
            });
        }
        "error" => {
            let message = val
                .get("error")
                .and_then(|e| e.get("message").or(Some(e)))
                .and_then(Value::as_str)
                .unwrap_or(trimmed)
                .to_string();
            out.push(AgentEvent::Error {
                message,
                recoverable: false,
            });
        }
        _ => {}
    }

    out
}

fn extract_claude_usage(usage: &Value, cost_usd: Option<f64>) -> AgentEvent {
    AgentEvent::UsageUpdate {
        input_tokens: usage
            .get("input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        output_tokens: usage
            .get("output_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cache_read_tokens: usage
            .get("cache_read_input_tokens")
            .and_then(Value::as_u64)
            .unwrap_or(0),
        cost_usd,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_claude_auth_json() {
        let json = r#"{"loggedIn":true,"authMethod":"claude.ai","email":"dev@example.com","subscriptionType":"max"}"#;
        let state = parse_claude_auth_status(json, "", true);
        assert_eq!(
            state,
            AuthState::Authenticated {
                method: "claude.ai".into(),
                account_hint: Some("dev@example.com".into()),
                plan_hint: Some("Claude Max".into()),
            }
        );

        let logged_out = r#"{"loggedIn":false,"authMethod":"none"}"#;
        assert_eq!(
            parse_claude_auth_status(logged_out, "", false),
            AuthState::LoggedOut
        );
    }

    #[test]
    fn normalizes_claude_plans() {
        let n = |sub: Option<&str>, tier: Option<&str>, m: &str, p: Option<&str>| {
            normalize_claude_plan(sub, tier, m, p).unwrap()
        };
        assert_eq!(n(Some("max"), Some("default_claude_max_20x"), "claude.ai", None), "Claude Max (20x)");
        assert_eq!(n(Some("max"), Some("default_claude_max_5x"), "claude.ai", None), "Claude Max (5x)");
        assert_eq!(n(Some("pro"), None, "claude.ai", None), "Claude Pro");
        assert_eq!(n(Some("free"), None, "claude.ai", None), "Claude Free");
        assert_eq!(n(Some("team"), None, "claude.ai", None), "Claude Team");
        assert_eq!(n(Some("enterprise"), None, "claude.ai", None), "Claude Enterprise");
        assert_eq!(n(Some("ultra"), None, "claude.ai", None), "Claude Ultra");
        assert_eq!(n(Some("education"), None, "claude.ai", None), "Claude for Education");
        assert_eq!(n(Some("scientists"), None, "claude.ai", None), "Claude for Scientists");
        assert_eq!(n(None, None, "api_key", None), "Anthropic API");
        assert_eq!(n(None, None, "claude.ai", Some("bedrock")), "Amazon Bedrock");
        assert_eq!(n(Some("stripe_subscription"), None, "claude.ai", None), "Claude.ai Subscription");
    }

    #[test]
    fn api_key_override_marks_plan_as_api() {
        let auth = AuthState::Authenticated {
            method: "claude.ai".into(),
            account_hint: None,
            plan_hint: Some("Claude Pro".into()),
        };
        assert!(matches!(
            apply_api_key_plan(auth, Some("sk-x")),
            AuthState::Authenticated { plan_hint: Some(p), .. } if p == "Anthropic API"
        ));
    }

    #[test]
    fn parses_claude_stream_transcript() {
        let lines = [
            r#"{"type":"system","subtype":"init","session_id":"sess-abc","model":"claude-sonnet-4-5"}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"thinking_delta","thinking":"Checking tests..."}}}"#,
            r#"{"type":"stream_event","event":{"type":"content_block_delta","delta":{"type":"text_delta","text":"Hello world"}}}"#,
            r#"{"type":"assistant","message":{"content":[{"type":"tool_use","id":"tu_1","name":"Bash","input":{"command":"cargo test"}}]}}"#,
            r#"{"type":"user","message":{"content":[{"type":"tool_result","tool_use_id":"tu_1","content":"ok 5 passed","is_error":false}]}}"#,
            r#"{"type":"result","is_error":false,"result":"Hello world","total_cost_usd":0.012,"usage":{"input_tokens":120,"output_tokens":45,"cache_read_input_tokens":80}}"#,
        ];

        let mut saw_deltas = false;
        let mut all = Vec::new();
        for l in lines {
            all.extend(parse_claude_stream_line(l, &mut saw_deltas));
        }

        assert!(saw_deltas);
        assert!(all.iter().any(|e| matches!(
            e,
            AgentEvent::SessionInit {
                provider_session_id: Some(s),
                resolved_model: Some(m)
            } if s == "sess-abc" && m == "claude-sonnet-4-5"
        )));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::ThoughtDelta { text } if text == "Checking tests...")));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::TextDelta { text } if text == "Hello world")));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::ToolCallStarted { name, kind: ToolKind::Shell, .. } if name == "Bash")));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::ToolCallCompleted { call_id, success: true, .. } if call_id == "tu_1")));
        assert!(all.iter().any(|e| matches!(e, AgentEvent::TurnCompleted { exit_reason: TurnExitReason::Completed, .. })));
    }
}
